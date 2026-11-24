//! Small list operations the benchmarks queries share.

use std::collections::{BTreeSet, HashMap};

/// Appends `item` unless the list already has it: a deduplicated list in first-seen order.
pub(super) fn push_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|x| x == item) {
        list.push(item.to_owned());
    }
}

/// Every key of either map, sorted, each once.
pub(super) fn sorted_union_keys<A, B>(
    a: &HashMap<String, A>,
    b: &HashMap<String, B>,
) -> Vec<String> {
    a.keys()
        .chain(b.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
