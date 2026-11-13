//! The rules that read several models at once: how deep owned relations nest,
//! whether they cycle, and whether every foreign key is indexed.

use super::{ModelOutline, short_name};

/// Every cross-model rule `models` break, one line each.
pub(super) fn broken_rules(models: &[ModelOutline]) -> Vec<String> {
    let owned: Vec<Vec<usize>> = models
        .iter()
        .map(|model| owned_models(models, model))
        .collect();
    let mut depths = vec![Depth::Unknown; models.len()];
    let mut broken = Vec::new();
    for (index, model) in models.iter().enumerate() {
        let name = short_name(model.name);
        if model.rules.forbid_relation_cycles
            && let Some(cycle) = cycle_through(&owned, index)
        {
            broken.push(format!(
                "{name}'s relations lead back to it: {} (forbid_relation_cycles)",
                chain(models, &cycle)
            ));
        }
        if let Some(max) = model.rules.max_relation_depth {
            match depth(&owned, &mut depths, index) {
                Some((deepest, _)) if deepest <= max as usize => {}
                Some((deepest, _)) => broken.push(format!(
                    "{name}'s relations nest {deepest} deep ({}), over the {max} max_relation_depth allows",
                    chain(models, &deepest_chain(&depths, index))
                )),
                None => broken.push(format!(
                    "{name}'s relations reach a cycle, so they nest without end (max_relation_depth)"
                )),
            }
        }
    }
    broken.extend(unindexed_foreign_keys(models));
    broken
}

/// The models whose rows `model` owns, by index: every model on an owned
/// relation's table. A relation to the model's own table (a tree's children)
/// nests data, not the schema, so it is left out.
fn owned_models(models: &[ModelOutline], model: &ModelOutline) -> Vec<usize> {
    let mut owned: Vec<usize> = model
        .relations
        .iter()
        .filter(|relation| relation.owns)
        .filter_map(|relation| relation.related.as_deref())
        .filter(|related| *related != model.table)
        .flat_map(|related| on_table(models, related))
        .collect();
    owned.sort_unstable();
    owned.dedup();
    owned
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Depth {
    Unknown,
    Visiting,
    /// The longest owned chain below a model and the next model on it; `None`
    /// when a cycle is reachable.
    Known(Option<(usize, Option<usize>)>),
}

/// The longest chain of owned relations from `index`, and the next model on
/// it; `None` when a cycle is reachable, which makes it endless.
fn depth(
    owned: &[Vec<usize>],
    depths: &mut [Depth],
    index: usize,
) -> Option<(usize, Option<usize>)> {
    match depths[index] {
        Depth::Known(known) => return known,
        Depth::Visiting => return None,
        Depth::Unknown => {}
    }
    depths[index] = Depth::Visiting;
    let mut deepest = Some((0, None));
    for &next in &owned[index] {
        deepest = match (deepest, depth(owned, depths, next)) {
            (Some((best, _)), Some((below, _))) if below + 1 > best => {
                Some((below + 1, Some(next)))
            }
            (Some(best), Some(_)) => Some(best),
            _ => None,
        };
    }
    depths[index] = Depth::Known(deepest);
    deepest
}

fn deepest_chain(depths: &[Depth], start: usize) -> Vec<usize> {
    let mut chain = vec![start];
    let mut at = start;
    while let Depth::Known(Some((_, Some(next)))) = depths[at] {
        chain.push(next);
        at = next;
    }
    chain
}

/// A chain of owned relations from `start` back to it, if there is one.
fn cycle_through(owned: &[Vec<usize>], start: usize) -> Option<Vec<usize>> {
    let mut path = vec![start];
    let mut seen = vec![false; owned.len()];
    fn walk(owned: &[Vec<usize>], start: usize, path: &mut Vec<usize>, seen: &mut [bool]) -> bool {
        let at = *path.last().expect("the path holds its start");
        for &next in &owned[at] {
            if next == start {
                path.push(next);
                return true;
            }
            if !seen[next] {
                seen[next] = true;
                path.push(next);
                if walk(owned, start, path, seen) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
    walk(owned, start, &mut path, &mut seen).then_some(path)
}

fn chain(models: &[ModelOutline], chain: &[usize]) -> String {
    chain
        .iter()
        .map(|index| short_name(models[*index].name))
        .collect::<Vec<_>>()
        .join(" → ")
}

/// The models on `table`, by index; several structs may read one table.
fn on_table<'a>(models: &'a [ModelOutline], table: &'a str) -> impl Iterator<Item = usize> + 'a {
    (0..models.len()).filter(move |index| models[*index].table == table)
}

/// Each foreign key column that leads no index of its table, for the tables
/// a model held to `require_foreign_key_index` reads. An index any model on
/// the table declares counts; a key on a table no compiled model declares is
/// left alone.
fn unindexed_foreign_keys(models: &[ModelOutline]) -> Vec<String> {
    let mut keys: Vec<(&str, &str)> = models
        .iter()
        .flat_map(|model| &model.relations)
        .flat_map(|relation| &relation.foreign_keys)
        .map(|(table, column)| (table.as_str(), *column))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter_map(|(table, column)| {
            let model = on_table(models, table)
                .map(|index| &models[index])
                .find(|model| model.rules.require_foreign_key_index)?;
            let indexed = on_table(models, table)
                .map(|index| &models[index])
                .any(|model| {
                    model.primary_keys.first().is_some_and(|key| key == column)
                        || model
                            .indexes
                            .iter()
                            .any(|index| index.columns.first().is_some_and(|first| first == column))
                });
            (!indexed).then(|| {
                format!(
                    "{}'s foreign key `{column}` leads no index (require_foreign_key_index)",
                    short_name(model.name)
                )
            })
        })
        .collect()
}
