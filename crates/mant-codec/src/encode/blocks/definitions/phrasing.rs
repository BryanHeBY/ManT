//! Borrow projected owner roots into one phrasing context and group HEAD rows.
use super::super::super::inline::{link_destination, render_inline_node_refs};
use super::super::super::{MarkdownInlineProjection, MarkdownOptions};
use super::super::project_inline;
use mant_ir::{DefinitionTerm, Inline, InlineContentRef};
use std::borrow::Cow;

pub(in crate::encode::blocks) struct InlineRoot<'a> {
    nodes: Cow<'a, [Inline]>,
    pub(in crate::encode::blocks) has_output: bool,
    pub(in crate::encode::blocks) open_row: bool,
}

impl<'a> InlineRoot<'a> {
    pub(in crate::encode::blocks) fn project(
        content: InlineContentRef<'a>,
        locations: Option<&dyn MarkdownInlineProjection>,
    ) -> Self {
        let nodes = project_inline(content.content, locations);
        let has_output = has_body_scalar(&nodes);
        let open_row = mant_ir::last_visible_character(&nodes) == Some('\n');
        Self {
            nodes,
            has_output,
            open_row,
        }
    }

    fn append_nodes<'root>(&'root self, output: &mut Vec<&'root Inline>, options: MarkdownOptions) {
        if self.has_output {
            output.extend(self.nodes.iter());
        } else {
            append_destinations(&self.nodes, output, options);
        }
    }
}

fn append_destinations<'a>(
    nodes: &'a [Inline],
    output: &mut Vec<&'a Inline>,
    options: MarkdownOptions,
) {
    for node in nodes {
        match node {
            Inline::Anchor { .. } => output.push(node),
            Inline::Link {
                target, children, ..
            } => {
                if link_destination(target, options, false).is_some() {
                    output.push(node);
                } else {
                    // This standalone root owns no body scalars. A transparent
                    // wrapper contributes only its destinations to the stream.
                    append_destinations(children, output, options);
                }
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                append_destinations(children, output, options);
            }
            _ => {}
        }
    }
}

pub(in crate::encode::blocks) fn render_roots<'a>(
    roots: impl Iterator<Item = &'a InlineRoot<'a>>,
    options: MarkdownOptions,
) -> String {
    let mut nodes = Vec::new();
    for root in roots {
        root.append_nodes(&mut nodes, options);
    }
    render_inline_node_refs(&nodes, options)
}

fn has_body_scalar(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
            !value.is_empty()
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_body_scalar(children),
        Inline::LineBreak {} => true,
        Inline::Anchor { .. } => false,
    })
}

pub(super) struct DefinitionHead<'a> {
    pub(super) roots: Vec<InlineRoot<'a>>,
    pub(super) terms: Vec<(Vec<usize>, String)>,
    pub(super) has_output: bool,
}

impl<'a> DefinitionHead<'a> {
    pub(super) fn project(
        terms: &'a [DefinitionTerm],
        options: MarkdownOptions,
        locations: Option<&dyn MarkdownInlineProjection>,
    ) -> Self {
        let roots = terms
            .iter()
            .map(|root| InlineRoot::project(root.inline_content(), locations))
            .collect::<Vec<_>>();
        let has_output = roots.iter().any(|root| root.has_output);
        // Navigation stays on an adjacent real HEAD row, never a new row
        // selected solely because its Markdown syntax is nonempty.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut pending = Vec::new();
        for (index, root) in roots.iter().enumerate() {
            if root.has_output {
                pending.push(index);
                groups.push(std::mem::take(&mut pending));
            } else if let Some(last) = groups.last_mut() {
                last.push(index);
            } else {
                pending.push(index);
            }
        }
        if !pending.is_empty() {
            groups.push(pending);
        }
        let terms = groups
            .into_iter()
            .filter_map(|mut indices| {
                // Every group has at most one physical root. Keep destination
                // roots before it so trailing navigation cannot occupy that
                // root's open hard tail. Move once per group, never per target.
                if let Some(position) = indices.iter().position(|&i| roots[i].has_output) {
                    let physical = indices.remove(position);
                    indices.push(physical);
                }
                let text = render_roots(indices.iter().map(|&index| &roots[index]), options);
                (!text.is_empty()).then_some((indices, text))
            })
            .collect();
        Self {
            roots,
            terms,
            has_output,
        }
    }
}
