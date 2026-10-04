//! Pure IR projection after native receipts have already decided output.

use super::{
    CompletedRowOrigin, INTERNAL_COMPLETED_ROW, INTERNAL_DEVICE_ROW_END, INTERNAL_LINK_SPLIT,
    INTERNAL_LITERAL_ROW, Inline,
};

/// A row-end receipt can precede or follow the same projected delimiter.
/// Accepted later cells start another row; completed empty rows have their
/// own summary and must not be counted as this graph-row boundary.
pub(in crate::mandoc) fn trailing_device_row_end_receipt(nodes: &[Inline]) -> bool {
    fn visit(nodes: &[Inline], protected: &mut bool) {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } if id.as_str() == INTERNAL_DEVICE_ROW_END => {
                    *protected = true;
                }
                Inline::Anchor { id, .. }
                    if matches!(id.as_str(), INTERNAL_COMPLETED_ROW | INTERNAL_LITERAL_ROW) =>
                {
                    *protected = false;
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => visit(children, protected),
                Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => {
                    *protected = false;
                }
                _ => {}
            }
        }
    }
    let mut protected = false;
    visit(nodes, &mut protected);
    protected
}

/// Receipts are read only after native acceptance has removed rejected
/// glyphs. Accepted intervening glyphs already represent earlier hard rows
/// inside the paragraph; an unprinted suffix cannot hide their empty rows.
pub(in crate::mandoc) fn trailing_completed_row_receipts(nodes: &[Inline]) -> u16 {
    fn visit(nodes: &[Inline], rows: &mut u16) {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } if id.as_str() == INTERNAL_COMPLETED_ROW => {
                    *rows = rows.saturating_add(1);
                }
                Inline::Anchor { id, .. } if id.as_str() == INTERNAL_LITERAL_ROW => {
                    *rows = 0;
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => visit(children, rows),
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. }
                    if value.chars().any(|character| !character.is_whitespace()) =>
                {
                    *rows = 0;
                }
                _ => {}
            }
        }
    }
    let mut rows = 0;
    visit(nodes, &mut rows);
    rows
}

/// Preserve the order of completed source rows in the current accepted tail.
/// Raw no-fill empty TEXT is literal content; spacing requests remain layout.
/// Later accepted glyphs consume earlier tail ownership in this same Vec.
pub(in crate::mandoc) fn trailing_completed_row_origins(
    nodes: &[Inline],
) -> Vec<CompletedRowOrigin> {
    fn visit(nodes: &[Inline], rows: &mut Vec<CompletedRowOrigin>) {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } if id.as_str() == INTERNAL_COMPLETED_ROW => {
                    rows.push(CompletedRowOrigin::Layout);
                }
                Inline::Anchor { id, .. } if id.as_str() == INTERNAL_LITERAL_ROW => {
                    rows.push(CompletedRowOrigin::LiteralText);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => visit(children, rows),
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. }
                    if !value.is_empty() =>
                {
                    rows.clear();
                }
                _ => {}
            }
        }
    }
    let mut rows = Vec::new();
    visit(nodes, &mut rows);
    rows
}

/// Locate the operand boundary recorded during execution. Styles and nested
/// semantic owners can surround that marker, but cannot move it past an
/// authored leading blank or make automatic padding part of a link label.
pub(super) fn split_output_scope_prefix(
    nodes: Vec<Inline>,
    marker: &str,
) -> (Vec<Inline>, Vec<Inline>, bool) {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut found = false;
    for mut node in nodes {
        if found {
            suffix.push(node);
            continue;
        }
        if matches!(&node, Inline::Anchor { id, .. } if id.as_str() == marker) {
            found = true;
            continue;
        }
        let children = match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => Some(std::mem::take(children)),
            _ => None,
        };
        if let Some(children) = children {
            let (left, right, nested_found) = split_output_scope_prefix(children, marker);
            if !nested_found {
                replace_scope_children(&mut node, left);
                prefix.push(node);
                continue;
            }
            if !left.is_empty() {
                let mut before = node.clone();
                replace_scope_children(&mut before, left);
                prefix.push(before);
            }
            if !right.is_empty()
                || crate::mandoc::inline::links::presentation::retains_authored_identity(&node)
            {
                replace_scope_children(&mut node, right);
                suffix.push(node);
            }
            found = true;
        } else {
            prefix.push(node);
        }
    }
    (prefix, suffix, found)
}

fn replace_scope_children(node: &mut Inline, replacement: Vec<Inline>) {
    match node {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => *children = replacement,
        _ => unreachable!("only semantic and font containers have scope children"),
    }
}

