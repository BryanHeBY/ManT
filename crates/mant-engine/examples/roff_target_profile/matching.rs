//! Matches native owner obligations to compatible IR occurrences.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    ExpectedTarget, MatchedTarget, ObservedRole, ObservedTarget, ObservedTargets, TargetRole,
};

pub(super) fn match_targets(
    expected: &[ExpectedTarget],
    observed: &[ObservedTarget],
) -> (Vec<ExpectedTarget>, Vec<MatchedTarget>, BTreeSet<String>) {
    let mut alias_index = BTreeMap::<(usize, String), Vec<(usize, usize)>>::new();
    let mut anchors_by_section = BTreeMap::<usize, Vec<usize>>::new();
    for (index, candidate) in observed.iter().enumerate() {
        for (fragment_index, fragment) in candidate.fragment_aliases.iter().enumerate() {
            alias_index
                .entry((candidate.section_ordinal, fragment.clone()))
                .or_default()
                .push((index, fragment_index));
        }
        if candidate.role == ObservedRole::Anchor {
            anchors_by_section
                .entry(candidate.section_ordinal)
                .or_default()
                .push(index);
        }
    }

    let mut used = BTreeSet::<String>::new();
    let mut assignments = vec![None; expected.len()];

    for (expected_index, target) in expected
        .iter()
        .enumerate()
        .filter(|(_, target)| target.explicit)
    {
        let candidates = alias_index
            .get(&(target.section_ordinal, target.id.clone()))
            .map_or(&[][..], Vec::as_slice);
        if let Some((index, fragment_index)) =
            candidates.iter().copied().find(|(index, fragment_index)| {
                let candidate = &observed[*index];
                role_matches(target.expected_role, candidate.role)
                    && container_matches(target.expected_container, candidate.container)
                    && owner_matches(target, candidate)
                    && !used.contains(&format!("{index}:identity"))
                    && !used.contains(&format!("{index}:fragment:{fragment_index}"))
            })
        {
            used.insert(format!("{index}:fragment:{fragment_index}"));
            used.insert(format!("{index}:identity"));
            assignments[expected_index] =
                Some(matched_target(target, &observed[index], "fragment-alias"));
        } else if target.id == target.normalized_id {
            let candidate = observed.iter().enumerate().find(|(index, candidate)| {
                candidate.section_ordinal == target.section_ordinal
                    && candidate.identity == target.id
                    && role_matches(target.expected_role, candidate.role)
                    && container_matches(target.expected_container, candidate.container)
                    && owner_matches(target, candidate)
                    && !used.contains(&format!("{index}:identity"))
            });
            if let Some((index, candidate)) = candidate {
                used.insert(format!("{index}:identity"));
                assignments[expected_index] =
                    Some(matched_target(target, candidate, "canonical-identity"));
            }
        }
    }

    let mut section_cursors = BTreeMap::<usize, usize>::new();
    for (expected_index, target) in expected.iter().enumerate() {
        if target.explicit || assignments[expected_index].is_some() {
            continue;
        }
        let candidates = anchors_by_section
            .get(&target.section_ordinal)
            .map_or(&[][..], Vec::as_slice);
        let cursor = section_cursors.entry(target.section_ordinal).or_default();
        let candidate = candidates[*cursor..]
            .iter()
            .enumerate()
            .find_map(|(offset, index)| {
                let index = *index;
                let candidate = &observed[index];
                let identity_claim = format!("{index}:identity");
                let fragment_prefix = format!("{index}:fragment:");
                (container_matches(target.expected_container, candidate.container)
                    && owner_matches(target, candidate)
                    && !used.contains(&identity_claim)
                    && !used.iter().any(|claim| claim.starts_with(&fragment_prefix))
                    && generated_identity_matches(&target.normalized_id, &candidate.identity))
                .then_some((offset, index))
            });
        if let Some((offset, index)) = candidate {
            *cursor += offset + 1;
            used.insert(format!("{index}:identity"));
            assignments[expected_index] = Some(matched_target(
                target,
                &observed[index],
                "ordered-identity-occurrence",
            ));
        }
    }

    let mut missing = Vec::new();
    let mut confirmed = Vec::new();
    for (target, candidate) in expected.iter().zip(assignments) {
        if let Some(candidate) = candidate {
            confirmed.push(candidate);
        } else {
            missing.push(target.clone());
        }
    }
    (missing, confirmed, used)
}

fn owner_matches(expected: &ExpectedTarget, observed: &ObservedTarget) -> bool {
    expected.owner_source_line == observed.owner_source_line
}

fn matched_target(
    expected: &ExpectedTarget,
    observed: &ObservedTarget,
    matched_by: &'static str,
) -> MatchedTarget {
    MatchedTarget {
        logical_owner_key: expected.logical_owner_key.clone(),
        observed_ir_path: observed.ir_path.clone(),
        observed_identity: observed.identity.clone(),
        matched_by,
    }
}

fn container_matches(expected: &str, observed: &str) -> bool {
    match expected {
        "section" => observed == "section",
        "item" => matches!(observed, "definition" | "list-item" | "table-cell"),
        "content" => observed != "section" && observed != "document",
        _ => false,
    }
}

const fn role_matches(expected: TargetRole, observed: ObservedRole) -> bool {
    matches!(
        (expected, observed),
        (TargetRole::Section, ObservedRole::Section) | (TargetRole::Anchor, ObservedRole::Anchor)
    )
}

pub(super) fn unexpected_targets(
    observed: &ObservedTargets,
    used: &BTreeSet<String>,
) -> Vec<String> {
    let mut unexpected = Vec::new();
    for (index, target) in observed.occurrences.iter().enumerate() {
        for (fragment_index, fragment) in target.fragment_aliases.iter().enumerate() {
            let claim = format!("{index}:fragment:{fragment_index}");
            if !used.contains(&claim) {
                unexpected.push(format!(
                    "fragment alias {fragment:?} on {} {}",
                    target.container, target.ir_path
                ));
            }
        }
        if target.role == ObservedRole::Anchor
            && !observed.entries.contains(&target.identity)
            && !used.contains(&format!("{index}:identity"))
        {
            unexpected.push(format!(
                "anchor {:?} in {} {}",
                target.identity, target.container, target.ir_path
            ));
        }
    }
    unexpected
}

pub(super) fn generated_identity_matches(base: &str, candidate: &str) -> bool {
    candidate == base || collision_base(candidate) == Some(base)
}

fn collision_base(candidate: &str) -> Option<&str> {
    let (base, suffix) = candidate.rsplit_once('-')?;
    let value = suffix.parse::<usize>().ok()?;
    (value >= 2 && suffix == value.to_string()).then_some(base)
}
