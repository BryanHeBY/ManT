//! Assemble definition HEAD/BODY owners using one projected inline context.

use std::borrow::Cow;

use mant_ir::{Block, DefinitionItem, EntryOwner, Inline, InlineContentRef, InlineLayout};

use super::super::inline::{
    InlineRootNodes, block_prefix_escape_position, link_destination, render_inline_node_refs,
    render_inline_owner_node_refs,
};
use super::super::mapped::MappedText;
use super::super::{MarkdownInlineProjection, MarkdownOptions};
use super::{join_definition_items, nonempty, project_inline, render_block};

pub(super) fn render_definition_list(
    items: &[DefinitionItem],
    compact: bool,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let rendered = items
        .iter()
        .filter_map(|item| {
            let (mut content, has_terms) = definition_content(item, options, locations, track)?;
            // `render_inline` protects block markers visible inside one inline
            // run, but a hanging definition can assemble the marker only when
            // its term and description are joined (`1.` + ` text`). Protect
            // that final first line so a semantic definition cannot reparse as
            // a nested ordered list in the public CommonMark projection.
            if has_terms && let Some(position) = block_prefix_escape_position(&content.text) {
                content.insert(position, "\\");
            }
            content
                .with_owner(EntryOwner::Definition(item), track)
                .prefix("- ")
                .map(|content| (content, item.layout.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    join_definition_items(rendered, compact)
}

/// Keep the first effective BODY block and its already executed boundary
/// together. `VerticalSpace`'s distance can simplify in `CommonMark`, but its
/// paragraph boundary cannot disappear when an empty output block is skipped.
struct DefinitionBody<'a> {
    blocks: Vec<MappedText>,
    first_prose: Option<Vec<InlineRoot<'a>>>,
    leading_space: bool,
}

struct InlineRoot<'a> {
    nodes: Cow<'a, [Inline]>,
    layout: &'a InlineLayout,
    has_output: bool,
}

impl<'a> InlineRoot<'a> {
    fn project(
        content: InlineContentRef<'a>,
        locations: Option<&dyn MarkdownInlineProjection>,
        options: MarkdownOptions,
    ) -> Self {
        let nodes = project_inline(content.content, locations);
        let has_output = inline_output(&nodes, options) == InlineOutput::Visible;
        Self {
            nodes,
            layout: content.layout,
            has_output,
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
                    // This standalone root had no visible output. A wrapper
                    // omitted by the shared policy cannot revive its trimmed
                    // label whitespace when ownership roots are assembled.
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

fn render_roots<'a>(
    roots: impl Iterator<Item = &'a InlineRoot<'a>>,
    options: MarkdownOptions,
) -> String {
    let roots = roots
        .map(|root| {
            let mut nodes = Vec::new();
            root.append_nodes(&mut nodes, options);
            InlineRootNodes {
                nodes,
                layout: root.layout,
            }
        })
        .collect::<Vec<_>>();
    if roots.iter().all(|root| root.layout.is_empty()) {
        let nodes = roots
            .iter()
            .flat_map(|root| root.nodes.iter().copied())
            .collect::<Vec<_>>();
        return render_inline_node_refs(&nodes, options);
    }
    render_inline_owner_node_refs(&roots, options)
}

fn definition_body<'a>(
    blocks: &'a [Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> DefinitionBody<'a> {
    let mut body = DefinitionBody {
        blocks: Vec::new(),
        first_prose: None,
        leading_space: false,
    };
    let mut destinations = Vec::new();
    for block in blocks {
        if body.blocks.is_empty() && mant_ir::geometry::block_gap(block) > 0 {
            body.leading_space = true;
        }
        if matches!(block, Block::VerticalSpace { .. }) {
            continue;
        }
        if let Block::Paragraph {
            children,
            inline_layout,
            ..
        } = block
        {
            // Decoration receives the original root, never an assembled seam.
            let projected = InlineRoot::project(
                InlineContentRef {
                    content: children,
                    layout: inline_layout,
                },
                locations,
                options,
            );
            if body.blocks.is_empty() {
                let has_output = projected.has_output;
                destinations.push(projected);
                if !has_output {
                    continue;
                }
                let rendered = render_roots(destinations.iter(), options);
                body.first_prose = Some(std::mem::take(&mut destinations));
                body.blocks.push(rendered.into());
            } else if let Some(rendered) =
                nonempty(render_roots(std::iter::once(&projected), options))
            {
                body.blocks.push(rendered);
            }
        } else if let Some(mut rendered) = render_block(block, options, locations, track) {
            if body.blocks.is_empty() {
                let markers = render_roots(destinations.iter(), options);
                if !markers.is_empty() {
                    rendered.insert(0, &format!("{markers}\n\n"));
                }
                destinations.clear();
            }
            body.blocks.push(rendered);
        }
    }
    if body.blocks.is_empty()
        && let Some(markers) = nonempty(render_roots(destinations.iter(), options))
    {
        body.blocks.push(markers);
    }
    body
}

#[derive(PartialEq, Eq)]
enum InlineOutput {
    Empty,
    LabelSpacing,
    Visible,
}

fn inline_output(nodes: &[Inline], options: MarkdownOptions) -> InlineOutput {
    let mut output = InlineOutput::Empty;
    for node in nodes {
        // Classify glyphs and label spacing together in one borrowed pass.
        // A nested transparent wrapper cannot trigger another subtree scan.
        let current = match node {
            Inline::Text { value } if !value.trim_matches([' ', '\t']).is_empty() => {
                InlineOutput::Visible
            }
            Inline::Text { value } if !value.is_empty() => InlineOutput::LabelSpacing,
            Inline::Code { value } | Inline::Equation { value, .. } if !value.is_empty() => {
                InlineOutput::Visible
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                inline_output(children, options)
            }
            Inline::Link {
                target, children, ..
            } => {
                let label = inline_output(children, options);
                if label == InlineOutput::LabelSpacing
                    && link_destination(target, options, false).is_some()
                {
                    InlineOutput::Visible
                } else {
                    label
                }
            }
            Inline::LineBreak { .. } => InlineOutput::Visible,
            Inline::Anchor { .. }
            | Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. } => InlineOutput::Empty,
        };
        match current {
            InlineOutput::Visible => return current,
            InlineOutput::LabelSpacing => output = current,
            InlineOutput::Empty => {}
        }
    }
    output
}

fn definition_content(
    item: &DefinitionItem,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<(MappedText, bool)> {
    let projected = item
        .terms
        .iter()
        .map(|root| InlineRoot::project(root.inline_content(), locations, options))
        .collect::<Vec<_>>();
    let has_terms = projected.iter().any(|root| root.has_output);
    // Zero-width navigation belongs to an adjacent real term row, never a new
    // row selected only because preserving destinations emits HTML syntax.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut pending = Vec::new();
    for (index, root) in projected.iter().enumerate() {
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
    let mut terms = groups
        .into_iter()
        .filter_map(|indices| {
            let text = render_roots(indices.iter().map(|&index| &projected[index]), options);
            (!text.is_empty()).then_some((indices, text))
        })
        .collect::<Vec<_>>();
    let mut body = definition_body(&item.description, options, locations, track);
    if let (Some((indices, term)), Some(prose)) = (terms.last_mut(), &body.first_prose)
        && has_terms
        && !body.leading_space
        && item.layout.head_body_relation.joins_without_separator()
    {
        // Source ownership stays split; one inline context selects delimiters
        // for the physical row. Independent encoded strings cannot be joined:
        // adjacent strong/emphasis/code delimiters may become literal text.
        let roots = indices
            .iter()
            .map(|&index| &projected[index])
            .chain(prose.iter());
        *term = render_roots(roots, options);
        body.blocks.remove(0);
        body.first_prose = None;
        let head = terms.into_iter().map(|(_, text)| MappedText::from(text));
        let head = MappedText::join(head, "  \n");
        let tail = MappedText::join(body.blocks, "\n\n");
        return Some((
            if tail.text.is_empty() {
                head
            } else {
                MappedText::join([head, tail], "\n\n")
            },
            has_terms,
        ));
    }
    let head = MappedText::join(terms.into_iter().map(|(_, text)| text.into()), "  \n");
    let description = MappedText::join(body.blocks, "\n\n");
    let content = match (head.text.is_empty(), description.text.is_empty()) {
        (false, false) if !has_terms => {
            // Only destinations were retained from HEAD: preserve their
            // syntax without fabricating a label row or a word separator.
            MappedText::join([head, description], "")
        }
        (false, false) => {
            let separator = if body.leading_space {
                "\n\n"
            } else if body.first_prose.is_some() {
                if item.layout.inline_term() {
                    " "
                } else {
                    "  \n"
                }
            } else {
                // Fences, nested lists and display equations need their own
                // block; they never share the term's inline coding context.
                "\n"
            };
            MappedText::join([head, description], separator)
        }
        (false, true) => head,
        (true, false) => description,
        (true, true) => return None,
    };
    Some((content, has_terms))
}
