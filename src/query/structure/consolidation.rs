pub struct JoinResultConsolidator;

impl JoinResultConsolidator {
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
