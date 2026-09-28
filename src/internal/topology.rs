//! Ordering nodes after the nodes they depend on.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Kahn's algorithm over `count` nodes and `(before, after)` edges: of the
/// nodes whose predecessors have all been taken, the one with the smallest
/// `key` goes next, which keeps the order deterministic. Returns the order and
/// the nodes left over, on or behind a cycle, in index order.
pub(crate) fn topological_order<K: Ord>(
    count: usize,
    edges: &[(usize, usize)],
    key: impl Fn(usize) -> K,
) -> (Vec<usize>, Vec<usize>) {
    let mut unmet = vec![0usize; count];
    let mut followers: Vec<Vec<usize>> = vec![Vec::new(); count];
    for &(before, after) in edges {
        followers[before].push(after);
        unmet[after] += 1;
    }

    let mut ready: BinaryHeap<Reverse<(K, usize)>> = (0..count)
        .filter(|&node| unmet[node] == 0)
        .map(|node| Reverse((key(node), node)))
        .collect();
    let mut ordered = Vec::with_capacity(count);
    while let Some(Reverse((_, node))) = ready.pop() {
        ordered.push(node);
        for &follower in &followers[node] {
            unmet[follower] -= 1;
            if unmet[follower] == 0 {
                ready.push(Reverse((key(follower), follower)));
            }
        }
    }

    let left_over = (0..count).filter(|&node| unmet[node] > 0).collect();
    (ordered, left_over)
}
