//! Man link labels are annotations over normally executed block content.
use super::{Block, Inline, Node, NodeKind, first_part_children, source_span};
use crate::mandoc::inline::ownership::{Annotation, OutputMembership};
use crate::mandoc::inline::{InlineBuilder, append_inline_nodes, man_link_destination};
use libmandoc_rs::{MacroToken::Man, ManMacro};
use mant_ir::LinkTarget;

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_man_link(&mut self, node: &Node) {
        let head = first_part_children(node, NodeKind::Head);
        let body = first_part_children(node, NodeKind::Body);
        let (target, has_target) =
            man_link_destination(head, node.macro_token.as_ref() == Some(&Man(ManMacro::Mt)));

        let registered = self
            .state
            .formatter
            .execution
            .zero_advance
            .new_annotation(target);
        let target = registered.target;

        // The label can contain PP, IP, nested UR/MT, and fi/nf requests.
        // man_term.c::print_man_node() executes them with the ordinary node
        // driver; only the output annotation is specific to UR/MT.
        let output_start = self.state.link_output_cursor(registered.key);
        self.state
            .formatter
            .execution
            .zero_advance
            .begin_output_owner();
        if has_target && !body.is_empty() {
            self.state
                .formatter
                .execution
                .zero_advance
                .qualify_current_link(target.clone());
        }
        self.state.formatter.font.man_text_boundary(); // BLOCK pre
        self.state.formatter.font.man_text_boundary(); // HEAD pre
        self.state.formatter.font.man_text_boundary(); // HEAD post
        self.state.formatter.font.man_text_boundary(); // BODY pre
        // A link BODY is a new non-RS sibling list. print_bvspace() cannot
        // climb through UR/MT to an earlier paragraph outside this BODY.
        let inherited = std::mem::take(&mut self.man_source_predecessor);
        self.push_nodes(body);
        self.man_source_predecessor = inherited;
        self.state.formatter.font.man_text_boundary(); // BODY post

        // man_term.c::post_UR() always executes its first generated word,
        // even for an empty HEAD. Its boundary may settle a final BODY \z
        // glyph, while a tight \c still permits the bracket to overstrike.
        self.push_link_word(node, InlineBuilder::prepare_generated_word);
        let membership = self
            .state
            .formatter
            .execution
            .zero_advance
            .end_output_owner();
        let head_is_label = body.is_empty() && has_target;
        let cursor_executed = has_target && !head_is_label;
        if cursor_executed {
            self.state.formatter.execution.zero_advance.bind_deferred(
                membership,
                &Annotation::Link {
                    target: target.clone(),
                    title: None,
                },
                true,
            );
        }
        if cursor_executed
            && !self
                .state
                .wrap_first_link_since(output_start, &target, membership)
        {
            self.push_link_word(node, |builder| {
                builder.append(vec![Inline::Link {
                    target: target.clone(),
                    title: None,
                    children: Vec::new(),
                }]);
            });
        }
        self.state
            .finish_output_membership(output_start, membership);
        self.push_link_word(node, |builder| {
            builder.append_prepared_text("<");
            builder.tighten_next_boundary();
            if head_is_label {
                builder.append_scope(
                    |builder| append_inline_nodes(builder, head, self.context.default_name),
                    |children| {
                        vec![Inline::Link {
                            target: target.clone(),
                            title: None,
                            children,
                        }]
                    },
                );
            } else {
                append_inline_nodes(builder, head, self.context.default_name);
            }
            builder.tighten_next_boundary();
            builder.append_text(">");
        });
        let content_marker = format!("{}:content", link_cursor_marker(registered.key));
        self.state
            .formatter
            .execution
            .pending_output_scope_prefixes
            .retain(|marker| marker != &content_marker);
        let entry = membership
            .has_enclosing()
            .then(|| crate::mandoc::inline::ownership::markers::entry_marker(&target))
            .flatten();
        self.state
            .discard_link_cursor(output_start, entry.as_deref());
        self.state.formatter.font.man_text_boundary(); // BLOCK post
    }

    fn push_link_word(&mut self, node: &Node, append: impl FnOnce(&mut InlineBuilder)) {
        if self.state.formatter.no_fill {
            self.push_no_fill_generated(node, false, false, false, append);
        } else {
            self.state
                .push_inline_with(source_span(node), false, false, append);
        }
    }
}

