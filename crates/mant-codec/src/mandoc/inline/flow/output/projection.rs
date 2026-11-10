//! Pure IR projection after native receipts have already decided output.

use super::{INTERNAL_FIELD_WORD, INTERNAL_LINK_SPLIT, Inline};

impl super::super::InlineExecutionState {
    pub(in crate::mandoc) fn note_removed_plain_output_node(&mut self, index: usize) {
        if index < self.plain_input.output_start {
            self.plain_input.output_start -= 1;
        }
    }

    /// Man link annotation can wrap a committed literal row and a live
    /// native word together. Carry the original output boundary through
    /// that local rewrite and split the wrapper until native retirement.
    pub(in crate::mandoc) fn rewrite_plain_output_suffix<R>(
        &mut self,
        nodes: &mut Vec<Inline>,
        from: usize,
        rewrite: impl FnOnce(&mut Vec<Inline>) -> R,
    ) -> R {
        if self.definition.is_some()
            || self.plain_input.buffer.is_empty()
            || self.plain_input.output_start < from
        {
            return rewrite(nodes);
        }
        let start = &mut self.plain_input.output_start;
        if *start >= nodes.len() {
            let result = rewrite(nodes);
            *start = nodes.len();
            return result;
        }
        let marker = format!("{INTERNAL_FIELD_WORD}rewrite-boundary");
        nodes.insert(*start, Inline::anchor(marker.clone()));
        let result = rewrite(nodes);
        *start = restore_plain_output_boundary(nodes, from, &marker);
        result
    }
}

fn restore_plain_output_boundary(nodes: &mut Vec<Inline>, from: usize, marker: &str) -> usize {
    fn contains(node: &Inline, marker: &str) -> bool {
        match node {
            Inline::Anchor { id, .. } => id.as_str() == marker,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => children.iter().any(|child| contains(child, marker)),
            _ => false,
        }
    }

    let index = from
        + nodes[from..]
            .iter()
            .position(|node| contains(node, marker))
            .expect("link rewrite retains the native output boundary");
    let tail = nodes.split_off(index);
    let mut tail = tail.into_iter();
    let root = tail.next().expect("located boundary root");
    let (left, right, _) = split_output_scope_prefix(vec![root], marker);
    let split_link = matches!((left.last(), right.first()),
        (Some(Inline::Link { target: left, title: lt, .. }),
         Some(Inline::Link { target: right, title: rt, .. })) if left == right && lt == rt);
    nodes.extend(left);
    if split_link {
        nodes.push(Inline::anchor(INTERNAL_LINK_SPLIT));
    }
    let start = nodes.len();
    nodes.extend(right);
    nodes.extend(tail);
    start
}

/// Locate the operand boundary recorded during execution. Styles and nested
/// semantic owners can surround that marker, but cannot move it past an
/// authored leading blank or make automatic padding part of a link label.
pub(in crate::mandoc) fn split_output_scope_prefix(
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
        let delayed = crate::mandoc::inline::ownership::is_delayed_origin(inline);
        let identity =
            crate::mandoc::inline::links::presentation::retains_authored_identity(inline);
        match inline {
            Inline::Anchor { id, .. } => {
                // The final native receipt can reject both sides. The
                // structural seam must still be split before either drain.
                id.as_str() == super::section_handoff::BOUNDARY
                    || !super::is_private_output_marker(inline)
            }
            Inline::LineBreak { .. }
            | Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. } => false,
            Inline::Link { children, .. } => {
                retain_inline_identities(children);
                !delayed || !children.is_empty()
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                retain_inline_identities(children);
                identity || !children.is_empty()
            }
        }
    });
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
        node if crate::mandoc::inline::ownership::is_delayed_origin(node) => true,
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

pub(in crate::mandoc) fn native_row_origin(node: &Inline) -> Option<i32> {
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
    let mut output: Vec<Inline> = Vec::with_capacity(nodes.len());
    let mut seam = false;
    let mut between = Vec::new();
    for mut node in std::mem::take(nodes) {
        #[cfg(test)]
        crate::mandoc::inline::ownership::metrics::note(|counts| counts.normalization += 1);
        match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => join_authored_links(children),
            _ => {}
        }
        if matches!(&node, Inline::Anchor { id, .. } if id.as_str() == INTERNAL_LINK_SPLIT) {
            seam = true;
            continue;
        }
        let metadata = matches!(&node, Inline::Anchor { id, .. } if id.as_str().starts_with(super::INTERNAL_FIELD_WORD) || id.as_str().starts_with(super::INTERNAL_OUTPUT_SCOPE));
        if metadata
            || (seam
                && matches!(&node, Inline::Text { value } if value.chars().all(char::is_whitespace)))
        {
            between.push(node);
            continue;
        }
        if seam
            && let (
                Some(Inline::Link {
                    target: left,
                    title: lt,
                    children: accepted,
                }),
                Inline::Link {
                    target: right,
                    title: rt,
                    children,
                },
            ) = (output.last_mut(), &mut node)
            && left == right
            && lt == rt
        {
            accepted.append(&mut between);
            accepted.append(children);
            seam = false;
            continue;
        }
        output.append(&mut between);
        output.push(node);
        seam = false;
    }
    output.append(&mut between);
    *nodes = output;
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
