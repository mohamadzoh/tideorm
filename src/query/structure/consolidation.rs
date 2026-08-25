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
/// - The **first** parent value seen for a key is the one kept; later copies are
///   dropped rather than merged, which is correct for a join that repeats an
///   identical parent row and lossy if it does not.
/// - The key function is called more than once per row, so keep it cheap and
///   free of side effects.
///
/// This is a namespace, not a value — every method is associated, and the unit
/// struct is never instantiated.
pub struct JoinResultConsolidator;

impl JoinResultConsolidator {
    /// Group `(parent, child)` pairs by the parent's key.
    ///
    /// Use this for an INNER JOIN, where every returned row has a child by
    /// construction. A parent with no children simply does not appear — if you
    /// need those, the join has to be a LEFT JOIN and the helper
    /// [`consolidate_two_optional`](Self::consolidate_two_optional).
    pub fn consolidate_two<A, B, K, F>(items: Vec<(A, B)>, key_fn: F) -> Vec<(A, Vec<B>)>
    where
        A: Clone,
        K: Eq + std::hash::Hash,
        F: Fn(&A) -> K,
    {
        use std::collections::HashMap;

        let mut groups: HashMap<K, (A, Vec<B>)> = HashMap::new();
        let mut order: Vec<K> = Vec::new();

        for (a, b) in items {
            let key = key_fn(&a);
            if let Some((_, bs)) = groups.get_mut(&key) {
                bs.push(b);
            } else {
                order.push(key_fn(&a));
                groups.insert(key, (a, vec![b]));
            }
        }

        order
            .into_iter()
            .filter_map(|key| groups.remove(&key))
            .collect()
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
        A: Clone,
        K: Eq + std::hash::Hash,
        F: Fn(&A) -> K,
    {
        use std::collections::HashMap;

        let mut groups: HashMap<K, (A, Vec<B>)> = HashMap::new();
        let mut order: Vec<K> = Vec::new();

        for (a, maybe_b) in items {
            let key = key_fn(&a);
            if let Some((_, bs)) = groups.get_mut(&key) {
                if let Some(b) = maybe_b {
                    bs.push(b);
                }
            } else {
                order.push(key_fn(&a));
                let values = maybe_b.into_iter().collect();
                groups.insert(key, (a, values));
            }
        }

        order
            .into_iter()
            .filter_map(|key| groups.remove(&key))
            .collect()
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
        A: Clone,
        B: Clone,
        KA: Eq + std::hash::Hash + Clone,
        KB: Eq + std::hash::Hash + Clone,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        use std::collections::HashMap;

        let mut a_groups: HashMap<KA, (A, HashMap<KB, (B, Vec<C>)>, Vec<KB>)> = HashMap::new();
        let mut a_order: Vec<KA> = Vec::new();

        for (a, b, c) in items {
            let key_a_value = key_a(&a);
            let key_b_value = key_b(&b);

            if let Some((_, b_groups, b_order)) = a_groups.get_mut(&key_a_value) {
                if let Some((_, values)) = b_groups.get_mut(&key_b_value) {
                    values.push(c);
                } else {
                    b_order.push(key_b_value.clone());
                    b_groups.insert(key_b_value, (b, vec![c]));
                }
            } else {
                a_order.push(key_a_value.clone());
                let mut b_groups = HashMap::new();
                let b_order = vec![key_b_value.clone()];
                b_groups.insert(key_b_value, (b, vec![c]));
                a_groups.insert(key_a_value, (a, b_groups, b_order));
            }
        }

        a_order
            .into_iter()
            .filter_map(|key| {
                a_groups.remove(&key).map(|(a, mut b_groups, b_order)| {
                    let values = b_order
                        .into_iter()
                        .filter_map(|inner_key| b_groups.remove(&inner_key))
                        .collect();
                    (a, values)
                })
            })
            .collect()
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
        A: Clone,
        B: Clone,
        KA: Eq + std::hash::Hash + Clone,
        KB: Eq + std::hash::Hash + Clone,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        use std::collections::HashMap;

        let mut a_groups: HashMap<KA, (A, HashMap<KB, (B, Vec<C>)>, Vec<KB>)> = HashMap::new();
        let mut a_order: Vec<KA> = Vec::new();

        for (a, b, maybe_c) in items {
            let key_a_value = key_a(&a);
            let key_b_value = key_b(&b);

            if let Some((_, b_groups, b_order)) = a_groups.get_mut(&key_a_value) {
                if let Some((_, values)) = b_groups.get_mut(&key_b_value) {
                    if let Some(c) = maybe_c {
                        values.push(c);
                    }
                } else {
                    b_order.push(key_b_value.clone());
                    let values = maybe_c.into_iter().collect();
                    b_groups.insert(key_b_value, (b, values));
                }
            } else {
                a_order.push(key_a_value.clone());
                let mut b_groups = HashMap::new();
                let b_order = vec![key_b_value.clone()];
                let values = maybe_c.into_iter().collect();
                b_groups.insert(key_b_value, (b, values));
                a_groups.insert(key_a_value, (a, b_groups, b_order));
            }
        }

        a_order
            .into_iter()
            .filter_map(|key| {
                a_groups.remove(&key).map(|(a, mut b_groups, b_order)| {
                    let values = b_order
                        .into_iter()
                        .filter_map(|inner_key| b_groups.remove(&inner_key))
                        .collect();
                    (a, values)
                })
            })
            .collect()
    }
}
