//! How an eager load matches the keys on one side of a relation with the
//! other: as values of the column they are compared with, integers by value
//! whatever their width, as the database's own comparison does. Where the
//! column's collation could match keys that differ, the database pairs them.

use std::collections::{HashMap, HashSet};

use super::{EagerLoadModel, key_value};
use crate::error::{Error, Result};
use crate::internal::{column_type_of, json_to_column_value};
use crate::model::Model;
use crate::orm::ColumnType;
use crate::orm::sql_builder::ValueKey;
use crate::query::QueryBuilder;

/// The keys a column of a model holds. A key is read as a value of the
/// column's type, so an `i64` key finds the `i32` or the text column holding
/// the same value, and compared as a [`ValueKey`], so integers of different
/// widths are one key.
pub(crate) struct KeyColumn(Option<ColumnType>);

impl KeyColumn {
    /// The keys `M`'s `column` holds.
    pub(crate) fn of<M: Model>(column: &str) -> Self {
        Self(column_type_of::<M>(column))
    }

    /// `value` as a key of the column.
    pub(crate) fn key(&self, value: &serde_json::Value) -> ValueKey {
        ValueKey::new(json_to_column_value(value, self.0.as_ref()))
    }
}

/// Every row of `M` whose `foreign_key` is one of `keys`, grouped by that key
/// as [`KeyColumn`] reads it.
///
/// One `WHERE .. IN (..)` for every parent, so eager loading stays one query
/// instead of N+1. `morph_type` adds the polymorphic type discriminator as
/// `(column, value)`.
async fn grouped_by_key<M: EagerLoadModel>(
    keys: &[serde_json::Value],
    foreign_key: &str,
    morph_type: Option<(&'static str, &'static str)>,
) -> Result<HashMap<ValueKey, Vec<M>>> {
    let key_column = KeyColumn::of::<M>(foreign_key);
    let mut grouped: HashMap<ValueKey, Vec<M>> = HashMap::new();

    // Each key once: a scan of the list per key would be quadratic in the
    // number of parents.
    let mut seen = HashSet::new();
    let lookup_keys: Vec<serde_json::Value> = keys
        .iter()
        .filter(|key| !key.is_null() && seen.insert(key_column.key(key)))
        .cloned()
        .collect();

    // A chunk per query keeps each IN list under the backends'
    // bind-parameter limits however many parents there are.
    for chunk in lookup_keys.chunks(5_000) {
        let mut query = M::query()
            .with_rule_scope(crate::rules::RuleScope::Internal)
            .where_in(foreign_key, chunk.to_vec());
        if let Some((type_column, type_value)) = morph_type {
            query = query.where_eq(type_column, type_value);
        }
        let rows = query.get().await?;

        // The database matched each row under the column's collation, which
        // can ignore case (SQLite's NOCASE, MySQL's default ones), accents or
        // trailing spaces. When a row's key is not one of the requested keys,
        // or two requested keys could match one stored key, the database
        // pairs them instead.
        let requested: HashSet<ValueKey> = chunk.iter().map(|key| key_column.key(key)).collect();
        let mut keyed = Vec::with_capacity(rows.len());
        for row in rows {
            keyed.push((key_column.key(&key_value(&row, foreign_key)?), row));
        }
        if keyed.iter().all(|(key, _)| requested.contains(key)) && !fold_together(chunk) {
            for (key, row) in keyed {
                grouped.entry(key).or_default().push(row);
            }
            continue;
        }

        let pairs = matching_keys::<M>(&key_column, chunk, foreign_key, morph_type).await?;
        for (key, row) in keyed {
            match pairs.get(&key) {
                // The row is cloned for every key but the last, which takes it.
                Some(matched) => {
                    if let Some((last, others)) = matched.split_last() {
                        for matched in others {
                            grouped
                                .entry(matched.clone())
                                .or_default()
                                .push(row.clone());
                        }
                        grouped.entry(last.clone()).or_default().push(row);
                    }
                }
                // A key the pairing read back in another form still keeps
                // the parent it names exactly.
                None if requested.contains(&key) => {
                    grouped.entry(key).or_default().push(row);
                }
                None => {}
            }
        }
    }

    Ok(grouped)
}