#[derive(Default, Eq, PartialEq)]
enum LabelStart {
    #[default]
    AwaitCursor,
    AwaitContent,
    Content,
}

#[derive(Default)]
pub(super) struct LabelRange {
    start: LabelStart,
    closed: bool,
    identity: bool,
}

pub(super) fn wrap_first_visible_inline(
    nodes: &mut Vec<Inline>,
    target: &LinkTarget,
    marker: &str,
    range: &mut LabelRange,
    membership: OutputMembership,
) -> bool {
    if range.closed {
        return false;
    }
    let start = if let Some(index) = nodes.iter().rposition(|node| is_link_cursor(node, marker)) {
        range.start = LabelStart::AwaitContent;
        index + 1
    } else if range.start != LabelStart::AwaitCursor {
        0
    } else {
        return false;
    };
    let suffix = nodes.split_off(start);
    let (mut children, after, closed) =
        crate::mandoc::inline::split_output_scope_prefix(suffix, &membership.end_marker());
    range.closed = closed;
    if range.start != LabelStart::Content {
        let (prefix, content, found) = crate::mandoc::inline::split_output_scope_prefix(
            children,
            &format!("{marker}:content"),
        );
        nodes.extend(prefix);
        children = content;
        if found {
            range.start = LabelStart::Content;
        }
    }
    if range.start != LabelStart::Content {
        nodes.extend(children);
        let wrapped = contains_annotation(&after, target);
        if !range.identity && !wrapped {
            nodes.push(Inline::Link {
                target: target.clone(),
                title: None,
                children: Vec::new(),
            });
        }
        range.identity = true;
        nodes.extend(after);
        return true;
    }
    // A trailing device row close remains outside the semantic label. A later
    // member in the same root can reclaim that internal boundary at assembly.
    let tail_start = children
        .iter()
        .rposition(|node| !matches!(node, Inline::Anchor { .. } | Inline::LineBreak { .. }))
        .map_or(0, |index| index + 1);
    let trailing = if children[tail_start..]
        .iter()
        .any(|node| matches!(node, Inline::LineBreak { .. }))
    {
        children.split_off(tail_start)
    } else {
        Vec::new()
    };
    if children.is_empty() {
        nodes.extend(trailing);
        let wrapped = contains_annotation(&after, target);
        if !range.identity && !wrapped {
            nodes.push(Inline::Link {
                target: target.clone(),
                title: None,
                children: Vec::new(),
            });
        }
        range.identity = true;
        nodes.extend(after);
        return true;
    }
    let mut wrapped = false;
    nodes.extend(membership.map_owned(children, |children| {
        wrapped = true;
        vec![Inline::Link {
            target: target.clone(),
            title: None,
            children,
        }]
    }));
    nodes.extend(trailing);
    wrapped |= contains_annotation(&after, target);
    range.identity |= wrapped;
    nodes.extend(after);
    wrapped
}

fn contains_annotation(nodes: &[Inline], target: &LinkTarget) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Link {
            target: candidate,
            children,
            ..
        } => candidate == target || contains_annotation(children, target),
        Inline::Strong { children } | Inline::Emphasis { children } => {
            contains_annotation(children, target)
        }
        _ => false,
    })
}

