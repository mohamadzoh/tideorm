use super::*;

/// Catalog objects ordered together with tables, not in separate type/routine phases.
#[derive(Default)]
pub(super) struct PostgresObjects {
    pub(super) prelude: Vec<String>,
    pub(super) indexes: Vec<String>,
    pub(super) postlude: Vec<String>,
    pub(super) nodes: BTreeMap<(String, i64), Vec<String>>,
}

pub(super) async fn postgres_dependencies(conn: &impl ConnectionTrait) -> Result<PostgresObjects> {
    // These objects can be required by views, constraints, indexes or routines.
    // Refuse the export before write_schema touches its output file.
    let mut unsupported = query(conn, Backend::Postgres,
        "SELECT pg_catalog.pg_describe_object(o.classid, o.oid, 0) AS object
         FROM (
             SELECT 'pg_catalog.pg_proc'::regclass AS classid, oid, pronamespace AS namespace
             FROM pg_catalog.pg_proc WHERE prokind='a'
             UNION ALL SELECT 'pg_catalog.pg_class'::regclass, c.oid, c.relnamespace
             FROM pg_catalog.pg_foreign_table f JOIN pg_catalog.pg_class c ON c.oid=f.ftrelid
             UNION ALL SELECT 'pg_catalog.pg_operator'::regclass, oid, oprnamespace FROM pg_catalog.pg_operator
             UNION ALL SELECT 'pg_catalog.pg_ts_config'::regclass, oid, cfgnamespace FROM pg_catalog.pg_ts_config
             UNION ALL SELECT 'pg_catalog.pg_ts_dict'::regclass, oid, dictnamespace FROM pg_catalog.pg_ts_dict
             UNION ALL SELECT 'pg_catalog.pg_ts_parser'::regclass, oid, prsnamespace FROM pg_catalog.pg_ts_parser
             UNION ALL SELECT 'pg_catalog.pg_ts_template'::regclass, oid, tmplnamespace FROM pg_catalog.pg_ts_template
         ) o JOIN pg_catalog.pg_namespace n ON n.oid=o.namespace
         WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
                           WHERE d.classid=o.classid AND d.objid=o.oid AND d.deptype='e')
         ORDER BY object", Vec::new()).await?;
    unsupported.extend(query(conn, Backend::Postgres,
        "SELECT format('foreign server %I', srvname) AS object FROM pg_catalog.pg_foreign_server ORDER BY srvname",
        Vec::new()).await?);
    if !unsupported.is_empty() {
        let objects = unsupported
            .iter()
            .map(|row| get::<String>(row, "object"))
            .collect::<Result<Vec<_>>>()?;
        return Err(Error::query(format!(
            "PostgreSQL schema export does not support these objects: {}",
            objects.join(", ")
        )));
    }
    let mut statements = Vec::new();
    let mut postlude = Vec::new();
    for row in query(
        conn,
        Backend::Postgres,
        "SELECT format('CREATE SCHEMA IF NOT EXISTS %I', nspname) AS definition,
                format('ALTER SCHEMA %I OWNER TO %I', nspname, pg_catalog.pg_get_userbyid(nspowner)) AS owner
         FROM pg_catalog.pg_namespace
         WHERE nspname <> 'information_schema' AND nspname !~ '^pg_'
         ORDER BY nspname",
        Vec::new(),
    )
    .await?
    {
        statements.push(get(&row, "definition")?);
        statements.push(get(&row, "owner")?);
    }
    for row in query(
        conn,
        Backend::Postgres,
        "SELECT format('CREATE EXTENSION IF NOT EXISTS %I WITH SCHEMA %I VERSION %L',
                       e.extname, n.nspname, e.extversion) AS definition
         FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace
         WHERE e.extname <> 'plpgsql' ORDER BY e.extname",
        Vec::new(),
    )
    .await?
    {
        statements.push(get(&row, "definition")?);
    }
    for row in query(conn, Backend::Postgres,
        "SELECT format('CREATE COLLATION %I.%I (provider = %s, %s, deterministic = %s%s)',
            n.nspname, c.collname,
            CASE c.collprovider WHEN 'i' THEN 'icu' WHEN 'b' THEN 'builtin' ELSE 'libc' END,
            CASE WHEN c.collprovider = 'c' THEN format('lc_collate = %L, lc_ctype = %L', c.collcollate, c.collctype)
                 ELSE format('locale = %L', COALESCE(to_jsonb(c)->>'colllocale', to_jsonb(c)->>'colliculocale')) END,
            CASE WHEN c.collisdeterministic THEN 'true' ELSE 'false' END,
            CASE WHEN to_jsonb(c)->>'collicurules' IS NULL THEN ''
                 ELSE format(', rules = %L', to_jsonb(c)->>'collicurules') END) AS definition,
            format('ALTER COLLATION %I.%I OWNER TO %I', n.nspname, c.collname, pg_catalog.pg_get_userbyid(c.collowner)) AS owner
         FROM pg_catalog.pg_collation c JOIN pg_catalog.pg_namespace n ON n.oid = c.collnamespace
         WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
               WHERE d.classid = 'pg_catalog.pg_collation'::regclass AND d.objid = c.oid AND d.deptype = 'e')
         ORDER BY n.nspname, c.collname", Vec::new()).await? {
        statements.push(get(&row, "definition")?);
        statements.push(get(&row, "owner")?);
    }

    // Identity sequences are recreated by their column clauses. Other
    // sequences must exist before defaults referencing them are restored.
    for row in query(conn, Backend::Postgres,
        "SELECT format('CREATE %sSEQUENCE %I.%I AS %s START WITH %s INCREMENT BY %s MINVALUE %s MAXVALUE %s CACHE %s %s',
            CASE c.relpersistence WHEN 'u' THEN 'UNLOGGED ' ELSE '' END,
            n.nspname, c.relname, pg_catalog.format_type(s.seqtypid, NULL),
            s.seqstart, s.seqincrement, s.seqmin, s.seqmax, s.seqcache,
            CASE WHEN s.seqcycle THEN 'CYCLE' ELSE 'NO CYCLE' END) AS definition,
            format('ALTER SEQUENCE %I.%I OWNER TO %I', n.nspname, c.relname, pg_catalog.pg_get_userbyid(c.relowner)) AS owner
         FROM pg_catalog.pg_sequence s JOIN pg_catalog.pg_class c ON c.oid = s.seqrelid
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
               WHERE d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = c.oid AND d.deptype IN ('i', 'e'))
         ORDER BY n.nspname, c.relname", Vec::new()).await? {
        statements.push(get(&row, "definition")?);
        statements.push(get(&row, "owner")?);
    }

    let rows = query(conn, Backend::Postgres,
        "WITH RECURSIVE edges AS (
             SELECT objid AS owner, refobjid AS dependency FROM pg_catalog.pg_depend
             WHERE classid = 'pg_catalog.pg_type'::regclass
               AND refclassid = 'pg_catalog.pg_type'::regclass
             UNION
             SELECT t.oid, a.atttypid FROM pg_catalog.pg_type t
             JOIN pg_catalog.pg_attribute a ON a.attrelid = t.typrelid
             WHERE a.attnum > 0 AND NOT a.attisdropped
         ), required(oid) AS (
             SELECT t.oid FROM pg_catalog.pg_type t
             JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace
             LEFT JOIN pg_catalog.pg_class c ON c.oid=t.typrelid
             WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
               AND NOT (t.typelem <> 0 AND t.typlen=-1)
               AND (t.typrelid=0 OR c.relkind='c')
               AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_type'::regclass AND d.objid=t.oid AND d.deptype IN ('e','i'))
             UNION
             SELECT a.atttypid FROM pg_catalog.pg_attribute a
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE c.relkind IN ('r', 'p', 'v', 'm') AND a.attnum > 0 AND NOT a.attisdropped
               AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
             UNION
             SELECT d.refobjid FROM pg_catalog.pg_depend d
             JOIN pg_catalog.pg_proc p ON d.classid = 'pg_catalog.pg_proc'::regclass AND d.objid = p.oid
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
             WHERE d.refclassid = 'pg_catalog.pg_type'::regclass AND p.prokind IN ('f', 'p')
               AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
               AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid = 'pg_catalog.pg_proc'::regclass
                               AND e.objid = p.oid AND e.deptype = 'e')
             UNION
             SELECT e.dependency FROM required r JOIN edges e ON e.owner = r.oid
         )
         SELECT t.oid::bigint AS id, format('%I.%I', n.nspname, t.typname) AS name,
                (t.typelem <> 0 AND t.typlen = -1) OR COALESCE(c.relkind IN ('r', 'p', 'v', 'm'), false) OR EXISTS (
                    SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid = 'pg_catalog.pg_type'::regclass
                    AND d.objid = t.oid AND d.deptype = 'e') AS implicit,
                CASE t.typtype
                  WHEN 'e' THEN format('CREATE TYPE %I.%I AS ENUM (%s)', n.nspname, t.typname,
                    (SELECT string_agg(quote_literal(enumlabel), ', ' ORDER BY enumsortorder)
                     FROM pg_catalog.pg_enum WHERE enumtypid = t.oid))
                  WHEN 'd' THEN format('CREATE DOMAIN %I.%I AS %s%s%s%s%s', n.nspname, t.typname,
                    pg_catalog.format_type(t.typbasetype, t.typtypmod),
                    COALESCE((SELECT format(' COLLATE %I.%I', cn.nspname, co.collname) FROM pg_catalog.pg_collation co JOIN pg_catalog.pg_namespace cn ON cn.oid=co.collnamespace WHERE co.oid=t.typcollation), ''),
                    '',
                    CASE WHEN t.typnotnull AND NOT EXISTS (
                        SELECT 1 FROM pg_catalog.pg_constraint WHERE contypid = t.oid AND contype = 'n'
                    ) THEN ' NOT NULL' ELSE '' END,
                    COALESCE((SELECT string_agg(format(' CONSTRAINT %I %s', conname,
                        pg_catalog.pg_get_constraintdef(oid)), '' ORDER BY conname)
                        FROM pg_catalog.pg_constraint WHERE contypid = t.oid AND contype <> 'c'), ''))
                  WHEN 'c' THEN CASE WHEN c.relkind = 'c' THEN format('CREATE TYPE %I.%I AS (%s)', n.nspname, t.typname,
                    (SELECT string_agg(format('%I %s%s', a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod),
                        COALESCE((SELECT format(' COLLATE %I.%I', cn.nspname, co.collname) FROM pg_catalog.pg_collation co JOIN pg_catalog.pg_namespace cn ON cn.oid=co.collnamespace WHERE co.oid=a.attcollation), '')),
                        ', ' ORDER BY a.attnum) FROM pg_catalog.pg_attribute a
                     WHERE a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped)) END
                END AS definition
         FROM required r JOIN pg_catalog.pg_type t ON t.oid = r.oid
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
         LEFT JOIN pg_catalog.pg_class c ON c.oid = t.typrelid
         WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, t.typname", Vec::new()).await?;

    for row in query(conn, Backend::Postgres,
        "SELECT format('ALTER DOMAIN %I.%I SET DEFAULT %s', n.nspname, t.typname, pg_catalog.pg_get_expr(t.typdefaultbin, 0)) AS definition
         FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace
         WHERE t.typtype='d' AND t.typdefaultbin IS NOT NULL
           AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_type'::regclass AND d.objid=t.oid AND d.deptype='e')
         UNION ALL
         SELECT format('ALTER DOMAIN %I.%I ADD CONSTRAINT %I %s', n.nspname, t.typname, c.conname, pg_catalog.pg_get_constraintdef(c.oid))
         FROM pg_catalog.pg_constraint c JOIN pg_catalog.pg_type t ON t.oid=c.contypid
         JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace
         WHERE c.contype='c' AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_type'::regclass AND d.objid=t.oid AND d.deptype='e')",
        Vec::new()).await? {
        postlude.push(get(&row, "definition")?);
    }
    let mut nodes = BTreeMap::new();
    let mut indexes = Vec::new();
    for row in rows {
        let id: i64 = get(&row, "id")?;
        let name: String = get(&row, "name")?;
        let definition: Option<String> = get(&row, "definition")?;
        let implicit: bool = get(&row, "implicit")?;
        if !implicit && definition.is_none() {
            return Err(Error::query(format!(
                "Cannot export PostgreSQL type {name}: its definition requires objects not supported by schema export"
            )));
        }
        nodes.insert(
            ("type".into(), id),
            if implicit {
                Vec::new()
            } else {
                definition.into_iter().collect()
            },
        );
    }
    // As in pg_dump, defer validation of quoted function bodies: a SQL or
    // PL/pgSQL body can refer to a table restored later. Parsed SQL bodies
    // still have catalog dependencies, which are ordered below.
    statements.push("SET check_function_bodies = false".into());

    for row in query(conn, Backend::Postgres,
        "SELECT p.oid::bigint AS id, pg_catalog.pg_get_functiondef(p.oid) AS definition,
                format('ALTER %s %I.%I(%s) OWNER TO %I',
                    CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END,
                    n.nspname, p.proname, pg_catalog.pg_get_function_identity_arguments(p.oid),
                    pg_catalog.pg_get_userbyid(p.proowner)) AS owner
         FROM pg_catalog.pg_proc p
         JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
         WHERE p.prokind IN ('f', 'p') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
                           WHERE d.classid = 'pg_catalog.pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e')
         ORDER BY n.nspname, p.proname, p.oid", Vec::new()).await? {
        let id: i64 = get(&row, "id")?;
        let definitions = vec![get(&row, "definition")?, get(&row, "owner")?];
        nodes.insert(("routine".into(), id), definitions);
    }
    for row in query(conn, Backend::Postgres,
        "SELECT c.oid::bigint AS id,
                format('CREATE %s %I.%I (%s)%s AS %s%s',
                    CASE c.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END,
                    n.nspname, c.relname,
                    (SELECT string_agg(quote_ident(a.attname), ', ' ORDER BY a.attnum)
                     FROM pg_catalog.pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
                    CASE WHEN c.reloptions IS NULL THEN '' ELSE ' WITH (' || array_to_string(c.reloptions, ', ') || ')' END,
                    rtrim(pg_catalog.pg_get_viewdef(c.oid, false), ';' || chr(10) || chr(13) || ' '),
                    CASE c.relkind WHEN 'm' THEN ' WITH NO DATA' ELSE '' END) AS definition,
                format('ALTER %s %I.%I OWNER TO %I',
                    CASE c.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END,
                    n.nspname, c.relname, pg_catalog.pg_get_userbyid(c.relowner)) AS owner
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         WHERE c.relkind IN ('v', 'm') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass
                           AND d.objid=c.oid AND d.deptype='e')
         ORDER BY n.nspname,c.relname", Vec::new()).await? {
        let id: i64 = get(&row, "id")?;
        let definitions = vec![get(&row, "definition")?, get(&row, "owner")?];
        for index in query(conn, Backend::Postgres,
            "SELECT pg_catalog.pg_get_indexdef(indexrelid) AS definition FROM pg_catalog.pg_index WHERE indrelid=$1::oid ORDER BY indexrelid",
            vec![id.into()]).await? {
            indexes.push(get(&index, "definition")?);
        }
        nodes.insert(("table".into(), id), definitions);
    }
    for row in query(conn, Backend::Postgres,
        "SELECT format('ALTER VIEW %I.%I ALTER COLUMN %I SET DEFAULT %s', n.nspname, c.relname, a.attname, pg_catalog.pg_get_expr(d.adbin,d.adrelid)) AS definition
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
         JOIN pg_catalog.pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum
         WHERE c.relkind='v' AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_class'::regclass AND e.objid=c.oid AND e.deptype='e')
         ORDER BY n.nspname,c.relname,a.attnum",Vec::new()).await? {
        postlude.push(get(&row, "definition")?);
    }
    Ok(PostgresObjects {
        prelude: statements,
        nodes,
        indexes,
        postlude,
    })
}

