//! Bounded explicit relationships, never inferred from prose or common forms.
use super::{Candidate, LocatedNode, ResolvedContent, same};
use mant_protocol::{
    EvidenceBasis, MAX_EXPLANATION_CANDIDATES, MAX_EXPLANATION_RELATION_DEPTH,
    MAX_EXPLANATION_RELATIONS,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn expand<'a>(
    content: &ResolvedContent,
    query: &str,
    located: &[LocatedNode<'a>],
    orders: &[usize],
    candidates: &mut Vec<Candidate<'a>>,
) -> bool {
    let Some(document) = &content.document else {
        return false;
    };
    let invalid = mant_ir::entry_relation_issues(document)
        .into_iter()
        .map(|issue| issue.owner)
        .collect::<BTreeSet<_>>();
    let duplicates = mant_ir::DocumentIndex::build(document)
        .duplicates()
        .iter()
        .map(|d| d.id.clone())
        .collect::<BTreeSet<_>>();
    let indices = located
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.is_section() && !duplicates.contains(n.id()))
        .map(|(i, n)| (n.id(), i))
        .collect::<BTreeMap<_, _>>();
    let (edges, mut truncated) = graph(located, &indices, &invalid, &duplicates);
    let mut records = candidates
        .iter()
        .enumerate()
        .filter_map(|(record, c)| c.located.map(|index| (index, record)))
        .collect::<BTreeMap<_, _>>();
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    for candidate in candidates.iter_mut() {
        let Some(index) = candidate.located else {
            continue;
        };
        if !candidate.bases.iter().any(|b| {
            matches!(
                b,
                EvidenceBasis::Name | EvidenceBasis::Form | EvidenceBasis::Identity
            )
        }) {
            continue;
        }
        let facts = located[index].identity().expect("entry candidate");
        if invalid.contains(&facts.id) || duplicates.contains(&facts.id) {
            continue;
        }
        for group in &facts.alias_groups {
            if group.iter().any(|name| same(name, query, facts.case)) {
                if group.iter().map(String::len).sum::<usize>() > 8192 {
                    truncated = true;
                    continue;
                }
                candidate.bases.push(EvidenceBasis::AliasGroup {
                    members: group.clone(),
                });
            }
        }
        visited.insert(index);
        queue.push_back((index, index, Vec::new()));
    }
    let mut followed = 0;
    while let Some((current, origin, path)) = queue.pop_front() {
        for &(target, declaration) in edges.get(&current).into_iter().flatten() {
            if visited.contains(&target) {
                continue;
            }
            if followed == MAX_EXPLANATION_RELATIONS
                || path.len() == MAX_EXPLANATION_RELATION_DEPTH
                || candidates.len() == MAX_EXPLANATION_CANDIDATES
            {
                truncated = true;
                continue;
            }
            followed += 1;
            visited.insert(target);
            let mut path = path.clone();
            path.push(located[declaration].id().into());
            let basis = EvidenceBasis::Related {
                from: located[origin].id().into(),
                declarations: path.clone(),
            };
            if let Some(&record) = records.get(&target) {
                candidates[record].bases.push(basis);
            } else {
                records.insert(target, candidates.len());
                candidates.push(Candidate {
                    order: orders[target],
                    located: Some(target),
                    ordinary: None,
                    section: None,
                    block_path: None,
                    source: located[target].source(),
                    bases: vec![basis],
                });
            }
            queue.push_back((target, origin, path));
        }
    }
    truncated
}

type Edges = BTreeMap<usize, Vec<(usize, usize)>>;
fn graph(
    located: &[LocatedNode<'_>],
    indices: &BTreeMap<&str, usize>,
    invalid: &BTreeSet<mant_ir::NodeId>,
    duplicates: &BTreeSet<mant_ir::NodeId>,
) -> (Edges, bool) {
    let mut edges = Edges::new();
    let mut count = 0;
    for (index, node) in located.iter().enumerate() {
        let Some(facts) = node.identity() else {
            continue;
        };
        if invalid.contains(&facts.id) || duplicates.contains(&facts.id) {
            continue;
        }
        if let Some(target) = facts
            .alias_of
            .as_ref()
            .and_then(|id| indices.get(id.as_str()))
        {
            if count == MAX_EXPLANATION_RELATIONS {
                return (edges, true);
            }
            count += 1;
            edges.entry(index).or_default().push((*target, index));
            edges.entry(*target).or_default().push((index, index));
        }
    }
    (edges, false)
}
