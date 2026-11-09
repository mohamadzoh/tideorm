//! A PostgreSQL schema read from its catalog: the tables in dependency
//! order, then what is restored once every table exists.

use super::postgres_dependencies::postgres_dependencies;
use super::postgres_tables::postgres_table;
use super::*;

pub(super) async fn introspect_postgres(conn: &OrmConnection) -> Result<Catalog> {
    use crate::internal::TransactionTrait;
    let txn = conn.begin().await.map_err(translate_error)?;
    txn.execute_unprepared("SET LOCAL search_path = pg_catalog")
        .await
        .map_err(translate_error)?;
    let catalog = introspect_postgres_catalog(&txn).await?;
    txn.commit().await.map_err(translate_error)?;
    Ok(catalog)
}

async fn introspect_postgres_catalog(conn: &impl ConnectionTrait) -> Result<Catalog> {
    let pg = DatabaseType::Postgres;
    let table_rows = query(
        conn,
        pg,
        "WITH RECURSIVE tables AS (
            SELECT c.oid, n.nspname, c.relname, c.relispartition, c.relowner, c.relpersistence, c.reloptions,
                   pg_catalog.pg_get_partkeydef(c.oid) AS partition_key,
                   pg_catalog.pg_get_expr(c.relpartbound, c.oid) AS partition_bound
            FROM pg_catalog.pg_class c
            JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind IN ('r', 'p')
              AND n.nspname NOT IN ('information_schema', 'pg_catalog')
              AND n.nspname !~ '^pg_'
              AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=c.oid AND d.deptype='e')
         ), hierarchy AS (
            SELECT t.oid, 0 AS depth FROM tables t
            WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits i WHERE i.inhrelid = t.oid)
            UNION ALL
            SELECT t.oid, h.depth + 1 FROM tables t
            JOIN pg_catalog.pg_inherits i ON i.inhrelid = t.oid
            JOIN hierarchy h ON h.oid = i.inhparent
         )
         SELECT t.oid AS table_id, t.nspname::text AS table_schema, t.relname::text AS table_name,
                t.partition_key, t.partition_bound, t.relpersistence::text AS persistence,
                CASE WHEN t.reloptions IS NULL THEN NULL ELSE array_to_string(t.reloptions, ', ') END AS storage_options,
                format('ALTER TABLE %I.%I OWNER TO %I', t.nspname, t.relname, pg_catalog.pg_get_userbyid(t.relowner)) AS owner,
                CASE WHEN t.relispartition THEN parents.names END AS partition_parent,
                CASE WHEN NOT t.relispartition THEN parents.names END AS inheritance_parents
         FROM tables t JOIN (SELECT oid, max(depth) AS depth FROM hierarchy GROUP BY oid) h USING(oid)
         LEFT JOIN LATERAL (
             SELECT string_agg(format('%I.%I', pn.nspname, p.relname), ', ' ORDER BY i.inhseqno) AS names
             FROM pg_catalog.pg_inherits i
             JOIN pg_catalog.pg_class p ON p.oid = i.inhparent
             JOIN pg_catalog.pg_namespace pn ON pn.oid = p.relnamespace
             WHERE i.inhrelid = t.oid
         ) parents ON true
         ORDER BY h.depth, t.nspname, t.relname",
        Vec::new(),
    )
    .await?;

    let mut objects = postgres_dependencies(conn).await?;
    let mut catalog = Catalog::default();

    let mut foreign_keys = Vec::new();
    let mut routine_checks = Vec::new();
    for row in table_rows {
        let table_id: i64 = get(&row, "table_id")?;
        let definitions = postgres_table(
            conn,
            &row,
            &mut catalog,
            &mut foreign_keys,
            &mut routine_checks,
        )
        .await?;
        objects
            .nodes
            .insert(("table".into(), table_id), definitions);
    }
    catalog
        .verbatim
        .extend(std::mem::take(&mut objects.indexes));
    catalog
        .constraints
        .extend(std::mem::take(&mut objects.postlude));
    catalog.definitions = objects.ordered(conn).await?;
    // Add local child checks first, then let parent additions recurse and
    // merge them. This preserves local/inherited ownership of matching checks.
    for checks in routine_checks.into_iter().rev() {
        catalog.constraints.extend(checks);
    }
    catalog.verbatim.extend(statements(conn,
        "SELECT format('ALTER SEQUENCE %I.%I OWNED BY %I.%I.%I', sn.nspname, s.relname, tn.nspname, t.relname, a.attname) AS definition
         FROM pg_catalog.pg_class s JOIN pg_catalog.pg_namespace sn ON sn.oid = s.relnamespace
         JOIN pg_catalog.pg_depend d ON d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = s.oid
             AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype = 'a'
         JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
         JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
         JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = d.refobjsubid
         WHERE s.relkind = 'S' AND sn.nspname <> 'information_schema' AND sn.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid = 'pg_catalog.pg_class'::regclass AND e.objid = s.oid AND e.deptype = 'e')
         ORDER BY sn.nspname, s.relname", Vec::new(), &["definition"]).await?);
    // A NOT VALID CHECK is added after the hierarchy exists so descendants
    // keep its inherited ownership. A child created after such a check can
    // already have validated it; preserve that per-table state as well.
    catalog.constraints.extend(statements(conn,
        "SELECT format('ALTER TABLE %I.%I VALIDATE CONSTRAINT %I', n.nspname, t.relname, c.conname) AS definition
         FROM pg_catalog.pg_constraint c
         JOIN pg_catalog.pg_class t ON t.oid = c.conrelid
         JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace
         WHERE c.contype = 'c' AND c.convalidated AND c.coninhcount > 0
           AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, t.relname, c.conname", Vec::new(), &["definition"]).await?);
    // Referenced UNIQUE/PRIMARY constraints must exist before any foreign key,
    // regardless of how the tables sort by name.
    catalog.constraints.extend(foreign_keys);

    catalog
        .constraints
        .extend(postgres_late_statements(conn).await?);
    catalog
        .constraints
        .extend(postgres_privileges::postgres_privileges(conn).await?);
    catalog
        .constraints
        .push("RESET check_function_bodies".into());
    Ok(catalog)
}

/// What is restored once every table, constraint and foreign key exists:
/// row-level security policies and flags, triggers and rules with their
/// enabled states, replica identities and `CLUSTER` designations.
async fn postgres_late_statements(conn: &impl ConnectionTrait) -> Result<Vec<String>> {
    let mut output = statements(conn,
        "SELECT format('CREATE POLICY %I ON %I.%I AS %s FOR %s TO %s%s%s',
             pol.polname, n.nspname, c.relname,
             CASE WHEN pol.polpermissive THEN 'PERMISSIVE' ELSE 'RESTRICTIVE' END,
             CASE pol.polcmd WHEN '*' THEN 'ALL' WHEN 'r' THEN 'SELECT' WHEN 'a' THEN 'INSERT' WHEN 'w' THEN 'UPDATE' WHEN 'd' THEN 'DELETE' END,
             (SELECT string_agg(CASE WHEN role_id = 0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(role_id)) END, ', ' ORDER BY ord)
              FROM unnest(pol.polroles) WITH ORDINALITY AS roles(role_id, ord)),
             CASE WHEN pol.polqual IS NULL THEN '' ELSE ' USING (' || pg_catalog.pg_get_expr(pol.polqual, pol.polrelid) || ')' END,
             CASE WHEN pol.polwithcheck IS NULL THEN '' ELSE ' WITH CHECK (' || pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid) || ')' END) AS definition
         FROM pg_catalog.pg_policy pol
         JOIN pg_catalog.pg_class c ON c.oid = pol.polrelid
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, c.relname, pol.polname", Vec::new(), &["definition"]).await?;
    output.extend(statements(conn,
        "SELECT format('ALTER TABLE %I.%I %s ROW LEVEL SECURITY', n.nspname, c.relname, action) AS definition
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         CROSS JOIN LATERAL (SELECT 'ENABLE' AS action WHERE c.relrowsecurity
                             UNION ALL SELECT 'FORCE' WHERE c.relforcerowsecurity) flags
         WHERE c.relkind IN ('r', 'p') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, c.relname, action", Vec::new(), &["definition"]).await?);

    // Restore user triggers after functions, tables, indexes and constraints.
    // Partition trigger clones are created by their parent's CREATE TRIGGER;
    // their individual enabled states still need to be restored.
    for row in query(conn, DatabaseType::Postgres,
        "SELECT CASE WHEN tg.tgparentid = 0 THEN pg_catalog.pg_get_triggerdef(tg.oid, false) END AS definition,
                format('ALTER TABLE ONLY %I.%I %s TRIGGER %I', ns.nspname, c.relname,
                    CASE tg.tgenabled WHEN 'D' THEN 'DISABLE' WHEN 'R' THEN 'ENABLE REPLICA'
                         WHEN 'A' THEN 'ENABLE ALWAYS' ELSE 'ENABLE' END, tg.tgname) AS state
         FROM pg_catalog.pg_trigger tg
         JOIN pg_catalog.pg_class c ON c.oid = tg.tgrelid
         JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
         WHERE NOT tg.tgisinternal AND c.relkind IN ('r', 'p', 'v')
           AND ns.nspname <> 'information_schema' AND ns.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid = 'pg_catalog.pg_trigger'::regclass
                           AND d.objid = tg.oid AND d.deptype = 'e')
         ORDER BY tg.tgparentid <> 0, ns.nspname, c.relname, tg.tgname", Vec::new()).await? {
        if let Some(definition) = get::<Option<String>>(&row, "definition")? {
            output.push(definition);
        }
        output.push(get(&row, "state")?);
    }
    output.extend(statements(conn,
        "SELECT pg_catalog.pg_get_ruledef(r.oid, false) AS definition,
             format('ALTER TABLE %I.%I %s RULE %I', n.nspname, c.relname,
                 CASE r.ev_enabled WHEN 'D' THEN 'DISABLE' WHEN 'R' THEN 'ENABLE REPLICA'
                     WHEN 'A' THEN 'ENABLE ALWAYS' ELSE 'ENABLE' END, r.rulename) AS state
         FROM pg_catalog.pg_rewrite r JOIN pg_catalog.pg_class c ON c.oid=r.ev_class
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         WHERE r.rulename <> '_RETURN' AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_rewrite'::regclass AND d.objid=r.oid AND d.deptype='e')
         ORDER BY n.nspname,c.relname,r.rulename", Vec::new(), &["definition", "state"]).await?);
    // Recursive CREATE INDEX chooses new names for descendant indexes. Locate
    // an inherited replica index by its root and owning table after replay,
    // including nested partitions and constraint-backed indexes.
    output.extend(statements(conn,
        "SELECT CASE WHEN root.name IS NOT NULL THEN
             format('DO %L', format('DECLARE replica_index text; BEGIN
                 WITH RECURSIVE family(oid) AS (
                     SELECT %L::regclass::oid
                     UNION ALL
                     SELECT inhrelid FROM pg_catalog.pg_inherits JOIN family ON inhparent=family.oid
                 )
                 SELECT idx.relname INTO STRICT replica_index FROM family
                 JOIN pg_catalog.pg_index pi ON pi.indexrelid=family.oid
                 JOIN pg_catalog.pg_class idx ON idx.oid=pi.indexrelid
                 WHERE pi.indrelid=%L::regclass;
                 EXECUTE %L || pg_catalog.quote_ident(replica_index);
             END', root.name, format('%I.%I', n.nspname,c.relname),
                 format('ALTER TABLE %I.%I REPLICA IDENTITY USING INDEX ', n.nspname,c.relname)))
             ELSE format('ALTER TABLE %I.%I REPLICA IDENTITY %s', n.nspname,c.relname,
                 CASE c.relreplident WHEN 'f' THEN 'FULL' WHEN 'n' THEN 'NOTHING' WHEN 'd' THEN 'DEFAULT'
                     WHEN 'i' THEN 'USING INDEX ' || quote_ident(i.relname) END) END AS definition
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         LEFT JOIN pg_catalog.pg_index ix ON ix.indrelid=c.oid AND ix.indisreplident
         LEFT JOIN pg_catalog.pg_class i ON i.oid=ix.indexrelid
         LEFT JOIN LATERAL (
             WITH RECURSIVE ancestors(oid) AS (
                 SELECT i.oid
                 UNION ALL
                 SELECT inhparent FROM pg_catalog.pg_inherits JOIN ancestors ON inhrelid=ancestors.oid
             )
             SELECT format('%I.%I', ns.nspname, idx.relname) AS name
             FROM ancestors JOIN pg_catalog.pg_class idx ON idx.oid=ancestors.oid
             JOIN pg_catalog.pg_namespace ns ON ns.oid=idx.relnamespace
             WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits WHERE inhrelid=idx.oid)
         ) root ON EXISTS (SELECT 1 FROM pg_catalog.pg_inherits WHERE inhrelid=i.oid)
         WHERE c.relkind IN ('r','p') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=c.oid AND d.deptype='e')
         ORDER BY n.nspname,c.relname", Vec::new(), &["definition"]).await?);
    // Internal trigger names contain source OIDs. Resolve the corresponding
    // constraint and trigger in the destination instead of replaying that name.
    output.extend(statements(conn,
        "SELECT format('DO %L', format('DECLARE trigger_name text; BEGIN
             WITH RECURSIVE family(oid) AS (
                 SELECT oid FROM pg_catalog.pg_constraint WHERE conrelid=%L::regclass AND conname=%L
                 UNION ALL SELECT child.oid FROM pg_catalog.pg_constraint child JOIN family ON child.conparentid=family.oid
             )
             SELECT target.tgname INTO STRICT trigger_name FROM pg_catalog.pg_trigger target
             JOIN family ON target.tgconstraint=family.oid
             JOIN pg_catalog.pg_constraint constraint_row ON constraint_row.oid=family.oid
             WHERE target.tgisinternal AND target.tgrelid=%L::regclass AND target.tgtype=%s
               AND target.tgfoid=%L::regproc
               AND constraint_row.conrelid=%L::regclass
               AND constraint_row.confrelid=COALESCE(%L::regclass,0);
             EXECUTE %L || pg_catalog.quote_ident(trigger_name);
         END', root.relation, root.name, format('%I.%I', n.nspname,t.relname), tg.tgtype,
             format('%I.%I', fn.nspname,p.proname), c.conrelid::regclass::text,
             CASE WHEN c.confrelid=0 THEN NULL ELSE c.confrelid::regclass::text END,
             format('ALTER TABLE ONLY %I.%I %s TRIGGER ', n.nspname,t.relname,
                 CASE tg.tgenabled WHEN 'D' THEN 'DISABLE' WHEN 'R' THEN 'ENABLE REPLICA' ELSE 'ENABLE ALWAYS' END))) AS definition
         FROM pg_catalog.pg_trigger tg
         JOIN pg_catalog.pg_class t ON t.oid=tg.tgrelid
         JOIN pg_catalog.pg_namespace n ON n.oid=t.relnamespace
         JOIN pg_catalog.pg_constraint c ON c.oid=tg.tgconstraint
         JOIN pg_catalog.pg_proc p ON p.oid=tg.tgfoid
         JOIN pg_catalog.pg_namespace fn ON fn.oid=p.pronamespace
         CROSS JOIN LATERAL (
             WITH RECURSIVE ancestors AS (
                 SELECT c.oid,c.conparentid,c.conrelid,c.conname
                 UNION ALL SELECT parent.oid,parent.conparentid,parent.conrelid,parent.conname
                 FROM pg_catalog.pg_constraint parent JOIN ancestors ON parent.oid=ancestors.conparentid
             ) SELECT conrelid::regclass::text AS relation,conname AS name FROM ancestors WHERE conparentid=0
         ) root
         WHERE tg.tgisinternal AND tg.tgenabled <> 'O'
           AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=t.oid AND d.deptype='e')
         ORDER BY n.nspname,t.relname,tg.oid", Vec::new(), &["definition"]).await?);
    // CLUSTER's index designation is separate from CREATE INDEX. Resolve
    // inherited indexes through their root because their names may change.
    output.extend(statements(conn,
        "SELECT format('DO %L', format('DECLARE cluster_index text; BEGIN
             WITH RECURSIVE family(oid) AS (
                 SELECT %L::regclass::oid
                 UNION ALL SELECT inhrelid FROM pg_catalog.pg_inherits JOIN family ON inhparent=family.oid
             )
             SELECT idx.relname INTO STRICT cluster_index FROM family
             JOIN pg_catalog.pg_index pi ON pi.indexrelid=family.oid
             JOIN pg_catalog.pg_class idx ON idx.oid=pi.indexrelid
             WHERE pi.indrelid=%L::regclass;
             EXECUTE %L || pg_catalog.quote_ident(cluster_index);
         END', root.name, format('%I.%I', n.nspname,t.relname),
             format('ALTER TABLE %I.%I CLUSTER ON ', n.nspname,t.relname))) AS definition
         FROM pg_catalog.pg_index ix JOIN pg_catalog.pg_class t ON t.oid=ix.indrelid
         JOIN pg_catalog.pg_namespace n ON n.oid=t.relnamespace
         CROSS JOIN LATERAL (
             WITH RECURSIVE ancestors(oid) AS (
                 SELECT ix.indexrelid
                 UNION ALL SELECT inhparent FROM pg_catalog.pg_inherits JOIN ancestors ON inhrelid=ancestors.oid
             ) SELECT format('%I.%I', ns.nspname,i.relname) AS name FROM ancestors
             JOIN pg_catalog.pg_class i ON i.oid=ancestors.oid JOIN pg_catalog.pg_namespace ns ON ns.oid=i.relnamespace
             WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits WHERE inhrelid=i.oid)
         ) root
         WHERE ix.indisclustered AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=t.oid AND d.deptype='e')
         ORDER BY n.nspname,t.relname", Vec::new(), &["definition"]).await?);
    Ok(output)
}
