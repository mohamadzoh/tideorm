//! Object ACLs are restored explicitly, including empty ACLs. Destination
//! default privileges must not grant access absent from the source.
use super::*;

pub(super) async fn postgres_privileges(conn: &impl ConnectionTrait) -> Result<Vec<String>> {
    let mut output = Vec::new();
    let objects = query(conn, Backend::Postgres,
        "WITH objects AS (
          SELECT format('SCHEMA %I',nspname) AS target, 'pg_catalog.pg_namespace' AS catalog,
                 format('nspname=%L',nspname) AS lookup, 'nspacl' AS acl_column, 'nspowner' AS owner_column,
                 'n' AS kind,nspowner AS owner,nspacl AS acl,NULL::text AS owner_statement,quote_ident(nspname) AS namespace
          FROM pg_catalog.pg_namespace WHERE nspname <> 'information_schema' AND nspname !~ '^pg_'
          UNION ALL
          SELECT format('%s %I.%I',CASE c.relkind WHEN 'S' THEN 'SEQUENCE' ELSE 'TABLE' END,n.nspname,c.relname),
                 'pg_catalog.pg_class',format('oid=%L::regclass',format('%I.%I',n.nspname,c.relname)),
                 'relacl','relowner',CASE c.relkind WHEN 'S' THEN 'S' ELSE 'r' END,c.relowner,c.relacl,NULL,quote_ident(n.nspname)
          FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
          WHERE c.relkind IN ('r','p','v','m','S') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
            AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=c.oid AND d.deptype='e')
          UNION ALL
          SELECT format('TYPE %I.%I',n.nspname,t.typname),'pg_catalog.pg_type',
                 format('oid=%L::regtype',format('%I.%I',n.nspname,t.typname)),
                 'typacl','typowner','T',t.typowner,t.typacl,
                 format('ALTER TYPE %I.%I OWNER TO %I',n.nspname,t.typname,pg_catalog.pg_get_userbyid(t.typowner)),quote_ident(n.nspname)
          FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace
          LEFT JOIN pg_catalog.pg_class c ON c.oid=t.typrelid
          WHERE (t.typtype IN ('e','d') OR c.relkind='c') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
            AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_type'::regclass AND d.objid=t.oid AND d.deptype='e')
          UNION ALL
          SELECT format('%s %I.%I(%s)',CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END,n.nspname,p.proname,pg_catalog.pg_get_function_identity_arguments(p.oid)),
                 'pg_catalog.pg_proc',format('oid=%L::regprocedure',format('%I.%I(%s)',n.nspname,p.proname,pg_catalog.oidvectortypes(p.proargtypes))),
                 'proacl','proowner','f',p.proowner,p.proacl,NULL,quote_ident(n.nspname)
          FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
          WHERE p.prokind IN ('f','p') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
            AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_proc'::regclass AND d.objid=p.oid AND d.deptype='e')
        ) SELECT target,catalog,lookup,acl_column,owner_column,kind,owner_statement,namespace,quote_ident(pg_catalog.pg_get_userbyid(owner)) AS owner_name,
            (SELECT json_agg(json_build_object('role',CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(a.grantee)) END,
                'privilege',a.privilege_type,'grantable',a.is_grantable,'grantor',quote_ident(pg_catalog.pg_get_userbyid(a.grantor))) ORDER BY a.grantee,a.privilege_type)
             FROM pg_catalog.aclexplode(COALESCE(acl,pg_catalog.acldefault(kind::\"char\",owner))) a) AS grants
        FROM objects ORDER BY kind <> 'n', target", Vec::new()).await?;
    let mut table_grants = BTreeMap::new();
    let mut schema_acls = Vec::new();
    for row in objects {
        let target: String = get(&row, "target")?;
        let source_acl: Option<serde_json::Value> = get(&row, "grants")?;
        let statements = if target.starts_with("SCHEMA ") {
            &mut schema_acls
        } else {
            &mut output
        };
        if !target.starts_with("SCHEMA ") {
            grantor_access(statements, &get::<String>(&row, "namespace")?, &source_acl)?;
        }
        if let Some(owner) = get::<Option<String>>(&row, "owner_statement")? {
            statements.push(owner);
        }
        let acl: String = get(&row, "acl_column")?;
        let owner: String = get(&row, "owner_column")?;
        let kind: String = get(&row, "kind")?;
        let catalog: String = get(&row, "catalog")?;
        let lookup: String = get(&row, "lookup")?;
        let query = format!(
            "SELECT CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(a.grantee)) END FROM {catalog}, LATERAL pg_catalog.aclexplode(COALESCE({acl},pg_catalog.acldefault('{}',{owner}))) a WHERE {lookup}",
            kind
        );
        statements.push(revoke(&target, "ALL", &query));
        let source = grants(
            statements,
            &target,
            &get::<String>(&row, "owner_name")?,
            source_acl,
            &[],
        )?;
        table_grants.insert(target, source);
    }
    for row in query(conn, Backend::Postgres,
        "SELECT format('TABLE %I.%I',n.nspname,c.relname) AS target,quote_ident(pg_catalog.pg_get_userbyid(c.relowner)) AS owner_name,quote_ident(n.nspname) AS namespace,quote_ident(a.attname) AS column_name,
             format('SELECT CASE WHEN x.grantee=0 THEN ''PUBLIC'' ELSE quote_ident(pg_catalog.pg_get_userbyid(x.grantee)) END FROM pg_catalog.pg_attribute, LATERAL pg_catalog.aclexplode(attacl) x WHERE attrelid=%L::regclass AND attname=%L',format('%I.%I',n.nspname,c.relname),a.attname) AS lookup,
             (SELECT json_agg(json_build_object('role',CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(x.grantee)) END,
                 'privilege',format('%s (%I)',x.privilege_type,a.attname),'grantable',x.is_grantable,'grantor',quote_ident(pg_catalog.pg_get_userbyid(x.grantor)))) FROM pg_catalog.aclexplode(a.attacl) x) AS grants
         FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         WHERE c.relkind IN ('r','p','v','m') AND a.attnum>0 AND NOT a.attisdropped AND a.attacl IS NOT NULL
           AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=c.oid AND d.deptype='e')
         ORDER BY n.nspname,c.relname,a.attnum",Vec::new()).await? {
        let target: String=get(&row,"target")?;
        grantor_access(&mut output, &get::<String>(&row,"namespace")?, &get::<Option<serde_json::Value>>(&row,"grants")?)?;
        output.push(revoke(&target,&format!("ALL ({})",get::<String>(&row,"column_name")?),&get::<String>(&row,"lookup")?));
        grants(&mut output, &target, &get::<String>(&row, "owner_name")?, get(&row, "grants")?,
            table_grants.get(&target).map(Vec::as_slice).unwrap_or(&[]))?;
    }
    // Schema USAGE can have been revoked without removing an object grant
    // chain. Remove the temporary lookup access only after restoring that chain.
    output.extend(schema_acls);
    output.extend(default_privileges(conn).await?);
    Ok(output)
}

