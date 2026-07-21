//! Reconstructs sections and semantic blocks from the copied mandoc tree.

use libmandoc_rs::{
    DisplayKind, Node, NodeKind, NormalizedListKind, TableAlignment as MandocTableAlignment,
};
use mant_ast::{
    Block, DefinitionItem, Inline, LayoutHint, ListItem, ListKind, Section,
    TableAlignment as AstTableAlignment, TableCell as AstTableCell, TableRow,
};

use super::{
    LoweringContext,
    inline::{InlineBuilder, lower_inline_nodes, parse_roff_text, plain_text},
    part_children, source_span,
};

pub(super) fn lower_sections(root: &Node, context: &mut LoweringContext<'_>) -> Vec<Section> {
    let mut paragraph_distance = 1;
    let mut sections = Vec::new();
    for node in &root.children {
        update_paragraph_distance(node, &mut paragraph_distance);
        if !is_section(node, true) {
            continue;
        }
        let has_preceding_content = sections.last().is_some_and(section_has_body);
        let spacing_before_lines = section_spacing(
            node,
            sections.is_empty(),
            has_preceding_content,
            paragraph_distance,
        );
        sections.push(lower_section(
            node,
            context,
            spacing_before_lines,
            &mut paragraph_distance,
        ));
    }
    sections
}

fn lower_section(
    node: &Node,
    context: &mut LoweringContext<'_>,
    spacing_before_lines: u16,
    paragraph_distance: &mut u16,
) -> Section {
    let heading = lower_inline_nodes(part_children(node, NodeKind::Head), context.default_name);
    let title = plain_text(&heading).trim().to_owned();
    // Allocate IDs in visible document order. Besides being deterministic for
    // consumers, this makes `.Sx` resolution independent of tree recursion.
    let id = context.section_id(&title);
    let body = part_children(node, NodeKind::Body);
    let first_subsection = body
        .iter()
        .position(|child| is_section(child, false))
        .unwrap_or(body.len());
    let blocks = lower_blocks(&body[..first_subsection], context, 0, paragraph_distance);
    let mut children = Vec::new();
    let mut has_preceding_content = !blocks.is_empty();
    for child in &body[first_subsection..] {
        update_paragraph_distance(child, paragraph_distance);
        if !is_section(child, false) {
            continue;
        }
        let child_spacing = section_spacing(
            child,
            children.is_empty(),
            has_preceding_content,
            *paragraph_distance,
        );
        let child = lower_section(child, context, child_spacing, paragraph_distance);
        has_preceding_content = section_has_body(&child);
        children.push(child);
    }
    Section {
        id,
        title,
        spacing_before_lines,
        blocks,
        children,
        source: source_span(node),
    }
}

fn update_paragraph_distance(node: &Node, paragraph_distance: &mut u16) {
    if node.macro_name.as_deref() == Some("PD")
        && let Some(lines) = paragraph_distance_lines(node)
    {
        *paragraph_distance = lines;
    }
}

fn section_spacing(
    node: &Node,
    is_first: bool,
    has_preceding_content: bool,
    paragraph_distance: u16,
) -> u16 {
    match node.macro_name.as_deref() {
        // man(7) uses the current `.PD` value, except before the first heading
        // at a level and after an empty peer section.
        Some("SH" | "SS") => {
            if has_preceding_content {
                paragraph_distance
            } else {
                0
            }
        }
        // mdoc(7) gives top-level sections one row even before the first Sh;
        // Ss only receives it when visible content precedes the heading.
        Some("Sh") => u16::from(is_first || has_preceding_content),
        Some("Ss") => u16::from(has_preceding_content),
        _ => 0,
    }
}

fn section_has_body(section: &Section) -> bool {
    !section.blocks.is_empty() || section.children.iter().any(section_has_body)
}

fn is_section(node: &Node, top_level: bool) -> bool {
    matches!(
        (node.macro_name.as_deref(), top_level),
        (Some("Sh" | "SH"), true) | (Some("Ss" | "SS"), false)
    )
}

