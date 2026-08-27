use super::{DocumentBuilder, NodeId, NodeKind, StructureOutcome};

/// Mark the man heading nodes that libmandoc validates as same-document
/// destinations. The common one-word form deliberately keeps `tag` absent:
/// libmandoc reuses that child text as the destination instead of allocating a
/// second string, while the two boolean flags preserve the public semantic
/// contract used by navigation lowering. Multiword SH/SS tags are fallback
/// names, so a duplicate suppresses both targets rather than silently choosing
/// an arbitrary source-order winner.
pub(super) fn mark_man_targets(builder: &mut DocumentBuilder, heads: &[NodeId]) {
    let mut tags = std::collections::BTreeMap::<String, TagEntry>::new();
    for head in heads {
        let Some(macro_name) = builder.node_macro_name(*head).map(str::to_owned) else {
            continue;
        };
        if matches!(macro_name.as_str(), "SH" | "SS") {
            let heading = visible_head_text(builder, *head);
            let tag = heading.split_whitespace().collect::<Vec<_>>().join("_");
            let Some(text) = first_head_text(builder, *head) else {
                continue;
            };
            if tag.is_empty() {
                continue;
            }
            let direct_text = builder
                .children(*head)
                .and_then(|children| children.first())
                .and_then(|child| builder.node_text(*child));
            let priority = if tag == heading {
                TagPriority::Strong
            } else {
                TagPriority::Fallback
            };
            let explicit = direct_text != Some(text) || tag != text;
            register_man_tag(
                builder,
                &mut tags,
                *head,
                &TagCandidate {
                    name: tag,
                    priority,
                    explicit,
                },
            );
            continue;
        }

        let candidate_text = match macro_name.as_str() {
            "IP" => builder
                .children(*head)
                .and_then(|children| children.first())
                .and_then(|child| builder.node_text(*child).map(|text| (*child, text))),
            "TP" | "TQ" => first_man_term_text(builder, *head),
            _ => None,
        };
        let Some((text_node, text)) = candidate_text else {
            continue;
        };
        let Some((name, priority, start, end)) = parse_man_tag(text) else {
            continue;
        };
        let direct_text_node = builder
            .children(*head)
            .and_then(|children| children.first())
            .copied()
            .filter(|child| builder.node_text(*child).is_some());
        register_man_tag(
            builder,
            &mut tags,
            *head,
            &TagCandidate {
                name,
                priority,
                explicit: direct_text_node != Some(text_node) || start != 0 || end != text.len(),
            },
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TagPriority {
    Strong,
    Weak,
    Fallback,
    Deleted,
}

#[derive(Debug)]
struct TagCandidate {
    name: String,
    priority: TagPriority,
    explicit: bool,
}

#[derive(Debug)]
struct TagEntry {
    priority: TagPriority,
    heads: Vec<NodeId>,
}

/// Register a man(7) automatic target with the same priority transitions as
/// libmandoc's `tag_put`: strong names win over weak names, fallback section
/// names only survive when unique, and equal-strength names retain every
/// source-order occurrence.
fn register_man_tag(
    builder: &mut DocumentBuilder,
    tags: &mut std::collections::BTreeMap<String, TagEntry>,
    head: NodeId,
    candidate: &TagCandidate,
) {
    use std::collections::btree_map::Entry;

    match tags.entry(candidate.name.clone()) {
        Entry::Vacant(entry) => {
            mark_target(
                builder,
                head,
                candidate.explicit.then_some(candidate.name.as_str()),
            );
            entry.insert(TagEntry {
                priority: candidate.priority,
                heads: vec![head],
            });
        }
        Entry::Occupied(mut occupied) => {
            let entry = occupied.get_mut();
            if entry.priority < candidate.priority {
                return;
            }
            if entry.priority > candidate.priority || candidate.priority == TagPriority::Fallback {
                for previous in entry.heads.drain(..) {
                    clear_target(builder, previous);
                }
                if candidate.priority == TagPriority::Fallback {
                    entry.priority = TagPriority::Deleted;
                    return;
                }
                entry.priority = candidate.priority;
            }
            mark_target(
                builder,
                head,
                candidate.explicit.then_some(candidate.name.as_str()),
            );
            entry.heads.push(head);
        }
    }
}

/// Mirror `man_validate.c:check_tag` over the preserved AST spelling.  The
/// parser intentionally retains the small escape subset used for automatic
/// anchors so source-compatible tags do not depend on renderer state.
fn parse_man_tag(text: &str) -> Option<(String, TagPriority, usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut priority = TagPriority::Strong;
    loop {
        match *bytes.get(cursor)? {
            b' ' | b'\t' => {
                priority = TagPriority::Weak;
                cursor += 1;
            }
            b'-' => cursor += 1,
            b'\\' => cursor = skip_tag_escape(bytes, cursor)?,
            byte if byte.is_ascii_alphabetic() => {
                let start = cursor;
                while let Some(byte) = bytes.get(cursor) {
                    if matches!(*byte, b' ' | b'\t' | b'\\') {
                        break;
                    }
                    cursor += 1;
                }
                if cursor == start {
                    return None;
                }
                if cursor != bytes.len() && priority == TagPriority::Strong {
                    priority = TagPriority::Weak;
                }
                return Some((text[start..cursor].to_owned(), priority, start, cursor));
            }
            _ => return None,
        }
    }
}

/// Advance over the exact leading escape families accepted by libmandoc's
/// tag validator: font changes, ignored zero-width controls, and the two
/// printable one-character special escapes it treats as a dash/backslash.
fn skip_tag_escape(bytes: &[u8], slash: usize) -> Option<usize> {
    let kind = *bytes.get(slash + 1)?;
    match kind {
        b'&' | b'-' | b'e' => Some(slash + 2),
        b'f' => match *bytes.get(slash + 2)? {
            b'[' => bytes[slash + 3..]
                .iter()
                .position(|byte| *byte == b']')
                .map(|offset| slash + 4 + offset),
            b'(' => bytes.get(slash + 4).map(|_| slash + 5),
            _ => Some(slash + 3),
        },
        _ => None,
    }
}

/// `post_TP` considers the first node that starts on a subsequent physical
/// source line.  If it is one of six font macros, it peels exactly one level;
/// deeper macro nesting deliberately does not become an automatic target.
fn first_man_term_text(builder: &DocumentBuilder, head: NodeId) -> Option<(NodeId, &str)> {
    let term = builder.children(head)?.iter().copied().find(|node| {
        builder
            .node_flags(*node)
            .is_some_and(|flags| flags.line_start)
    })?;
    let term = match builder.node_macro_name(term) {
        Some("B" | "BI" | "BR" | "I" | "IB" | "IR") => *builder.children(term)?.first()?,
        _ => term,
    };
    builder.node_text(term).map(|text| (term, text))
}

fn mark_target(builder: &mut DocumentBuilder, head: NodeId, tag: Option<&str>) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = true;
    flags.permalink = true;
    let _ = builder.set_node_flags(head, flags);
    if let Some(tag) = tag {
        let _ = builder.set_node_tag(head, tag);
    }
}

fn clear_target(builder: &mut DocumentBuilder, head: NodeId) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = false;
    flags.permalink = false;
    let _ = builder.set_node_flags(head, flags);
    let _ = builder.clear_node_tag(head);
}

fn first_head_text(builder: &DocumentBuilder, head: NodeId) -> Option<&str> {
    let first = *builder.children(head)?.first()?;
    builder.node_text(first).or_else(|| {
        builder
            .children(first)
            .and_then(|children| children.first())
            .and_then(|child| builder.node_text(*child))
    })
}

fn visible_head_text(builder: &DocumentBuilder, head: NodeId) -> String {
    let mut text = Vec::new();
    let mut pending = builder
        .children(head)
        .map(|children| children.iter().copied().rev().collect::<Vec<_>>())
        .unwrap_or_default();
    while let Some(node) = pending.pop() {
        if let Some(value) = builder.node_text(node) {
            text.push(value);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied().rev());
        }
    }
    text.join(" ")
}

pub(super) fn append_to_active(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    node: NodeId,
) {
    if parent == root {
        root_children.push(node);
    } else {
        let _ = builder.append_existing_child(parent, node);
    }
}

pub(super) fn make_block(
    builder: &mut DocumentBuilder,
    block: NodeId,
    macro_name: &str,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<(NodeId, NodeId)> {
    if builder.node_kind(block) != Some(NodeKind::Element) {
        return None;
    }
    if builder.node_count().saturating_add(2) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(block);
        }
        return None;
    }
    let arguments = builder.children(block)?.to_vec();
    let location = builder.node_location(block);
    let head = builder.push(block, NodeKind::Head)?;
    let body = builder.push(block, NodeKind::Body)?;
    if !builder.set_node_kind(block, NodeKind::Block)
        || !builder.macro_name(block, macro_name)
        || !builder.macro_name(head, macro_name)
        || !builder.macro_name(body, macro_name)
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(block, &[head, body])
        || !builder.replace_children(head, &arguments)
    {
        return None;
    }
    Some((head, body))
}