fn grantor_access(
    output: &mut Vec<String>,
    namespace: &str,
    source: &Option<serde_json::Value>,
) -> Result<()> {
    if let Some(serde_json::Value::Array(grants)) = source {
        let mut roles = std::collections::BTreeSet::new();
        for grant in grants {
            roles.insert(
                grant["grantor"]
                    .as_str()
                    .ok_or_else(|| Error::internal("invalid ACL grantor"))?,
            );
        }
        for role in roles {
            output.push(format!("GRANT USAGE ON SCHEMA {namespace} TO {role}"));
        }
    }
    Ok(())
}

fn revoke(target: &str, privileges: &str, lookup: &str) -> String {
    let body = format!(
        "DECLARE grantee_name text; BEGIN FOR grantee_name IN {lookup} LOOP EXECUTE {} || grantee_name || ' CASCADE'; END LOOP; END",
        postgres_dependencies::literal(&format!("REVOKE {privileges} ON {target} FROM "))
    );
    format!("DO {}", postgres_dependencies::literal(&body))
}
/// Grant dependencies are ordered before changing to each original grantor.
/// Using the restoring owner for every GRANT loses REVOKE CASCADE semantics.
#[derive(Clone, serde::Deserialize)]
struct Grant {
    role: String,
    privilege: String,
    grantable: bool,
    grantor: String,
}