fn lower_blocks(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
) -> Vec<Block> {
    let mut state = BlockState::new(indent_columns);

    for node in nodes {
        if node.macro_name.as_deref() == Some("PD") {
            update_paragraph_distance(node, paragraph_distance);
            continue;
        }
        if node.flags.no_print
            || node.kind == NodeKind::Comment
            || is_section(node, false)
            || is_nonprinting_request(node)
        {
            continue;
        }
        if node.kind == NodeKind::Text
            && node.text.as_deref().is_some_and(str::is_empty)
            && !node.flags.no_fill
        {
            state.flush_paragraph();
            state.output.push(Block::VerticalSpace {
                lines: 1,
                source: source_span(node),
            });
            continue;
        }
        if node.flags.no_fill && is_inline(node) {
            let line = lower_inline_nodes(std::slice::from_ref(node), context.default_name);
            if !line.is_empty() {
                state.push_preformatted(line, source_span(node));
            }
            continue;
        }
        state.flush_preformatted();
        if node.macro_name.as_deref() == Some("Pp") {
            state.flush_paragraph();
            if !state.output.is_empty() {
                state.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: source_span(node),
                });
            }
        } else if node.macro_name.as_deref() == Some("sp") {
            state.flush_paragraph();
            if let Some(lines) = vertical_distance_lines(node).filter(|lines| *lines > 0) {
                state.output.push(Block::VerticalSpace {
                    lines,
                    source: source_span(node),
                });
            }
        } else if node.macro_name.as_deref() == Some("br") {
            state.hard_break();
        } else if is_inline(node) {
            state.push_inline(
                lower_inline_nodes(std::slice::from_ref(node), context.default_name),
                source_span(node),
            );
        } else {
            state.flush_paragraph();
            lower_structural_node(
                node,
                context,
                indent_columns,
                paragraph_distance,
                &mut state.output,
            );
        }
    }
    state.finish()
}

fn lower_structural_node(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
    output: &mut Vec<Block>,
) {
    match node.macro_name.as_deref() {
        Some("PP" | "P" | "LP" | "HP") => {
            let spacing_before = if output.is_empty() {
                0
            } else {
                *paragraph_distance
            };
            let nested = lower_blocks(
                part_children(node, NodeKind::Body),
                context,
                indent_columns,
                paragraph_distance,
            );
            extend_blocks_with_spacing(output, nested, spacing_before);
        }
        Some("TP" | "IP") => {
            let spacing_before = *paragraph_distance;
            let item = definition_item(node, context, indent_columns, paragraph_distance);
            append_definition(
                output,
                item,
                indent_columns,
                spacing_before,
                source_span(node),
            );
        }
        Some("TQ") => {
            let item = definition_item(node, context, indent_columns, paragraph_distance);
            append_definition(output, item, indent_columns, 0, source_span(node));
        }
        Some("Bl") => {
            let mut block = lower_mdoc_list(node, context, indent_columns, paragraph_distance);
            if !output.is_empty() && !node.compact {
                set_block_spacing(&mut block, 1);
            }
            output.push(block);
        }
        Some("Bd") if node.display_kind == Some(DisplayKind::Filled) => {
            let spacing_before = u16::from(!output.is_empty() && !node.compact);
            let nested = lower_blocks(
                part_children(node, NodeKind::Body),
                context,
                indent_columns + display_indent(node),
                paragraph_distance,
            );
            extend_blocks_with_spacing(output, nested, spacing_before);
        }
        Some("Bd" | "D1" | "Dl") => {
            let mut block =
                preformatted_block(node, context, indent_columns + display_indent(node));
            if node.macro_name.as_deref() == Some("Bd") && !output.is_empty() && !node.compact {
                set_block_spacing(&mut block, 1);
            }
            output.push(block);
        }
        Some("RS") => {
            let nested = lower_blocks(
                part_children(node, NodeKind::Body),
                context,
                indent_columns + 4,
                paragraph_distance,
            );
            extend_transparent_blocks(output, nested, *paragraph_distance);
        }
        _ if node.kind == NodeKind::Table => {
            append_table_row(output, node, indent_columns);
        }
        _ if node.kind == NodeKind::Equation => {
            output.push(Block::Equation {
                value: node.equation.clone().unwrap_or_default(),
                display: true,
                layout: layout(indent_columns),
                source: source_span(node),
            });
        }
        _ => {
            let body = part_children(node, NodeKind::Body);
            let children = if body.is_empty() {
                node.children.as_slice()
            } else {
                body
            };
            output.extend(lower_blocks(
                children,
                context,
                indent_columns,
                paragraph_distance,
            ));
        }
    }
}