/// Whether the database could match two of `keys` to the same stored key.
///
/// A case-insensitive collation matches text that differs only by ASCII
/// case or trailing spaces, which is checked here. Beyond plain ASCII what
/// matches is the collation's to say (`cafe` and `café` under an
/// accent-insensitive one, `ss` and `ß` under others), so text keys of which
/// any is not plain ASCII are left to the database.
pub(crate) fn fold_together(keys: &[serde_json::Value]) -> bool {
    let texts: Vec<&str> = keys.iter().filter_map(serde_json::Value::as_str).collect();
    if texts.len() < 2 {
        return false;
    }
    if texts
        .iter()
        .any(|key| !key.bytes().all(|byte| (b' '..=b'~').contains(&byte)))
    {
        return true;
    }
    let mut folded = HashSet::new();
    texts
        .iter()
        .any(|key| !folded.insert(key.trim_end_matches(' ').to_ascii_lowercase()))
}

/// How many keys one pairing query compares, one `UNION ALL` member each:
/// SQLite takes at most 500 members in a compound select.
const PAIRED_KEYS_PER_QUERY: usize = 200;

/// Which of `keys` each stored foreign key of `M` matches, as the database
/// compares them: `foreign_key = ?` for each key, under the column's own
/// collation, mapped from the stored key to the keys it matches.
async fn matching_keys<M: Model>(
    key_column: &KeyColumn,
    keys: &[serde_json::Value],
    foreign_key: &str,
    morph_type: Option<(&'static str, &'static str)>,
) -> Result<HashMap<ValueKey, Vec<ValueKey>>> {
    let column = M::column_named(foreign_key);
    let db_type = M::query().db_type_for_sql()?;
    let lookup = |index: usize, key: &serde_json::Value| {
        // DISTINCT must preserve different byte spellings under case/accent-insensitive collations.
        let exact = db_type
            .dialect()
            .exact_key(&crate::query::db_sql::format_column(db_type, column));
        let mut query = M::query()
            .with_rule_scope(crate::rules::RuleScope::Internal)
            .distinct()
            .select(vec![column])
            .select_raw(&format!("{exact} AS tideorm_exact_key"))
            .select_raw(&format!("{index} AS tideorm_key_index"))
            .where_eq(foreign_key, key.clone());
        if let Some((type_column, type_value)) = morph_type {
            query = query.where_eq(type_column, type_value);
        }
        query
    };

    let mut pairs: HashMap<ValueKey, Vec<ValueKey>> = HashMap::new();
    let mut seen = HashSet::new();
    for (batch, chunk) in keys.chunks(PAIRED_KEYS_PER_QUERY).enumerate() {
        let offset = batch * PAIRED_KEYS_PER_QUERY;
        let mut lookups = chunk
            .iter()
            .enumerate()
            .map(|(index, key)| lookup(offset + index, key));
        let Some(first) = lookups.next() else {
            continue;
        };
        let query = lookups.fold(first, QueryBuilder::union_all);

        for row in query.get_json().await? {
            let index = row["tideorm_key_index"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| keys.get(index))
                .ok_or_else(|| {
                    Error::query(format!(
                        "an eager load of {} paired a key it did not look up",
                        M::table_name()
                    ))
                })?;
            let stored = key_column.key(&row[column]);
            let key = key_column.key(index);
            if seen.insert((stored.clone(), key.clone())) {
                pairs.entry(stored).or_default().push(key);
            }
        }
    }
    Ok(pairs)
}

/// A copy of `models` with each primary key once, in order; a repeat is
/// skipped before it is cloned.
fn distinct_by_primary_key<M: Model>(models: Vec<&M>) -> Vec<M> {
    let mut seen = HashSet::new();
    models
        .into_iter()
        .filter(|model| {
            seen.insert(serde_json::to_string(&model.primary_key()).unwrap_or_default())
        })
        .cloned()
        .collect()
}

