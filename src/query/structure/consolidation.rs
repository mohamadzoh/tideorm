use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;

/// Regroups the flat rows a JOIN returns into the nested shape they describe.
///
/// A join between one parent and N children returns the parent repeated once per
/// child. These helpers collapse that repetition — `Vec<(Order, LineItem)>`
/// becomes `Vec<(Order, Vec<LineItem>)>` — which is the shape application code
/// almost always wants after `find_also_related()`.
///
/// Three properties are worth relying on, and one is worth avoiding:
///
/// - Rows for the same parent do **not** have to be adjacent; grouping is by key
///   equality, not by run.
/// - Parents come out in order of first appearance, and each parent's children
///   keep their relative input order, so an `ORDER BY` on either side survives.
/// - Each key function is called once per row.
/// - The **first** parent value seen for a key is the one kept; later copies are
///   dropped rather than merged, which is correct for a join that repeats an
///   identical parent row and lossy if it does not.
///
/// This is a namespace, not a value — every method is associated, and the unit
/// struct is never instantiated.
pub struct JoinResultConsolidator;

/// The slot of `key` in `groups`, pushing `make()` as a new group for a key not
/// seen before.
fn group_slot<K: Eq + Hash, G>(
    index: &mut HashMap<K, usize>,
    groups: &mut Vec<G>,
    key: K,
    make: impl FnOnce() -> G,
) -> usize {
    match index.entry(key) {
        Entry::Occupied(entry) => *entry.get(),
        Entry::Vacant(entry) => {
            groups.push(make());
            *entry.insert(groups.len() - 1)
        }
    }
}

impl JoinResultConsolidator {
    /// Group `(parent, child)` pairs by the parent's key.
    ///
    /// Use this for an INNER JOIN, where every returned row has a child by
    /// construction. A parent with no children simply does not appear — if you
    /// need those, the join has to be a LEFT JOIN and the helper
    /// [`consolidate_two_optional`](Self::consolidate_two_optional).
    pub fn consolidate_two<A, B, K, F>(items: Vec<(A, B)>, key_fn: F) -> Vec<(A, Vec<B>)>
    where
        K: Eq + Hash,
        F: Fn(&A) -> K,
    {
        Self::consolidate_two_optional(
            items.into_iter().map(|(a, b)| (a, Some(b))).collect(),
            key_fn,
        )
    }

    /// Group LEFT JOIN rows, where an unmatched parent arrives with `None`.
    ///
    /// The `None`s are dropped and the parent is kept with an empty child
    /// vector, so "no children" and "children" are both representable — the one
    /// thing [`consolidate_two`](Self::consolidate_two) cannot express.
    pub fn consolidate_two_optional<A, B, K, F>(
        items: Vec<(A, Option<B>)>,
        key_fn: F,
    ) -> Vec<(A, Vec<B>)>
    where
        K: Eq + Hash,
        F: Fn(&A) -> K,
    {
        let mut index = HashMap::new();
        let mut groups: Vec<(A, Vec<B>)> = Vec::new();

        for (a, maybe_b) in items {
            let slot = group_slot(&mut index, &mut groups, key_fn(&a), || (a, Vec::new()));
            groups[slot].1.extend(maybe_b);
        }

        groups
    }

    /// Nest a three-way join two levels deep: `(A, B, C)` rows become
    /// `(A, Vec<(B, Vec<C>)>)`.
    ///
    /// `key_a` identifies the outer parent and `key_b` the middle row. `key_b`
    /// is only compared within one `A` group, so a middle key that is only
    /// unique per parent — a row number, say — still groups correctly.
    ///
    /// Every row must carry all three levels, so a `B` with no `C` cannot be
    /// represented; use
    /// [`consolidate_three_optional`](Self::consolidate_three_optional) when the
    /// innermost join is a LEFT JOIN.
    #[allow(clippy::type_complexity)]
    pub fn consolidate_three<A, B, C, KA, KB, FA, FB>(
        items: Vec<(A, B, C)>,
        key_a: FA,
        key_b: FB,
    ) -> Vec<(A, Vec<(B, Vec<C>)>)>
    where
        KA: Eq + Hash,
        KB: Eq + Hash,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        Self::consolidate_three_optional(
            items.into_iter().map(|(a, b, c)| (a, b, Some(c))).collect(),
            key_a,
            key_b,
        )
    }

    /// [`consolidate_three`](Self::consolidate_three) for an innermost LEFT
    /// JOIN: a missing `C` is dropped and its `B` survives with an empty vector.
    ///
    /// Only the innermost level may be absent. `A` and `B` are still required on
    /// every row, so a parent with no middle row at all is not representable.
    #[allow(clippy::type_complexity)]
    pub fn consolidate_three_optional<A, B, C, KA, KB, FA, FB>(
        items: Vec<(A, B, Option<C>)>,
        key_a: FA,
        key_b: FB,
    ) -> Vec<(A, Vec<(B, Vec<C>)>)>
    where
        KA: Eq + Hash,
        KB: Eq + Hash,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        let mut a_index = HashMap::new();
        let mut groups: Vec<ParentGroup<A, B, C, KB>> = Vec::new();

        for (a, b, maybe_c) in items {
            let a_slot = group_slot(&mut a_index, &mut groups, key_a(&a), || ParentGroup {
                parent: a,
                children: Vec::new(),
                child_index: HashMap::new(),
            });

            let group = &mut groups[a_slot];
            let b_slot = group_slot(
                &mut group.child_index,
                &mut group.children,
                key_b(&b),
                || (b, Vec::new()),
            );
            group.children[b_slot].1.extend(maybe_c);
        }

        groups
            .into_iter()
            .map(|group| (group.parent, group.children))
            .collect()
    }
}

/// One outer parent of a three-way consolidation, with its middle rows in
/// order of first appearance and the index that finds them by key.
struct ParentGroup<A, B, C, KB> {
    parent: A,
    children: Vec<(B, Vec<C>)>,
    child_index: HashMap<KB, usize>,
}
