//! Annotate already executed output without creating formatter state.

use super::super::super::links;
use super::{Font, INTERNAL_LINK_SPLIT, INTERNAL_OUTPUT_SCOPE, Inline, InlineBuilder, projection};

impl InlineBuilder {
    /// Mark a local annotation scope without writing a formatter cell. A
    /// native flush may change its output length; the marker survives that
    /// receipt, so its owner never relies on a pre-flush vector index.
    pub(in crate::mandoc::inline) fn begin_output_scope(&mut self, marker: &str) {
        self.nodes.push(Inline::anchor(marker));
    }

    /// Find only this scope's new tail, then reuse the native committed/pending
    /// split. Missing metadata leaves the accepted text unannotated.
    pub(in crate::mandoc::inline) fn wrap_output_scope(
        &mut self,
        marker: &str,
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        if let Some(start) = self
            .nodes
            .iter()
            .rposition(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == marker))
        {
            self.wrap_output_from(start, wrap);
        }
    }

    /// Only macros that select a font establish this scope. Transparent
    /// macros and text operands must not invent a push/pop of their own.
    pub(in crate::mandoc) fn with_font_scope(
        &mut self,
        font: Font,
        append: impl FnOnce(&mut Self),
    ) {
        let saved = self.execution.font.push_scope(font);
        append(self);
        self.execution.font.pop_scope(saved);
    }

    /// Wrap one IR suffix without moving the native field's commit boundary.
    /// A wrapper can replace many children by one Link/Emphasis node; when a
    /// real `term_flushln()` fell inside it, keep accepted and pending slices
    /// separate so a later nbr=0 only rejects the pending slice.
    pub(super) fn wrap_output_from(
        &mut self,
        start: usize,
        mut wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        let end = self.nodes.len();
        let mut inner = self.nodes.split_off(start);
        let field_start = self
            .execution
            .author_execution
            .as_ref()
            .map(|author| author.field_output_start);
        if let Some(field_start) = field_start.filter(|&field_start| {
            self.execution.definition.is_some() && start < field_start && field_start < end
        }) {
            // Keep the committed prefix and the current native field in
            // distinct wrappers. A later term_fill() rejection must never
            // erase or retain part of the wrong wrapped output owner.
            let pending = inner.split_off(field_start - start);
            let accepted = wrap(inner);
            let pending = wrap(pending);
            let actual_link = accepted
                .last()
                .is_some_and(|node| !links::presentation::is_private_owner(node));
            let split_link = actual_link
                && matches!((accepted.last(), pending.last()),
                (Some(Inline::Link { target: left, title: left_title, .. }),
                 Some(Inline::Link { target: right, title: right_title, .. }))
                    if left == right && left_title == right_title)
                || matches!((accepted.last(), pending.last()), (Some(left), Some(right))
                    if links::presentation::same_link_owner(left, right));
            self.nodes.extend(accepted);
            if split_link {
                // This is one authored link across two native fields. Keep
                // its parts separate until the pending field is accepted or
                // rejected, then join the presentation owner at IR drain.
                self.nodes.push(Inline::anchor(INTERNAL_LINK_SPLIT));
            }
            let new_field_start = self.nodes.len();
            self.nodes.extend(pending);
            self.execution
                .author_execution
                .as_mut()
                .unwrap()
                .field_output_start = new_field_start;
        } else {
            self.nodes.extend(wrap(inner));
            if let Some(author) = &mut self.execution.author_execution
                && author.field_output_start >= end
                && end > start
            {
                author.field_output_start = self.nodes.len();
            }
        }
    }

    /// Style newly appended content without creating a new formatter state.
    /// The transform must preserve visible characters and line boundaries.
    /// In particular, a wrapper ending after `\\c` must not consume its
    /// pending join merely because its content is represented as a subtree.
    pub(in crate::mandoc) fn append_scope(
        &mut self,
        append: impl FnOnce(&mut Self),
        style: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        // Appending must still see the prefix, especially a preceding hard
        // break used for deduplication. Style only the new suffix afterwards.
        let checkpoint = self.begin_output_checkpoint();
        append(self);
        self.wrap_output_since(checkpoint, style);
    }

    /// Annotate operands without capturing their executed automatic prefix.
    /// `term_word()` writes that prefix before encoding the source spelling
    /// (term.c:573-589); inspecting leading whitespace cannot distinguish it
    /// from the author's own blank glyphs. These bounded local markers do.
    /// Return whether this occurrence produced a typed annotation prefix;
    /// a cached tail can rejoin only that prefix, never a preceding occurrence.
    pub(in crate::mandoc) fn append_semantic_scope(
        &mut self,
        owner: crate::mandoc::inline::ownership::annotations::AnnotationKey,
        retain_empty: bool,
        append: impl FnOnce(&mut Self),
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) -> (bool, crate::mandoc::inline::ownership::OutputMembership) {
        self.append_semantic_scope_with_entry(owner, retain_empty, true, append, wrap)
    }

    /// An equation's empty identity starts at its first actual operand.
    /// Earlier layout markers and foreign glyphs keep their original owners.
    pub(in crate::mandoc) fn append_semantic_word_scope(
        &mut self,
        owner: crate::mandoc::inline::ownership::annotations::AnnotationKey,
        append: impl FnOnce(&mut Self),
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) -> (bool, crate::mandoc::inline::ownership::OutputMembership) {
        self.append_semantic_scope_with_entry(owner, true, false, append, wrap)
    }

    fn append_semantic_scope_with_entry(
        &mut self,
        owner: crate::mandoc::inline::ownership::annotations::AnnotationKey,
        retain_empty: bool,
        retain_entry_identity: bool,
        append: impl FnOnce(&mut Self),
        mut wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) -> (bool, crate::mandoc::inline::ownership::OutputMembership) {
        self.execution.zero_advance.begin_output_owner();
        let scope = format!("{INTERNAL_OUTPUT_SCOPE}semantic:{owner}");
        let content = format!("{scope}:content");
        self.begin_output_scope(&scope);
        self.pending_output_scope_prefixes.push(content.clone());
        append(self);
        self.pending_output_scope_prefixes
            .retain(|pending| pending != &content);
        let membership = self.execution.zero_advance.end_output_owner();
        let mut started = false;
        let mut annotated = false;
        self.wrap_output_scope(&scope, |children| {
            membership.map_owned(children, |children| {
                if started {
                    if !has_annotation_member(&children) && !retain_empty {
                        return children;
                    }
                    let output = wrap(children);
                    annotated |= output
                        .iter()
                        .any(|node| matches!(node, Inline::Link { .. }));
                    return output;
                }
                let (mut prefix, children, found) =
                    projection::split_output_scope_prefix(children, &content);
                started = found;
                if found && (has_annotation_member(&children) || retain_empty) {
                    let output = wrap(children);
                    annotated |= output
                        .iter()
                        .any(|node| matches!(node, Inline::Link { .. }));
                    prefix.extend(output);
                } else {
                    prefix.extend(children);
                    if retain_empty && retain_entry_identity && !annotated {
                        let output = wrap(Vec::new());
                        annotated |= output
                            .iter()
                            .any(|node| matches!(node, Inline::Link { .. }));
                        prefix.extend(output);
                    }
                }
                prefix
            })
        });
        (annotated, membership)
    }
}

fn has_annotation_member(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Anchor { id, .. } => !id.as_str().starts_with("\0mant:"),
        Inline::Strong { children } | Inline::Emphasis { children } => {
            has_annotation_member(children)
        }
        _ => true,
    })
}