/// The rows of `R` whose `related_key` holds each of `parent_keys`, matched by
/// value, so an `i64` key finds an `i32` or a text foreign key holding it; a
/// `MorphOne`/`MorphMany` adds its type discriminator. One group per parent
/// key, in order, for a to-many relation.
#[doc(hidden)]
pub async fn __eager_keyed_many<R: EagerLoadModel>(
    parent_keys: &[serde_json::Value],
    related_key: &str,
    morph_type: Option<(&'static str, &'static str)>,
) -> Result<Vec<Vec<R>>> {
    let by_key = grouped_by_key::<R>(parent_keys, related_key, morph_type).await?;
    let keys = KeyColumn::of::<R>(related_key);
    Ok(per_parent_key(&keys, parent_keys, by_key)
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect())
}

/// [`__eager_keyed_many`] for a to-one relation: the first row per parent key.
#[doc(hidden)]
pub async fn __eager_keyed_one<R: EagerLoadModel>(
    parent_keys: &[serde_json::Value],
    related_key: &str,
    morph_type: Option<(&'static str, &'static str)>,
) -> Result<Vec<Option<R>>> {
    let by_key = grouped_by_key::<R>(parent_keys, related_key, morph_type).await?;
    let first_by_key = by_key
        .into_iter()
        .filter_map(|(key, group)| Some((key, group.into_iter().next()?)))
        .collect();
    let keys = KeyColumn::of::<R>(related_key);
    Ok(per_parent_key(&keys, parent_keys, first_by_key))
}

/// `by_key`'s value for each of `parent_keys`, read as keys of `key_column`,
/// in order. A value moves out at its key's last occurrence and is cloned for
/// the ones before, so parents with distinct keys copy no rows.
fn per_parent_key<V: Clone>(
    key_column: &KeyColumn,
    parent_keys: &[serde_json::Value],
    mut by_key: HashMap<ValueKey, V>,
) -> Vec<Option<V>> {
    let keys: Vec<ValueKey> = parent_keys.iter().map(|key| key_column.key(key)).collect();
    // A repeated key's later positions overwrite its earlier ones.
    let last: HashMap<&ValueKey, usize> = keys
        .iter()
        .enumerate()
        .map(|(position, key)| (key, position))
        .collect();
    keys.iter()
        .enumerate()
        .map(|(position, key)| {
            if last[key] == position {
                by_key.remove(key)
            } else {
                by_key.get(key).cloned()
            }
        })
        .collect()
}

/// The related rows of a `HasManyThrough` per parent key: the live pivot rows
/// of every parent, then the live related rows they link, each step matched by
/// value as the other relations are, so a pivot key of another integer type
/// than the owner's or the related model's key still links them. Each step
/// reads through its model's query, whose soft-delete scope leaves out trashed
/// pivot and related rows. A pair the pivot holds twice is one related row.
#[doc(hidden)]
pub async fn __eager_through<R: EagerLoadModel, P: EagerLoadModel>(
    parent_keys: &[serde_json::Value],
    foreign_key: &str,
    related_key: &str,
    related_local_key: &str,
) -> Result<Vec<Vec<R>>> {
    let links = grouped_by_key::<P>(parent_keys, foreign_key, None).await?;
    let mut related_keys = Vec::new();
    for link in links.values().flatten() {
        related_keys.push(key_value(link, related_key)?);
    }
    let related_by_key = grouped_by_key::<R>(&related_keys, related_local_key, None).await?;

    let link_keys = KeyColumn::of::<P>(foreign_key);
    let related_keys = KeyColumn::of::<R>(related_local_key);
    let mut related = Vec::with_capacity(parent_keys.len());
    for key in parent_keys {
        let mut rows = Vec::new();
        for link in links.get(&link_keys.key(key)).into_iter().flatten() {
            let link_key = key_value(link, related_key)?;
            if let Some(found) = related_by_key.get(&related_keys.key(&link_key)) {
                rows.extend(found);
            }
        }
        related.push(distinct_by_primary_key(rows));
    }
    Ok(related)
}
