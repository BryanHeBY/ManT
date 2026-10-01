//! Pure IR projection after native receipts have already decided output.

use super::{INTERNAL_FIELD_WORD, INTERNAL_LINK_SPLIT, INTERNAL_OUTPUT_SCOPE, Inline};

/// Semantic identity/styling does not hide the physical output row end.
/// An empty Text is an occupied row witness and deliberately stops the scan.
pub(in crate::mandoc::inline::flow) fn ends_with_executed_line_break(nodes: &[Inline]) -> bool {
    fn ending(nodes: &[Inline]) -> Option<bool> {
        for node in nodes.iter().rev() {
            match node {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => return Some(true),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. }
                | Inline::Link { children, .. } => {
                    if let Some(result) = ending(children) {
                        return Some(result);
                    }
                }
                _ => return Some(false),
            }
        }
        None
    }
    ending(nodes) == Some(true)
}

/// Paragraph terminators are transparent to metadata and styling. Actual
/// vertical output has already transferred to its completed-row owner.
pub(super) fn trim_output_terminators(nodes: &mut Vec<Inline>) -> bool {
    let mut trimming = true;
    let mut retained = Vec::with_capacity(nodes.len());
    for mut node in nodes.drain(..).rev() {
        if trimming {
            match &mut node {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => continue,
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. }
                | Inline::Link { children, .. } => trimming = trim_output_terminators(children),
                Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => {
                    trimming = false;
                }
            }
        }
        retained.push(node);
    }
    retained.reverse();
    *nodes = retained;
    trimming
}

/// End one projection destination after its word receipts have executed.
/// Native cell owners are private and must never become visible occupancy
/// witnesses, authored navigation targets, or serialized IR.
pub(in crate::mandoc) fn finalize_inline_output(nodes: &mut Vec<Inline>) {
    super::super::super::links::presentation::finalize_accepted_links(nodes);
    join_authored_links(nodes);
    strip_native_projection_markers(nodes);
}

fn strip_native_projection_markers(nodes: &mut Vec<Inline>) {
    nodes.retain_mut(|node| match node {
        Inline::Anchor { id, .. } => {
            !id.as_str().starts_with(INTERNAL_FIELD_WORD)
                && !id.as_str().starts_with(INTERNAL_OUTPUT_SCOPE)
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::PortableDisplay { children, .. }
        | Inline::Link { children, .. } => {
            strip_native_projection_markers(children);
            true
        }
        _ => true,
    });
}

pub(super) fn has_non_whitespace_glyph(nodes: &[Inline]) -> bool {
    let mut found = false;
    mant_ir::visit_inline_plain_text(nodes, |text| {
        found |= text.chars().any(|character| !character.is_whitespace());
    });
    found
}

/// Count formatter-breakable ASCII blanks at the end of the current field.
/// Generated fixed cells reset this accounting at their call sites, while
/// non-breaking spaces remain distinguishable by their Unicode value.
pub(in crate::mandoc::inline) fn trailing_ascii_spaces(nodes: &[Inline]) -> usize {
    fn visit(nodes: &[Inline], count: &mut usize) -> bool {
        for node in nodes.iter().rev() {
            match node {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => return false,
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let trailing = value.chars().rev().take_while(|&ch| ch == ' ').count();
                    *count = count.saturating_add(trailing);
                    if trailing != value.chars().count() {
                        return false;
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. }
                | Inline::Link { children, .. } => {
                    if !visit(children, count) {
                        return false;
                    }
                }
            }
        }
        true
    }

    let mut count = 0;
    visit(nodes, &mut count);
    count
}

/// Rejoin one authored link whose accepted and pending fields were wrapped
/// separately. Remove the private split marker after field acceptance has
/// been decided; matching typed identity does not merge unrelated links.
fn join_authored_links(nodes: &mut Vec<Inline>) {
    for node in nodes.iter_mut() {
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => join_authored_links(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak { .. } => {}
        }
    }

    let mut index = 0;
    while index < nodes.len() {
        let marker = matches!(&nodes[index], Inline::Anchor { id, .. } if id.as_str() == INTERNAL_LINK_SPLIT);
        if !marker {
            index += 1;
            continue;
        }
        let next = index + 1;
        let prefix = matches!(nodes.get(next), Some(Inline::Text { value }) if value.chars().all(char::is_whitespace));
        let right = next + usize::from(prefix);
        let merge = match (
            index.checked_sub(1).and_then(|left| nodes.get(left)),
            nodes.get(right),
        ) {
            (
                Some(Inline::Link {
                    target: left,
                    title: left_title,
                    ..
                }),
                Some(Inline::Link {
                    target: right,
                    title: right_title,
                    ..
                }),
            ) => left == right && left_title == right_title,
            _ => false,
        };
        if merge {
            let Inline::Link { mut children, .. } = nodes.remove(right) else {
                unreachable!("checked matching link")
            };
            if prefix {
                children.insert(0, nodes.remove(next));
            }
            if let Inline::Link {
                children: accepted, ..
            } = &mut nodes[index - 1]
            {
                accepted.append(&mut children);
            }
        }
        nodes.remove(index);
    }
}

pub(in crate::mandoc) fn trim_trailing_breakable_spaces(nodes: &mut Vec<Inline>, count: usize) {
    fn trim(nodes: &mut Vec<Inline>, remaining: &mut usize) -> bool {
        let mut index = nodes.len();
        while index > 0 && *remaining > 0 {
            index -= 1;
            let remove = match &mut nodes[index] {
                Inline::Anchor { .. } => continue,
                Inline::LineBreak { .. } => return false,
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let original_len = value.len();
                    while *remaining > 0 && value.ends_with(' ') {
                        value.pop();
                        *remaining -= 1;
                    }
                    let remove = value.is_empty() && original_len > 0;
                    if !remove && !value.ends_with(' ') {
                        return false;
                    }
                    remove
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    if !trim(children, remaining) {
                        return false;
                    }
                    children.is_empty()
                }
                Inline::Link { children, .. } => {
                    // A discarded native field can empty the label, but the
                    // authored destination is still a typed IR fact (CVS
                    // mdoc_html.c keeps its href). Trim label blanks only.
                    if !trim(children, remaining) {
                        return false;
                    }
                    false
                }
                Inline::PortableDisplay { children, .. } => {
                    // The wrapper retains native terminal facts: trim blanks
                    // inside it, but never discard the wrapper itself.
                    if !trim(children, remaining) {
                        return false;
                    }
                    false
                }
            };
            if remove {
                nodes.remove(index);
            }
        }
        *remaining > 0
    }

    let mut remaining = count;
    trim(nodes, &mut remaining);
}

/// Count executed hard-row boundaries through semantic wrappers.
pub(super) fn line_break_count(nodes: &[Inline]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Inline::LineBreak { .. } => 1,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => line_break_count(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. } => 0,
        })
        .sum()
}
