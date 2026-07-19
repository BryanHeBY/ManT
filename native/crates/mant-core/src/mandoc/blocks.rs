//! Reconstructs sections and semantic blocks from the copied mandoc tree.

use mant_ast::{
    Block, DefinitionItem, Inline, LayoutHint, ListItem, ListKind, Section,
    TableAlignment as AstTableAlignment, TableCell as AstTableCell, TableRow,
};
use mant_mandoc_sys::{
    DisplayKind, Node, NodeKind, NormalizedListKind, TableAlignment as MandocTableAlignment,
};

use super::{
    LoweringContext,
    inline::{InlineBuilder, lower_inline_nodes, parse_roff_text, plain_text},
    part_children, source_span,
};

pub(super) fn lower_sections(root: &Node, context: &mut LoweringContext<'_>) -> Vec<Section> {
    root.children
        .iter()
        .filter(|node| is_section(node, true))
        .map(|node| lower_section(node, context))
        .collect()
}

fn lower_section(node: &Node, context: &mut LoweringContext<'_>) -> Section {
    let heading = lower_inline_nodes(part_children(node, NodeKind::Head), context.default_name);
    let title = plain_text(&heading).trim().to_owned();
    // Allocate IDs in visible document order. Besides being deterministic for
    // consumers, this makes `.Sx` resolution independent of tree recursion.
    let id = context.section_id(&title);
    let body = part_children(node, NodeKind::Body);
    let children = body
        .iter()
        .filter(|child| is_section(child, false))
        .map(|child| lower_section(child, context))
        .collect();
    let mut paragraph_distance = 1;
    Section {
        id,
        title,
        blocks: lower_blocks(body, context, 0, &mut paragraph_distance),
        children,
        source: source_span(node),
    }
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
            if let Some(lines) = paragraph_distance_lines(node) {
                *paragraph_distance = lines;
            }
            continue;
        }
        if node.flags.no_print
            || node.kind == NodeKind::Comment
            || is_section(node, false)
            || is_nonprinting_request(node)
        {
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
        } else if node.macro_name.as_deref() == Some("sp") {
            state.flush_paragraph();
            state.output.push(Block::VerticalSpace {
                lines: 1,
                source: source_span(node),
            });
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
        Some("PP" | "P" | "LP") => output.extend(lower_blocks(
            part_children(node, NodeKind::Body),
            context,
            indent_columns,
            paragraph_distance,
        )),
        Some("TP") => {
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
        Some("IP") => {
            let spacing_before = *paragraph_distance;
            let item = indented_paragraph_item(node, context, indent_columns, paragraph_distance);
            append_definition(
                output,
                item,
                indent_columns,
                spacing_before,
                source_span(node),
            );
        }
        Some("Bl") => output.push(lower_mdoc_list(
            node,
            context,
            indent_columns,
            paragraph_distance,
        )),
        Some("Bd") if node.display_kind == Some(DisplayKind::Filled) => {
            output.extend(lower_blocks(
                part_children(node, NodeKind::Body),
                context,
                indent_columns + display_indent(node),
                paragraph_distance,
            ));
        }
        Some("Bd" | "D1" | "Dl") => {
            output.push(preformatted_block(
                node,
                context,
                indent_columns + display_indent(node),
            ));
        }
        Some("RS") => output.extend(lower_blocks(
            part_children(node, NodeKind::Body),
            context,
            indent_columns + 4,
            paragraph_distance,
        )),
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
            preformatted: Vec::new(),
            pre_source: None,
            indent_columns,
        }
    }

    fn push_inline(&mut self, nodes: Vec<Inline>, source: Option<mant_ast::SourceSpan>) {
        if self.paragraph_source.is_none() {
            self.paragraph_source = source;
        }
        self.paragraph.append(nodes);
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
    let term = lower_inline_nodes(part_children(node, NodeKind::Head), context.default_name);
    DefinitionItem {
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

fn indented_paragraph_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
) -> DefinitionItem {
    let head = part_children(node, NodeKind::Head);
    let term_nodes = head
        .last()
        .filter(|_| head.len() > 1)
        .filter(|node| node.text.as_deref().is_some_and(is_roff_measurement))
        .map_or(head, |_| &head[..head.len() - 1]);
    let term = lower_inline_nodes(term_nodes, context.default_name);
    DefinitionItem {
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

fn append_definition(
    output: &mut Vec<Block>,
    mut item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<mant_ast::SourceSpan>,
) {
    if let Some(Block::DefinitionList { items, compact, .. }) = output.last_mut() {
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
        output.push(Block::DefinitionList {
            items: vec![item],
            compact: paragraph_distance == 0,
            layout: layout(indent_columns),
            source,
        });
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
        if let Some(line) = previous_line
            && node.line > line
            && !output.is_empty()
        {
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

fn is_roff_measurement(value: &str) -> bool {
    let value = value.trim();
    let numeric = value
        .trim_start_matches(['+', '-'])
        .trim_end_matches(|character: char| character.is_ascii_alphabetic());
    !numeric.is_empty()
        && numeric
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        && numeric.chars().any(|character| character.is_ascii_digit())
}

const fn layout(indent_columns: u16) -> LayoutHint {
    LayoutHint { indent_columns }
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