fn append_table_row(output: &mut Vec<Block>, node: &Node, indent_columns: u16) {
    if node.table_cells.is_empty() {
        return;
    }
    let row = TableRow {
        cells: node
            .table_cells
            .iter()
            .map(|cell| AstTableCell {
                blocks: vec![Block::Paragraph {
                    children: cell.text.as_deref().map_or_else(Vec::new, parse_roff_text),
                    layout: LayoutHint::default(),
                    source: source_span(node),
                }],
                column_span: cell.column_span,
                row_span: cell.row_span,
                alignment: Some(match cell.alignment {
                    MandocTableAlignment::Left => AstTableAlignment::Left,
                    MandocTableAlignment::Center => AstTableAlignment::Center,
                    MandocTableAlignment::Right => AstTableAlignment::Right,
                }),
            })
            .collect(),
    };
    if let Some(Block::Table { rows, .. }) = output.last_mut() {
        rows.push(row);
    } else {
        output.push(Block::Table {
            rows: vec![row],
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
}

struct BlockState {
    output: Vec<Block>,
    paragraph: InlineBuilder,
    paragraph_source: Option<mant_ast::SourceSpan>,
    paragraph_last_line: Option<u32>,
    preformatted: Vec<Inline>,
    pre_source: Option<mant_ast::SourceSpan>,
    indent_columns: u16,
}

impl BlockState {
    const fn new(indent_columns: u16) -> Self {
        Self {
            output: Vec::new(),
            paragraph: InlineBuilder::new(),
            paragraph_source: None,
            paragraph_last_line: None,
            preformatted: Vec::new(),
            pre_source: None,
            indent_columns,
        }
    }

    fn push_inline(&mut self, nodes: Vec<Inline>, source: Option<mant_ast::SourceSpan>) {
        if nodes.is_empty() {
            return;
        }
        if self.paragraph_source.is_none() {
            self.paragraph_source = source;
        }
        let source_line = source.map(|span| span.line);
        if self
            .paragraph_last_line
            .zip(source_line)
            .is_some_and(|(previous, current)| current > previous)
        {
            self.paragraph.append_across_source_line(nodes);
        } else {
            self.paragraph.append(nodes);
        }
        if source_line.is_some() {
            self.paragraph_last_line = source_line;
        }
    }

    fn hard_break(&mut self) {
        self.paragraph.hard_break();
    }

    fn push_preformatted(&mut self, nodes: Vec<Inline>, source: Option<mant_ast::SourceSpan>) {
        self.flush_paragraph();
        if !self.preformatted.is_empty() {
            self.preformatted.push(Inline::LineBreak);
        }
        self.preformatted.extend(nodes);
        if self.pre_source.is_none() {
            self.pre_source = source;
        }
    }

    fn flush_paragraph(&mut self) {
        flush_paragraph(
            &mut self.output,
            &mut self.paragraph,
            &mut self.paragraph_source,
            self.indent_columns,
        );
        self.paragraph_last_line = None;
    }

    fn flush_preformatted(&mut self) {
        flush_preformatted(
            &mut self.output,
            &mut self.preformatted,
            &mut self.pre_source,
            self.indent_columns,
        );
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush_preformatted();
        self.flush_paragraph();
        self.output
    }
}

fn lower_mdoc_list(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
) -> Block {
    let items: Vec<&Node> = part_children(node, NodeKind::Body)
        .iter()
        .filter(|child| child.macro_name.as_deref() == Some("It"))
        .collect();
    let is_definition = matches!(
        node.list_kind,
        Some(NormalizedListKind::Definition | NormalizedListKind::Column)
    ) || (node.list_kind.is_none()
        && items
            .iter()
            .any(|item| !part_children(item, NodeKind::Head).is_empty()));
    let list_indent = indent_columns + display_indent(node);
    if is_definition {
        Block::DefinitionList {
            items: items
                .into_iter()
                .map(|item| definition_item(item, context, list_indent, paragraph_distance))
                .collect(),
            compact: node.compact,
            layout: layout(indent_columns),
            source: source_span(node),
        }
    } else {
        Block::List {
            kind: match node.list_kind {
                Some(NormalizedListKind::Ordered) => ListKind::Ordered,
                Some(NormalizedListKind::Plain) => ListKind::Plain,
                _ => ListKind::Bullet,
            },
            start: (node.list_kind == Some(NormalizedListKind::Ordered)).then_some(1),
            compact: node.compact,
            items: items
                .into_iter()
                .map(|item| ListItem {
                    blocks: lower_blocks(
                        part_children(item, NodeKind::Body),
                        context,
                        list_indent,
                        paragraph_distance,
                    ),
                })
                .collect(),
            layout: layout(indent_columns),
            source: source_span(node),
        }
    }
}

fn definition_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
) -> DefinitionItem {
    let mut term = lower_inline_nodes(visible_definition_head(node), context.default_name);
    if let Some(id) = definition_head_anchor(node, &term) {
        term.insert(0, Inline::Anchor { id });
    }
    DefinitionItem {
        identity: None,
        terms: (!term.is_empty()).then_some(term).into_iter().collect(),
        description: lower_blocks(
            part_children(node, NodeKind::Body),
            context,
            indent_columns + 4,
            paragraph_distance,
        ),
        spacing_before_lines: None,
    }
}

/// Preserve libmandoc's tag on a man(7) `.TP`/`.IP` head. Unlike mdoc `Fl`
/// tags, this identity lives on the structural head rather than a visible
/// inline child, so it has to be copied before lowering discards that wrapper.
fn definition_head_anchor(node: &Node, term: &[Inline]) -> Option<String> {
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    if !head.flags.deep_link_target {
        return None;
    }
    head.tag.clone().or_else(|| {
        plain_text(term)
            .trim_start_matches('-')
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
    })
}

/// Return only document content from a definition macro's mixed-purpose head.
///
/// This follows mandoc's own HTML and terminal renderers: `.IP` prints its
/// first head node and treats later arguments as layout, while `.TP`/`.TQ`
/// print only nodes beginning on the following input line. The distinction is
/// structural; inspecting strings such as `96u` would incorrectly remove a
/// numeric term while still leaking non-numeric width expressions.
fn visible_definition_head(node: &Node) -> &[Node] {
    let head = part_children(node, NodeKind::Head);
    match node.macro_name.as_deref() {
        Some("IP") => head.first().map_or(&[], std::slice::from_ref),
        Some("TP" | "TQ") => head
            .iter()
            .position(|child| child.flags.line_start)
            .map_or(&[], |visible_start| &head[visible_start..]),
        _ => head,
    }
}

fn append_definition(
    output: &mut Vec<Block>,
    mut item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<mant_ast::SourceSpan>,
) {
    if let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    {
        if !item.description.is_empty() {
            let first_pending = items
                .iter()
                .rposition(|previous| !previous.description.is_empty())
                .map_or(0, |index| index + 1);
            for pending in items.drain(first_pending..) {
                item.terms.splice(0..0, pending.terms);
            }
        }
        item.spacing_before_lines = Some(if items.is_empty() {
            0
        } else {
            paragraph_distance
        });
        *compact = *compact && paragraph_distance == 0;
        items.push(item);
    } else {
        item.spacing_before_lines = Some(0);
        let spacing_before_lines = if output.is_empty() {
            0
        } else {
            paragraph_distance
        };
        output.push(Block::DefinitionList {
            items: vec![item],
            compact: paragraph_distance == 0,
            layout: layout_with_spacing(indent_columns, spacing_before_lines),
            source,
        });
    }
}

/// Appends blocks lowered through a transparent roff wrapper.
///
/// Sphinx emits each option as its own `.INDENT`/`.UNINDENT` pair.  mandoc
/// expands those macros to separate `RS` nodes, so lowering every node in
/// isolation would turn one option list into many one-item lists and lose the
/// default `.PD` distance between them.  Adjacent lists with identical layout
/// are one semantic list; joining them retains that distance on the boundary.
fn extend_transparent_blocks(
    output: &mut Vec<Block>,
    mut nested: Vec<Block>,
    paragraph_distance: u16,
) {
    // A transparent wrapper cannot decide whether its first child begins a
    // new paragraph in isolation. At section start there is no leading
    // distance; after visible outer content the current `.PD` value applies.
    // An explicit vertical-space node already owns the distance before the
    // wrapper's first visible child. Applying `.PD` as well would represent
    // the same roff gap twice; this is common in Sphinx's `.sp` + `.INDENT`
    // output and produces visibly doubled rows in both the TUI and text views.
    let boundary_spacing = if output.is_empty()
        || output
            .last()
            .is_some_and(|block| matches!(block, Block::VerticalSpace { .. }))
    {
        0
    } else {
        paragraph_distance
    };
    add_leading_spacing(&mut nested, boundary_spacing);
    let merged_first = match (output.last_mut(), nested.first_mut()) {
        (
            Some(Block::DefinitionList {
                items: previous_items,
                compact: previous_compact,
                layout: previous_layout,
                ..
            }),
            Some(Block::DefinitionList {
                items: nested_items,
                compact: nested_compact,
                layout: nested_layout,
                ..
            }),
        ) if previous_layout.indent_columns == nested_layout.indent_columns => {
            if let Some(first) = nested_items.first_mut() {
                first.spacing_before_lines = Some(if previous_items.is_empty() {
                    0
                } else {
                    nested_layout.spacing_before_lines
                });
            }
            previous_items.append(nested_items);
            *previous_compact = *previous_compact && *nested_compact && paragraph_distance == 0;
            true
        }
        _ => false,
    };

    if merged_first {
        nested.remove(0);
    }
    output.extend(nested);
}

/// Appends nested semantic blocks while attaching a macro's leading distance
/// to the first visible block rather than inventing renderer-side margins.
fn extend_blocks_with_spacing(output: &mut Vec<Block>, mut nested: Vec<Block>, lines: u16) {
    add_leading_spacing(&mut nested, lines);
    output.extend(nested);
}

fn add_leading_spacing(blocks: &mut [Block], lines: u16) {
    if lines == 0 {
        return;
    }
    let Some(first) = blocks.first_mut() else {
        return;
    };
    set_block_spacing(first, lines);
}

fn set_block_spacing(block: &mut Block, lines: u16) {
    if let Block::VerticalSpace {
        lines: existing, ..
    } = block
    {
        *existing = (*existing).max(lines);
    } else if let Some(layout) = block_layout_mut(block) {
        layout.spacing_before_lines = layout.spacing_before_lines.max(lines);
    }
}

fn block_layout_mut(block: &mut Block) -> Option<&mut LayoutHint> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout),
        Block::VerticalSpace { .. } => None,
    }
}

