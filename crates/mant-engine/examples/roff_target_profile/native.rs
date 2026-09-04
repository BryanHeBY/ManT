//! Classifies libmandoc AST target owners into audit obligations.

use std::collections::BTreeMap;

use libmandoc_rs::{Node, NodeKind};

use super::{
    ClassifiedOwner, ExpectedTarget, OwnerClass, OwnerDisposition, TargetRole, UnclassifiedOwner,
    document_id_slug,
};

pub(super) struct NativeTargetProfile {
    pub(super) expected: Vec<ExpectedTarget>,
    pub(super) owner_count: usize,
    pub(super) classified_owner_count: usize,
    pub(super) logical_owner_count: usize,
    pub(super) classified_logical_owner_count: usize,
    pub(super) owner_classes: Vec<OwnerClass>,
    pub(super) unclassified: Vec<UnclassifiedOwner>,
}

struct AstNodeRef<'a> {
    node: &'a Node,
    path: String,
    pub(super) section_heading: bool,
    pub(super) section_ordinal: usize,
    pub(super) section_source_line: u32,
    pub(super) order: usize,
}

#[derive(Clone)]
pub(super) struct ExplicitTarget {
    pub(super) id: String,
    pub(super) argumentless: bool,
    pub(super) source_line: u32,
    pub(super) ast_path: String,
    pub(super) section_ordinal: usize,
    pub(super) section_source_line: u32,
    pub(super) order: usize,
}

pub(super) struct LogicalOwner {
    pub(super) target: Option<String>,
    pub(super) owner_source_line: u32,
    pub(super) owner_macro: String,
    pub(super) owner_kind: String,
    pub(super) ast_path: String,
    pub(super) section_heading: bool,
    pub(super) section_ordinal: usize,
    pub(super) section_source_line: u32,
    pub(super) order: usize,
    pub(super) raw_owner_count: usize,
    pub(super) explicit: Option<ExplicitTarget>,
}

pub(super) fn native_target_profile(root: &Node) -> NativeTargetProfile {
    let mut flattened = Vec::new();
    let mut next_section_ordinal = 0;
    flatten_nodes(
        root,
        "0",
        0,
        0,
        false,
        &mut next_section_ordinal,
        &mut flattened,
    );
    let explicit = explicit_targets(&flattened);
    let (mut owners, owner_count) = logical_owners(&flattened);
    let unmatched_explicit = bind_explicit_targets(&mut owners, &explicit);
    assemble_native_profile(owners, unmatched_explicit, owner_count)
}

