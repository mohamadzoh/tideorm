//! The drops of `force_sync`: every registered table that exists, each one
//! before the registered tables its foreign keys reference.

use crate::config::DatabaseType;
use crate::error::Result;
use crate::internal::OrmConnection;
use crate::schema::catalog;
use crate::tide_warn;

use super::schema::{ModelSchema, execute_ddl, qualifying_schema, table_reference};

/// Refuse `force_sync` where `environment` (`TIDEORM_ENV`) is `production`,
/// in any case as the CLI reads it, unless `allowed`
/// (`TIDEORM_ALLOW_FORCE_SYNC`): it deletes every registered table's rows.
pub(super) fn refuse_in_production(environment: Option<&str>, allowed: bool) -> Result<()> {
    let production = environment.is_some_and(|name| name.trim().eq_ignore_ascii_case("production"));
    if production && !allowed {
        return Err(crate::Error::configuration(
            "force_sync drops and recreates every registered table, and TIDEORM_ENV is \
             production; set TIDEORM_ALLOW_FORCE_SYNC=true to run it anyway",
        ));
    }
    Ok(())
}

/// Drop the tables of `models` that exist, `existing[i]` saying whether
/// `models[i]`'s does.
///
/// A table a foreign key references cannot be dropped first on MySQL and
/// MariaDB, nor on SQLite while the referencing table has rows. PostgreSQL's
/// `CASCADE` drops the keys other tables hold on a table (not those tables),
/// so a reference cycle or an unregistered referencing table is left to it;
/// elsewhere the database reports one.
pub(super) async fn drop_tables(
    conn: &OrmConnection,
    backend: DatabaseType,
    models: &[ModelSchema],
    existing: &[bool],
) -> Result<()> {
    let tables: Vec<&ModelSchema> = models
        .iter()
        .zip(existing)
        .filter_map(|(model, &exists)| exists.then_some(model))
        .collect();
    let mut references = Vec::with_capacity(tables.len());
    for (index, model) in tables.iter().enumerate() {
        let referenced = catalog::referenced_tables(
            conn,
            backend,
            qualifying_schema(model, backend),
            &model.table_name,
        )
        .await?;
        let targets: Vec<usize> = (0..tables.len())
            .filter(|&other| {
                other != index
                    && referenced
                        .iter()
                        .any(|name| names_table(name, tables[other], backend))
            })
            .collect();
        references.push(targets);
    }

    for index in drop_order(&references) {
        let model = tables[index];
        let cascade = if backend == DatabaseType::Postgres {
            " CASCADE"
        } else {
            ""
        };
        let sql = format!(
            "DROP TABLE IF EXISTS {}{cascade}",
            table_reference(model, backend)
        );
        execute_ddl(conn, backend, model, sql).await?;
        tide_warn!("Dropped TideORM table: {}", model.table_name);
    }
    Ok(())
}

/// Whether a referenced table, as the catalog writes it (`schema.name` on
/// PostgreSQL for a table outside the current schema), is `model`'s.
fn names_table(reference: &str, model: &ModelSchema, backend: DatabaseType) -> bool {
    match reference.rsplit_once('.') {
        Some((schema, name)) if backend == DatabaseType::Postgres => {
            schema == model.schema_name && name == model.table_name
        }
        _ if backend == DatabaseType::SQLite => reference.eq_ignore_ascii_case(&model.table_name),
        _ => reference == model.table_name,
    }
}

/// The order to drop tables in, given the tables each one references: a
/// table goes before every table it references. The members of a reference
/// cycle keep their order and go last.
fn drop_order(references: &[Vec<usize>]) -> Vec<usize> {
    let mut referenced_by = vec![0_usize; references.len()];
    for &target in references.iter().flatten() {
        referenced_by[target] += 1;
    }
    let mut dropped = vec![false; references.len()];
    let mut order = Vec::with_capacity(references.len());
    while let Some(next) =
        (0..references.len()).find(|&table| !dropped[table] && referenced_by[table] == 0)
    {
        dropped[next] = true;
        order.push(next);
        for &target in &references[next] {
            referenced_by[target] -= 1;
        }
    }
    order.extend((0..references.len()).filter(|&table| !dropped[table]));
    order
}

#[cfg(test)]
#[path = "../../tests/unit/sync_recreate_tests.rs"]
mod tests;