pub(super) fn literal(value: &str) -> String {
    format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

impl PostgresObjects {
    pub(super) async fn ordered(mut self, conn: &impl ConnectionTrait) -> Result<Vec<String>> {
        // Attribute defaults are restored later. Generated expressions and
        // CHECKs without routine dependencies are part of CREATE TABLE. Checks
        // calling user routines are restored after tables to avoid row-type cycles.
        // Row types and array types are implicit nodes linking their users to
        // the table or element type that actually creates them.
        let rows = query(conn, Backend::Postgres,
            "WITH objects AS (
                SELECT 'type' AS kind, oid, 'pg_catalog.pg_type'::regclass AS classid FROM pg_catalog.pg_type
                UNION ALL SELECT 'routine', oid, 'pg_catalog.pg_proc'::regclass FROM pg_catalog.pg_proc
                UNION ALL SELECT 'table', oid, 'pg_catalog.pg_class'::regclass FROM pg_catalog.pg_class
             ), edges AS (
                SELECT o.kind, o.oid, r.kind AS refkind, r.oid AS refoid
                FROM pg_catalog.pg_depend d
                JOIN objects o ON o.classid=d.classid AND o.oid=d.objid
                JOIN objects r ON r.classid=d.refclassid AND r.oid=d.refobjid
                WHERE d.deptype <> 'e' AND NOT (
                    o.kind='type' AND r.kind IN ('routine','table') AND EXISTS (
                        SELECT 1 FROM pg_catalog.pg_type t WHERE t.oid=o.oid AND t.typtype='d'))
                UNION
                SELECT CASE WHEN c.contypid <> 0 THEN 'type' ELSE 'table' END,
                       CASE WHEN c.contypid <> 0 THEN c.contypid ELSE c.conrelid END, r.kind, r.oid
                FROM pg_catalog.pg_constraint c
                JOIN pg_catalog.pg_depend d ON d.classid='pg_catalog.pg_constraint'::regclass AND d.objid=c.oid
                JOIN objects r ON r.classid=d.refclassid AND r.oid=d.refobjid
                WHERE c.contype='c' AND c.contypid=0 AND c.convalidated AND NOT EXISTS (
                    SELECT 1 FROM pg_catalog.pg_depend f WHERE f.classid='pg_catalog.pg_constraint'::regclass
                    AND f.objid=c.oid AND f.refclassid='pg_catalog.pg_proc'::regclass)
                UNION
                SELECT 'table', rw.ev_class, r.kind, r.oid FROM pg_catalog.pg_rewrite rw
                JOIN pg_catalog.pg_depend d ON d.classid='pg_catalog.pg_rewrite'::regclass AND d.objid=rw.oid
                JOIN objects r ON r.classid=d.refclassid AND r.oid=d.refobjid
                WHERE rw.rulename='_RETURN'
                UNION
                SELECT 'table', a.adrelid, r.kind, r.oid FROM pg_catalog.pg_attrdef a
                JOIN pg_catalog.pg_attribute col ON col.attrelid=a.adrelid AND col.attnum=a.adnum
                JOIN pg_catalog.pg_depend d ON d.classid='pg_catalog.pg_attrdef'::regclass AND d.objid=a.oid
                JOIN objects r ON r.classid=d.refclassid AND r.oid=d.refobjid
                WHERE col.attgenerated <> ''
                UNION
                SELECT 'table', inhrelid, 'table', inhparent FROM pg_catalog.pg_inherits
                UNION
                SELECT 'type', t.oid, 'type', a.atttypid FROM pg_catalog.pg_type t
                JOIN pg_catalog.pg_class c ON c.oid=t.typrelid AND c.relkind='c'
                JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
             ) SELECT kind, oid::bigint AS id, refkind, refoid::bigint AS refid FROM edges",
            Vec::new()).await?;
        let mut dependencies: BTreeMap<(String, i64), Vec<(String, i64)>> = BTreeMap::new();
        for row in rows {
            let owner = (get::<String>(&row, "kind")?, get::<i64>(&row, "id")?);
            let dependency = (get::<String>(&row, "refkind")?, get::<i64>(&row, "refid")?);
            if owner != dependency && self.nodes.contains_key(&dependency) {
                dependencies.entry(owner).or_default().push(dependency);
            }
        }
        let mut statements = self.prelude;
        while !self.nodes.is_empty() {
            let ready: Vec<_> = self
                .nodes
                .keys()
                .filter(|key| {
                    dependencies
                        .get(*key)
                        .is_none_or(|deps| deps.iter().all(|dep| !self.nodes.contains_key(dep)))
                })
                .cloned()
                .collect();
            if ready.is_empty() {
                return Err(Error::query(
                    "Cannot export cyclic PostgreSQL object dependencies",
                ));
            }
            for key in ready {
                statements.extend(self.nodes.remove(&key).unwrap());
            }
        }
        Ok(statements)
    }
}