/// Retain authored identities when a native receipt removes their row glyphs.
/// `term_fill()` can reject the label while `mdoc_html.c::mdoc_lk_pre()`
/// still preserves the href. Identity does not occupy a formatter cell or
/// supply a replacement label, address, or activation range.
pub(in crate::mandoc) fn retain_inline_identities(inlines: &mut Vec<Inline>) {
    inlines.retain_mut(|inline| {
        let identity =
            crate::mandoc::inline::links::presentation::retains_authored_identity(inline);
        match inline {
            Inline::Anchor { .. } => !super::is_private_output_marker(inline),
            Inline::LineBreak { .. }
            | Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. } => false,
            Inline::Link { children, .. } => {
                retain_inline_identities(children);
                true
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                retain_inline_identities(children);
                identity || !children.is_empty()
            }
        }
    });
}

/// Semantic identity/styling does not hide the physical output row end.
/// An empty Text is an occupied row witness and deliberately stops the scan.
pub(in crate::mandoc) fn ends_with_executed_line_break(nodes: &[Inline]) -> bool {
    fn ending(nodes: &[Inline]) -> Option<bool> {
        for node in nodes.iter().rev() {
            match node {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => return Some(true),
                Inline::Strong { children }
                | Inline::Emphasis { children }
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

/// Consume exactly one represented ending of an already closed native row.
/// Metadata and style wrappers do not alter the row event; an empty TEXT
/// remains an occupied-cell witness and prevents consuming an earlier row.
pub(in crate::mandoc) fn consume_one_row_ending(nodes: &mut Vec<Inline>) -> bool {
    fn consume(nodes: &mut Vec<Inline>) -> Option<bool> {
        for index in (0..nodes.len()).rev() {
            match &mut nodes[index] {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => {
                    nodes.remove(index);
                    return Some(true);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if let Some(result) = consume(children) {
                        return Some(result);
                    }
                }
                _ => return Some(false),
            }
        }
        None
    }
    consume(nodes) == Some(true)
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
    prepare_inline_output(nodes);
    strip_native_projection_markers(nodes);
}

pub(in crate::mandoc) fn prepare_inline_output(nodes: &mut Vec<Inline>) {
    super::super::super::links::presentation::finalize_accepted_links(nodes);
    join_authored_links(nodes);
}

pub(in crate::mandoc) fn strip_native_projection_markers(nodes: &mut Vec<Inline>) {
    nodes.retain_mut(|node| match node {
        Inline::Anchor { .. } => {
            super::row_origins::is_layout_carrier(node)
                || super::is_term_alternative(node)
                || !super::is_private_output_marker(node)
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => {
            strip_native_projection_markers(children);
            true
        }
        _ => true,
    });
}

pub(in crate::mandoc) fn native_row_origin(node: &Inline) -> Option<usize> {
    let Inline::Anchor { id, .. } = node else {
        return None;
    };
    id.as_str()
        .strip_prefix(super::INTERNAL_ROW_ORIGIN)?
        .parse()
        .ok()
}

pub(in crate::mandoc) fn has_rendered_formatter_glyph(nodes: &[Inline]) -> bool {
    let mut found = false;
    mant_ir::visit_inline_plain_text(nodes, |text| {
        // term_field() buffers ordinary SP/TAB until an encoded glyph.
        // An authored Unicode fixed blank is itself that encoded glyph
        // (term.c:389-427), so it cannot become an invisible row receipt.
        found |= text
            .chars()
            .any(|character| !matches!(character, ' ' | '\t' | '\r' | '\n'));
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
        // Native source-owner markers can separate these two projections
        // when a cached glyph settles at the next word. They are metadata,
        // not a different authored link or an intervening visible boundary.
        let is_owner_marker = |at: usize| {
            matches!(&nodes[at], Inline::Anchor { id, .. }
                if id.as_str().starts_with(super::INTERNAL_FIELD_WORD)
                    || id.as_str().strip_prefix(super::INTERNAL_OUTPUT_SCOPE).is_some_and(
                        |scope| scope.starts_with("lk:") || scope.starts_with("semantic:")))
        };
        let left = (0..index).rev().find(|&at| !is_owner_marker(at));
        let next = (index + 1..nodes.len())
            .find(|&at| !is_owner_marker(at))
            .unwrap_or(nodes.len());
        let prefix = matches!(nodes.get(next), Some(Inline::Text { value }) if value.chars().all(char::is_whitespace));
        let right = next + usize::from(prefix);
        let merge = match (left.and_then(|at| nodes.get(at)), nodes.get(right)) {
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
            } = &mut nodes[left.unwrap()]
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
            | Inline::Link { children, .. } => line_break_count(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. } => 0,
        })
        .sum()
}