fn logical_owners(flattened: &[AstNodeRef<'_>]) -> (Vec<LogicalOwner>, usize) {
    let mut grouped = BTreeMap::<(String, Option<String>), LogicalOwner>::new();
    let mut owner_count = 0;
    for reference in flattened {
        if !reference.node.flags.deep_link_target {
            continue;
        }
        owner_count += 1;
        let target = target_name(reference.node);
        let logical_path = logical_owner_path(reference);
        let key = (logical_path.clone(), target.clone());
        grouped
            .entry(key)
            .and_modify(|owner| {
                owner.raw_owner_count += 1;
                owner.section_heading |= reference.section_heading;
            })
            .or_insert_with(|| LogicalOwner {
                target,
                owner_source_line: reference.node.line,
                owner_macro: reference
                    .node
                    .macro_name
                    .clone()
                    .unwrap_or_else(|| "<none>".to_owned()),
                owner_kind: format!("{:?}", reference.node.kind).to_ascii_lowercase(),
                ast_path: logical_path,
                section_heading: reference.section_heading,
                section_ordinal: reference.section_ordinal,
                section_source_line: reference.section_source_line,
                order: reference.order,
                raw_owner_count: 1,
                explicit: None,
            });
    }
    let mut owners = grouped.into_values().collect::<Vec<_>>();
    owners.sort_by_key(|owner| owner.order);
    (owners, owner_count)
}

pub(super) fn bind_explicit_targets(
    owners: &mut [LogicalOwner],
    explicit: &[ExplicitTarget],
) -> Vec<ExplicitTarget> {
    let mut unmatched_explicit = Vec::new();
    for (index, target) in explicit.iter().cloned().enumerate() {
        let previous_explicit_order = index
            .checked_sub(1)
            .and_then(|previous| explicit.get(previous))
            .map_or(0, |previous| previous.order);
        let next_explicit_order = explicit
            .get(index + 1)
            .map_or(usize::MAX, |next| next.order);
        let containing_owner = owners
            .iter_mut()
            .filter(|owner| {
                owner.explicit.is_none()
                    && owner.target.as_deref() == Some(target.id.as_str())
                    && target.ast_path.starts_with(&format!("{}.", owner.ast_path))
            })
            .max_by_key(|owner| owner.ast_path.len());
        if let Some(owner) = containing_owner {
            owner.explicit = Some(target);
            continue;
        }
        let preceding_paragraph_owner = target.argumentless.then(|| {
            owners
                .iter_mut()
                .filter(|owner| {
                    owner.explicit.is_none()
                        && owner.owner_macro == "Pp"
                        && owner.target.as_deref() == Some(target.id.as_str())
                        && owner.order > previous_explicit_order
                        && owner.order < target.order
                        && preceding_sibling(&owner.ast_path, &target.ast_path)
                })
                .max_by_key(|owner| owner.order)
        });
        if let Some(Some(owner)) = preceding_paragraph_owner {
            owner.explicit = Some(target);
            continue;
        }
        let following_owner = owners
            .iter_mut()
            .filter(|owner| {
                owner.explicit.is_none()
                    && owner.target.as_deref() == Some(target.id.as_str())
                    && owner.order >= target.order
                    && owner.order < next_explicit_order
            })
            .min_by_key(|owner| owner.order);
        if let Some(owner) = following_owner {
            owner.explicit = Some(target);
        } else {
            unmatched_explicit.push(target);
        }
    }
    unmatched_explicit
}

fn preceding_sibling(candidate: &str, target: &str) -> bool {
    let Some((candidate_parent, candidate_index)) = candidate.rsplit_once('.') else {
        return false;
    };
    let Some((target_parent, target_index)) = target.rsplit_once('.') else {
        return false;
    };
    candidate_parent == target_parent
        && candidate_index
            .parse::<usize>()
            .ok()
            .zip(target_index.parse::<usize>().ok())
            .is_some_and(|(candidate, target)| candidate < target)
}

fn assemble_native_profile(
    owners: Vec<LogicalOwner>,
    unmatched_explicit: Vec<ExplicitTarget>,
    owner_count: usize,
) -> NativeTargetProfile {
    let logical_owner_count = owners.len() + unmatched_explicit.len();
    let mut targets = Vec::new();
    let mut classes = BTreeMap::<(String, String, OwnerDisposition, &'static str), usize>::new();
    let mut unclassified = Vec::new();
    let mut unclassified_raw_count = 0;

    for logical in owners {
        let owner = classify_target_owner(&logical);
        *classes
            .entry((
                owner.owner_macro.clone(),
                owner.owner_kind.clone(),
                owner.disposition,
                owner.reason,
            ))
            .or_default() += logical.raw_owner_count;
        if owner.disposition == OwnerDisposition::Unclassified {
            unclassified_raw_count += logical.raw_owner_count;
            unclassified.push(UnclassifiedOwner {
                target: owner.target,
                source_line: owner.source_line,
                owner_macro: owner.owner_macro,
                owner_kind: owner.owner_kind,
                ast_path: logical.ast_path.clone(),
                logical_owner_key: logical_owner_key(&logical),
                raw_owner_count: logical.raw_owner_count,
                reason: owner.reason,
            });
            continue;
        }
        if owner.disposition == OwnerDisposition::Excluded {
            continue;
        }
        let Some(id) = owner.target else {
            continue;
        };
        let expected_container = expected_container(&owner.owner_macro);
        targets.push(ExpectedTarget {
            normalized_id: document_id_slug(&id),
            explicit: owner.explicit,
            id,
            source_line: logical
                .explicit
                .as_ref()
                .map_or(logical.owner_source_line, |target| target.source_line),
            owner_source_line: logical.owner_source_line,
            owner_macro: owner.owner_macro,
            owner_kind: owner.owner_kind,
            ast_path: logical.ast_path.clone(),
            logical_owner_key: logical_owner_key(&logical),
            section_ordinal: logical.section_ordinal,
            section_source_line: logical.section_source_line,
            expected_role: owner.expected_role,
            expected_container,
        });
    }

    for target in unmatched_explicit {
        let normalized_id = document_id_slug(&target.id);
        targets.push(ExpectedTarget {
            id: target.id,
            normalized_id,
            source_line: target.source_line,
            owner_source_line: target.source_line,
            owner_macro: "Tg".to_owned(),
            owner_kind: "element".to_owned(),
            ast_path: target.ast_path.clone(),
            logical_owner_key: format!("tg:{}", target.ast_path),
            section_ordinal: target.section_ordinal,
            section_source_line: target.section_source_line,
            expected_role: TargetRole::Anchor,
            expected_container: "content",
            explicit: true,
        });
    }

    let classified_logical_owner_count = logical_owner_count - unclassified.len();
    NativeTargetProfile {
        expected: targets,
        owner_count,
        classified_owner_count: owner_count - unclassified_raw_count,
        logical_owner_count,
        classified_logical_owner_count,
        owner_classes: classes
            .into_iter()
            .map(
                |((owner_macro, owner_kind, disposition, reason), count)| OwnerClass {
                    owner_macro,
                    owner_kind,
                    disposition: disposition.as_str(),
                    reason,
                    count,
                },
            )
            .collect(),
        unclassified,
    }
}

pub(super) fn classify_target_owner(logical: &LogicalOwner) -> ClassifiedOwner {
    let target = logical
        .explicit
        .as_ref()
        .map(|target| target.id.clone())
        .or_else(|| logical.target.clone());
    let is_explicit = logical.explicit.is_some();
    let owner_macro = logical.owner_macro.clone();
    let owner_kind = logical.owner_kind.clone();
    let is_section_owner =
        logical.section_heading || matches!(owner_macro.as_str(), "SH" | "SS" | "Sh" | "Ss");
    let expected_role = if is_section_owner {
        TargetRole::Section
    } else {
        TargetRole::Anchor
    };
    let (disposition, reason) = if is_section_owner && !is_explicit {
        (
            OwnerDisposition::Excluded,
            "section uses the complete visible heading as its normalized identity",
        )
    } else if owner_macro == "Tg" && target.is_none() {
        (
            OwnerDisposition::Excluded,
            "argument-less Tg delegates its destination to the following owner",
        )
    } else if target.is_none() {
        (
            OwnerDisposition::Unclassified,
            "deep-link owner has no validated target name",
        )
    } else if is_explicit || owner_macro == "Tg" {
        (OwnerDisposition::Retained, "source-authored Tg destination")
    } else if matches!(
        owner_macro.as_str(),
        "IP" | "TP"
            | "TQ"
            | "Pp"
            | "Bd"
            | "D1"
            | "Dl"
            | "Bl"
            | "It"
            | "Fo"
            | "Fn"
            | "Fl"
            | "Cm"
            | "Em"
            | "No"
            | "Sy"
            | "Dv"
            | "Ic"
            | "Li"
            | "Ms"
            | "Er"
            | "Va"
            | "Ev"
    ) {
        (
            OwnerDisposition::Retained,
            "validated non-section navigation destination",
        )
    } else {
        (
            OwnerDisposition::Unclassified,
            "owner macro has no target-conservation policy",
        )
    };
    ClassifiedOwner {
        target,
        source_line: logical.owner_source_line,
        owner_macro,
        owner_kind,
        explicit: is_explicit,
        expected_role,
        disposition,
        reason,
    }
}

fn explicit_targets(nodes: &[AstNodeRef<'_>]) -> Vec<ExplicitTarget> {
    let mut targets = Vec::new();
    for (index, reference) in nodes.iter().enumerate() {
        if reference.node.macro_name.as_deref() != Some("Tg") {
            continue;
        }
        let authored = explicit_target_argument(reference.node);
        let target = authored.clone().or_else(|| {
            nodes[index + 1..]
                .iter()
                .filter(|candidate| candidate.node.line > reference.node.line)
                .find_map(|candidate| source_token(candidate.node))
        });
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            targets.push(ExplicitTarget {
                id: target,
                argumentless: authored.is_none(),
                source_line: reference.node.line,
                ast_path: reference.path.clone(),
                section_ordinal: reference.section_ordinal,
                section_source_line: reference.section_source_line,
                order: reference.order,
            });
        }
    }
    targets
}

fn logical_owner_path(reference: &AstNodeRef<'_>) -> String {
    logical_owner_path_for(&reference.path, reference.node.kind)
}

pub(super) fn logical_owner_path_for(path: &str, kind: NodeKind) -> String {
    if matches!(kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) {
        path.rsplit_once('.')
            .map_or_else(|| path.to_owned(), |(parent, _)| parent.to_owned())
    } else {
        path.to_owned()
    }
}

fn logical_owner_key(owner: &LogicalOwner) -> String {
    format!(
        "{}:{}:{}",
        owner.section_ordinal,
        owner.ast_path,
        owner.target.as_deref().unwrap_or("<missing>")
    )
}

fn expected_container(owner_macro: &str) -> &'static str {
    match owner_macro {
        "SH" | "SS" | "Sh" | "Ss" => "section",
        "IP" | "TP" | "TQ" | "It" => "item",
        _ => "content",
    }
}

fn target_name(node: &Node) -> Option<String> {
    if node.macro_name.as_deref() == Some("Tg") {
        return explicit_target_argument(node);
    }
    node.tag
        .as_deref()
        .map(str::to_owned)
        .or_else(|| source_token(node))
        .filter(|target| !target.is_empty())
}

fn explicit_target_argument(node: &Node) -> Option<String> {
    if node.macro_name.as_deref() != Some("Tg") {
        return None;
    }
    first_text_on_line(node, node.line)
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(str::to_owned)
}

fn source_token(node: &Node) -> Option<String> {
    first_text(node)
        .map(first_token)
        .filter(|target| !target.is_empty())
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text && !node.flags.no_print {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

fn first_text_on_line(node: &Node, line: u32) -> Option<&str> {
    if node.kind == NodeKind::Text && node.line == line {
        return node.text.as_deref();
    }
    node.children
        .iter()
        .find_map(|child| first_text_on_line(child, line))
}

fn first_token(value: &str) -> String {
    value
        .trim_start_matches('-')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn flatten_nodes<'a>(
    node: &'a Node,
    path: &str,
    parent_section_line: u32,
    parent_section_ordinal: usize,
    inside_section_heading: bool,
    next_section_ordinal: &mut usize,
    output: &mut Vec<AstNodeRef<'a>>,
) {
    let is_section = node.kind == NodeKind::Block
        && matches!(node.macro_name.as_deref(), Some("SH" | "SS" | "Sh" | "Ss"));
    let section_source_line = if is_section {
        node.line
    } else {
        parent_section_line
    };
    let section_ordinal = if is_section {
        *next_section_ordinal += 1;
        *next_section_ordinal
    } else {
        parent_section_ordinal
    };
    let section_heading = inside_section_heading
        || (node.kind == NodeKind::Head
            && matches!(node.macro_name.as_deref(), Some("SH" | "SS" | "Sh" | "Ss")));
    let order = output.len();
    output.push(AstNodeRef {
        node,
        path: path.to_owned(),
        section_heading,
        section_ordinal,
        section_source_line,
        order,
    });
    for (index, child) in node.children.iter().enumerate() {
        flatten_nodes(
            child,
            &format!("{path}.{index}"),
            section_source_line,
            section_ordinal,
            section_heading,
            next_section_ordinal,
            output,
        );
    }
}