fn block_indent(block: &Block) -> Option<u16> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout.indent_columns),
        Block::VerticalSpace { .. } => None,
    }
}

fn preformatted_block(node: &Node, context: &LoweringContext<'_>, indent_columns: u16) -> Block {
    let children = part_children(node, NodeKind::Body);
    let children = if children.is_empty() {
        &node.children
    } else {
        children
    };
    Block::Preformatted {
        children: preformatted_inlines(children, context.default_name),
        language: None,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

fn preformatted_inlines(nodes: &[Node], default_name: Option<&str>) -> Vec<Inline> {
    let mut output = Vec::new();
    let mut previous_line = None;
    for node in nodes {
        if node.kind == NodeKind::Comment || node.flags.no_print {
            continue;
        }
        if previous_line.is_some_and(|line| node.line > line) && !output.is_empty() {
            output.push(Inline::LineBreak);
        }
        let children = if node.kind == NodeKind::Text || node.macro_name.is_some() {
            lower_inline_nodes(std::slice::from_ref(node), default_name)
        } else {
            preformatted_inlines(&node.children, default_name)
        };
        output.extend(children);
        previous_line = Some(node.line);
    }
    output
}

fn flush_paragraph(
    output: &mut Vec<Block>,
    paragraph: &mut InlineBuilder,
    source: &mut Option<mant_ast::SourceSpan>,
    indent_columns: u16,
) {
    let current = std::mem::replace(paragraph, InlineBuilder::new()).finish();
    if current.is_empty() {
        *source = None;
    } else {
        output.push(Block::Paragraph {
            children: current,
            layout: layout(indent_columns),
            source: source.take(),
        });
    }
}

fn flush_preformatted(
    output: &mut Vec<Block>,
    preformatted: &mut Vec<Inline>,
    source: &mut Option<mant_ast::SourceSpan>,
    indent_columns: u16,
) {
    if preformatted.is_empty() {
        *source = None;
        return;
    }
    output.push(Block::Preformatted {
        children: std::mem::take(preformatted),
        language: None,
        layout: layout(indent_columns),
        source: source.take(),
    });
}

fn is_inline(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Text | NodeKind::Element)
        || matches!(
            node.macro_name.as_deref(),
            Some("Nm" | "Nd" | "Op" | "Oo" | "Dq" | "Sq" | "Pq" | "Bq" | "Brq" | "Aq")
        )
}

fn is_nonprinting_request(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("ad" | "fi" | "ft" | "hy" | "in" | "na" | "ne" | "nf" | "nh" | "nr" | "ta")
    )
}

