//! Maps native block nodes to portable `CommonMark` block constructs.

use std::borrow::Cow;

use mant_ir::{
    Block, DefinitionItem, EntryOwner, Inline, ListItem, ListKind, SourceSpan, TableRow,
    content_entries,
};

use super::inline::{
    block_prefix_escape_position, code_span, escape_text, fenced_code, flatten_inline, html_anchor,
    link_destination, preformatted_anchor_markers, protect_block_prefix, render_inline,
    render_inline_node_refs,
};
use super::mapped::MappedText;
use super::{MarkdownInlineProjection, MarkdownOptions};

pub(super) struct RenderedBlocks<'src> {
    pub(super) text: String,
    pub(super) entries: Vec<RenderedEntry<'src>>,
}

pub(super) struct RenderedEntry<'src> {
    pub(super) indices: Vec<usize>,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) owner: EntryOwner<'src>,
    pub(super) names: &'src [String],
    pub(super) source: Option<SourceSpan>,
}

pub(crate) fn render_blocks(blocks: &[Block], options: MarkdownOptions) -> Vec<String> {
    render_located_blocks(blocks, options, None)
}
pub(crate) fn render_located_blocks(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| render_block(block, options, locations, false))
        .map(|block| block.text)
        .collect()
}

pub(super) fn render_blocks_with_entries(
    blocks: &[Block],
    options: MarkdownOptions,
    track: bool,
) -> RenderedBlocks<'_> {
    let rendered = mapped_blocks(blocks, options, None, track);
    let entries = if track {
        let ranges = rendered.owner_ranges();
        content_entries(blocks)
            .into_iter()
            .filter_map(|located| {
                let owner = located.owner();
                let identity = owner.facts()?;
                let range = ranges.get(&std::ptr::from_ref(identity))?;
                Some(RenderedEntry {
                    owner,
                    names: located.names(),
                    indices: located.indices().to_vec(),
                    start: range.start,
                    end: range.end,
                    source: located.source(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    RenderedBlocks {
        text: rendered.text,
        entries,
    }
}

fn mapped_blocks(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> MappedText {
    MappedText::join(
        blocks
            .iter()
            .filter_map(|block| render_block(block, options, locations, track)),
        "\n\n",
    )
}

fn render_block(
    block: &Block,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    match block {
        Block::Paragraph { children, .. } => nonempty(inline(children, options, locations)),
        Block::Preformatted {
            children, language, ..
        } => {
            let code = fenced_code(&flatten_inline(children), language.as_deref());
            if options.preserve_anchors {
                let markers = preformatted_anchor_markers(children);
                if !markers.is_empty() {
                    return Some(format!("{markers}\n\n{code}").into());
                }
            }
            Some(code.into())
        }
        Block::List {
            kind,
            compact,
            items,
            ..
        } => render_list(*kind, *compact, items, options, locations, track),
        Block::DefinitionList { items, compact, .. } => {
            render_definition_list(items, *compact, options, locations, track)
        }
        Block::Table { rows, .. } => render_table(rows, options, track),
        Block::Equation { value, display, .. } => {
            if *display {
                Some(fenced_code(value, Some("math")).into())
            } else {
                nonempty(format!("Equation: {}", code_span(value)))
            }
        }
        Block::VerticalSpace { .. } => None,
        Block::ThematicBreak { .. } => Some("---".to_owned().into()),
        Block::Unsupported { name, text, .. } => {
            let text = escape_text(text.trim())
                .lines()
                .map(protect_block_prefix)
                .collect::<Vec<_>>()
                .join("  \n");
            if text.is_empty() {
                None
            } else {
                Some(
                    name.as_deref()
                        .map_or(text.clone(), |name| {
                            format!("**{}:** {text}", escape_text(name))
                        })
                        .into(),
                )
            }
        }
    }
}

fn render_list(
    kind: ListKind,
    compact: bool,
    items: &[ListItem],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let rendered = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let marker = match kind {
                ListKind::Ordered { .. } => {
                    format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                }
                ListKind::Bullet | ListKind::Dash | ListKind::Plain => "- ".to_owned(),
            };
            let mut blocks = item
                .blocks
                .iter()
                .filter_map(|block| render_block(block, options, locations, track))
                .collect::<Vec<_>>();
            if options.preserve_semantics
                && let Some(facts) = &item.entry
            {
                if let Some(head) = blocks.first_mut() {
                    head.text.push_str(&super::semantic::metadata(facts));
                }
                if let Some(domain) = facts
                    .value_domain
                    .as_ref()
                    .and_then(super::semantic::domain)
                {
                    blocks.insert(1, domain.into());
                }
            }
            let mut content = MappedText::join(blocks, "\n\n");
            if !options.preserve_semantics
                && options.preserve_anchors
                && let Some(facts) = &item.entry
            {
                content.insert(0, &html_anchor(&facts.id));
            }
            content
                .with_owner(EntryOwner::List(item), track)
                .prefix(&marker)
                .map(|content| (content, item.layout.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| {
        let mut content = join_definition_items(rendered, compact).expect("nonempty list items");
        if options.preserve_semantics
            && let Some(facts) = items.first().and_then(|i| i.entry.as_ref())
        {
            content.insert(
                0,
                &format!("{}\n", super::semantic::declaration(facts, items)),
            );
        }
        content
    })
}

fn render_definition_list(
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
    has_output: bool,
}

impl<'a> InlineRoot<'a> {
    fn project(
        nodes: &'a [Inline],
        locations: Option<&dyn MarkdownInlineProjection>,
        options: MarkdownOptions,
    ) -> Self {
        let nodes = project_inline(nodes, locations);
        let has_output = inline_output(&nodes, options) == InlineOutput::Visible;
        Self { nodes, has_output }
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
    let mut nodes = Vec::new();
    for root in roots {
        root.append_nodes(&mut nodes, options);
    }
    render_inline_node_refs(&nodes, options)
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
        if let Block::Paragraph { children, .. } = block {
            // Decoration receives the original root, never an assembled seam.
            let projected = InlineRoot::project(children, locations, options);
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
        .map(|root| InlineRoot::project(root, locations, options))
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

/// Preserve a man(7) `.PD` override when one is present, otherwise fall back
/// to the list-wide compactness used by mdoc(7) and HTML inputs.
fn join_definition_items(
    items: Vec<(MappedText, Option<u16>)>,
    compact: bool,
) -> Option<MappedText> {
    let mut items = items.into_iter();
    let (mut output, _) = items.next()?;
    for (item, spacing_before_lines) in items {
        let blank_lines = spacing_before_lines.unwrap_or(u16::from(!compact));
        output
            .text
            .push_str(&"\n".repeat(usize::from(blank_lines) + 1));
        output.append(item);
    }
    Some(output)
}

fn render_table(rows: &[TableRow], options: MarkdownOptions, track: bool) -> Option<MappedText> {
    // Empty data rows and native rules are authored structure, not fence
    // delimiters. Keep them in the same source-order portable row plan.
    let markers = if options.preserve_anchors {
        rows.iter()
            .filter(|row| mant_ir::table_row_is_navigation_only(row))
            .flat_map(|row| &row.cells)
            .flat_map(|cell| &cell.blocks)
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(preformatted_anchor_markers(children)),
                _ => None,
            })
            .collect::<String>()
    } else {
        String::new()
    };
    let rows = super::flat::rows(rows, track);
    if rows.is_empty() {
        return nonempty(markers);
    }
    Some({
        let mut body = MappedText::join(rows, "\n");
        let fenced = fenced_code(&body.text, None);
        let prefix = fenced.find('\n').expect("fence header") + 1;
        for (_, range) in &mut body.owners {
            range.start += prefix;
            range.end += prefix;
        }
        body.text = fenced;
        if !markers.is_empty() {
            body.insert(0, &format!("{markers}\n\n"));
        }
        body
    })
}

fn nonempty(value: String) -> Option<MappedText> {
    MappedText::from(value).nonempty()
}

fn inline(
    nodes: &[mant_ir::Inline],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> String {
    render_inline(&project_inline(nodes, locations), options)
}

fn project_inline<'a>(
    nodes: &'a [Inline],
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Cow<'a, [Inline]> {
    locations.map_or(Cow::Borrowed(nodes), |map| map.project(nodes))
}