pub(super) fn wrap_first_visible_block(
    block: &mut Block,
    target: &LinkTarget,
    marker: &str,
    range: &mut LabelRange,
    membership: OutputMembership,
) -> bool {
    let mut wrapped = false;
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            wrapped |= wrap_first_visible_inline(children, target, marker, range, membership);
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &mut item.blocks {
                    wrapped |= wrap_first_visible_block(block, target, marker, range, membership);
                }
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                for term in &mut item.terms {
                    wrapped |= wrap_first_visible_inline(
                        &mut term.content,
                        target,
                        marker,
                        range,
                        membership,
                    );
                }
                for block in &mut item.description {
                    wrapped |= wrap_first_visible_block(block, target, marker, range, membership);
                }
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &mut row.cells {
                    for block in &mut cell.blocks {
                        wrapped |=
                            wrap_first_visible_block(block, target, marker, range, membership);
                    }
                }
            }
        }
        _ => {}
    }
    wrapped
}

// One marker per active UR/MT scope. It cannot be authored by roff input
// (NUL terminates native strings), never writes a formatter cell and dies
// at that scope's exit. A suffix lookup avoids rescanning accumulated words.
pub(super) fn finish_output_membership(
    block: &mut Block,
    marker: &str,
    started: &mut bool,
    membership: OutputMembership,
) {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            finish_inline_membership(children, marker, started, membership);
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &mut item.blocks {
                    finish_output_membership(block, marker, started, membership);
                }
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                for term in &mut item.terms {
                    finish_inline_membership(&mut term.content, marker, started, membership);
                }
                for block in &mut item.description {
                    finish_output_membership(block, marker, started, membership);
                }
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &mut row.cells {
                    for block in &mut cell.blocks {
                        finish_output_membership(block, marker, started, membership);
                    }
                }
            }
        }
        _ => {}
    }
}

/// The cursor survives first-label annotation until all accepted fragments
/// are classified. Only that scope's suffix is visited: a long live paragraph
/// of independent UR/MT links must not be rebuilt at every wrapper exit.
pub(super) fn finish_inline_membership(
    nodes: &mut Vec<Inline>,
    marker: &str,
    started: &mut bool,
    membership: OutputMembership,
) {
    let start = if let Some(at) = nodes.iter().rposition(|node| is_link_cursor(node, marker)) {
        *started = true;
        at + 1
    } else if *started {
        0
    } else {
        return;
    };
    let suffix = nodes.split_off(start);
    nodes.extend(membership.map_owned(suffix, |nodes| nodes));
}

pub(super) fn link_cursor_marker(
    owner: crate::mandoc::inline::ownership::annotations::AnnotationKey,
) -> String {
    crate::mandoc::inline::ownership::markers::link_scope(owner)
}

fn is_link_cursor(node: &Inline, marker: &str) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str() == marker)
}

pub(super) fn remove_link_cursor(
    nodes: &mut Vec<Inline>,
    marker: &str,
    entry: Option<&str>,
) -> bool {
    let Some(index) = nodes.iter().rposition(|node| is_link_cursor(node, marker)) else {
        return false;
    };
    if let Some(entry) = entry {
        nodes[index] = Inline::anchor(entry);
    } else {
        nodes.remove(index);
    }
    remove_label_marker(&mut nodes[index..], &format!("{marker}:content"));
    true
}

fn remove_label_marker(nodes: &mut [Inline], marker: &str) {
    for node in nodes {
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                children.retain(
                    |node| !matches!(node, Inline::Anchor { id, .. } if id.as_str() == marker),
                );
                remove_label_marker(children, marker);
            }
            _ => {}
        }
    }
}

pub(super) fn remove_link_cursor_block(
    block: &mut Block,
    marker: &str,
    entry: Option<&str>,
) -> bool {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            remove_link_cursor(children, marker, entry) && children.is_empty()
        }
        Block::List { items, .. } => {
            for item in items {
                item.blocks
                    .retain_mut(|block| !remove_link_cursor_block(block, marker, entry));
            }
            false
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                for term in &mut item.terms {
                    remove_link_cursor(&mut term.content, marker, entry);
                }
                item.description
                    .retain_mut(|block| !remove_link_cursor_block(block, marker, entry));
            }
            false
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &mut row.cells {
                    cell.blocks
                        .retain_mut(|block| !remove_link_cursor_block(block, marker, entry));
                }
            }
            false
        }
        _ => false,
    }
}