/// Convert a `.PD` measurement to terminal rows using mandoc's unit ratios.
/// Missing arguments restore man(7)'s one-row default; invalid values retain
/// the previous state.
fn paragraph_distance_lines(node: &Node) -> Option<u16> {
    let Some(argument) = first_text(node) else {
        return Some(1);
    };
    distance_lines(argument)
}

fn vertical_distance_lines(node: &Node) -> Option<u16> {
    first_text(node).map_or(Some(1), distance_lines)
}

fn distance_lines(argument: &str) -> Option<u16> {
    let argument = argument.trim();
    let number_end = argument
        .find(|character: char| character.is_ascii_alphabetic())
        .unwrap_or(argument.len());
    let scale = argument[..number_end].parse::<f64>().ok()?;
    if !scale.is_finite() {
        return None;
    }
    let unit = argument[number_end..].trim();
    let vertical_rows = match unit {
        "u" => scale / 40.0,
        "c" => scale * 6.0 / 2.54,
        "f" => scale * 65_536.0 / 40.0,
        "i" => scale * 6.0,
        "M" => scale * 0.006,
        "p" => scale / 12.0,
        "m" | "n" => scale * 0.6,
        // `P`, `v`, no suffix, and unknown suffixes retain the vertical scale.
        _ => scale,
    };

    // Equivalent to mandoc's positive rounding in term_vspan(), without a
    // lossy float cast, including its fallback for unusually large values.
    for lines in 0_u16..66 {
        if vertical_rows < f64::from(lines) + 0.5005 {
            return Some(lines);
        }
    }
    Some(1)
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

const fn layout(indent_columns: u16) -> LayoutHint {
    LayoutHint {
        indent_columns,
        spacing_before_lines: 0,
    }
}

const fn layout_with_spacing(indent_columns: u16, spacing_before_lines: u16) -> LayoutHint {
    LayoutHint {
        indent_columns,
        spacing_before_lines,
    }
}

fn display_indent(node: &Node) -> u16 {
    let Some(offset) = node.offset.as_deref() else {
        return 4;
    };
    if offset == "left" {
        return 0;
    }
    if offset == "indent" {
        return 4;
    }
    offset
        .trim_end_matches(|character: char| character.is_ascii_alphabetic())
        .parse()
        .unwrap_or(4)
}