fn grants(
    output: &mut Vec<String>,
    target: &str,
    owner: &str,
    grants: Option<serde_json::Value>,
    inherited: &[Grant],
) -> Result<Vec<Grant>> {
    let source: Vec<Grant> =
        serde_json::from_value(grants.unwrap_or_else(|| serde_json::json!([])))
            .map_err(|error| Error::internal(format!("invalid ACL: {error}")))?;
    let mut remaining = source.clone();
    let mut available = std::collections::HashSet::new();
    let mut body = String::from("DECLARE saved_role text := current_setting('role'); BEGIN ");
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .position(|grant| {
                grant.grantor == owner
                    || available.contains(&(grant.grantor.clone(), grant.privilege.clone()))
                    || inherited.iter().any(|parent| {
                        parent.grantable
                            && parent.role == grant.grantor
                            && parent.privilege == grant.privilege.split(" (").next().unwrap()
                    })
            })
            .ok_or_else(|| Error::query(format!("Cannot order privilege grantors for {target}")))?;
        let grant = remaining.remove(ready);
        let option = if grant.grantable {
            " WITH GRANT OPTION"
        } else {
            ""
        };
        for statement in [
            format!("SET LOCAL ROLE {}", grant.grantor),
            format!(
                "GRANT {} ON {target} TO {}{option}",
                grant.privilege, grant.role
            ),
        ] {
            body.push_str(&format!(
                "EXECUTE {}; ",
                postgres_dependencies::literal(&statement)
            ));
        }
        if grant.grantable {
            available.insert((grant.role, grant.privilege));
        }
    }
    // LOCAL confines settings to this statement outside a transaction. Restore
    // explicitly as well, so replay inside an existing transaction is identical.
    body.push_str("IF saved_role = 'none' THEN SET LOCAL ROLE NONE; ELSE EXECUTE 'SET LOCAL ROLE ' || quote_ident(saved_role); END IF; END");
    if !source.is_empty() {
        output.push(format!("DO {}", postgres_dependencies::literal(&body)));
    }
    Ok(source)
}

async fn default_privileges(conn: &impl ConnectionTrait) -> Result<Vec<String>> {
    let mut output = Vec::new();
    for row in query(conn, Backend::Postgres,
        "SELECT format('ALTER DEFAULT PRIVILEGES FOR ROLE %I%s',pg_catalog.pg_get_userbyid(d.defaclrole),
                   CASE WHEN d.defaclnamespace=0 THEN '' ELSE format(' IN SCHEMA %I',n.nspname) END) AS prefix,
             CASE d.defaclobjtype WHEN 'r' THEN 'TABLES' WHEN 'S' THEN 'SEQUENCES' WHEN 'f' THEN 'FUNCTIONS'
                 WHEN 'T' THEN 'TYPES' WHEN 'n' THEN 'SCHEMAS' END AS kind,
             format('defaclrole=%L::regrole AND defaclobjtype=%L AND defaclnamespace=%s',quote_ident(pg_catalog.pg_get_userbyid(d.defaclrole)),d.defaclobjtype,
                 CASE WHEN d.defaclnamespace=0 THEN '0' ELSE format('%L::regnamespace',quote_ident(n.nspname)) END) AS lookup,
             quote_ident(pg_catalog.pg_get_userbyid(d.defaclrole)) AS owner_name,
             (SELECT json_agg(json_build_object('role',CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(a.grantee)) END,
                 'privilege',a.privilege_type,'grantable',a.is_grantable,'grantor',quote_ident(pg_catalog.pg_get_userbyid(a.grantor))))
              FROM pg_catalog.aclexplode(d.defaclacl) a) AS grants
         FROM pg_catalog.pg_default_acl d LEFT JOIN pg_catalog.pg_namespace n ON n.oid=d.defaclnamespace
         WHERE d.defaclnamespace=0 OR (n.nspname <> 'information_schema' AND n.nspname !~ '^pg_')
         ORDER BY d.defaclrole,d.defaclnamespace,d.defaclobjtype", Vec::new()).await? {
        let prefix: String = get(&row, "prefix")?;
        let kind: String = get(&row, "kind")?;
        let lookup: String = get(&row, "lookup")?;
        let owner: String = get(&row, "owner_name")?;
        let revoke = postgres_dependencies::literal(&format!("{prefix} REVOKE ALL ON {kind} FROM "));
        let body = format!("DECLARE grantee_name text; BEGIN FOR grantee_name IN SELECT CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(a.grantee)) END FROM pg_catalog.pg_default_acl,LATERAL pg_catalog.aclexplode(defaclacl) a WHERE {lookup} LOOP EXECUTE {revoke} || grantee_name || ' CASCADE'; END LOOP; END");
        output.push(format!("DO {}", postgres_dependencies::literal(&body)));
        // When no destination entry exists, built-in grants still apply.
        output.push(format!("{prefix} REVOKE ALL ON {kind} FROM PUBLIC, {owner}"));
        let source: Vec<Grant> = serde_json::from_value(get::<Option<serde_json::Value>>(&row, "grants")?.unwrap_or_else(||serde_json::json!([])))
            .map_err(|e|Error::internal(format!("invalid default ACL: {e}")))?;
        for grant in source {
            let option = if grant.grantable { " WITH GRANT OPTION" } else { "" };
            output.push(format!("{prefix} GRANT {} ON {kind} TO {}{option}",grant.privilege,grant.role));
        }
    }
    Ok(output)
}
