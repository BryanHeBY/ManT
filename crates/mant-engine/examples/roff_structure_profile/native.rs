//! Owned native observations and source topology.
use super::{
    AstEquationTopology, AstListTopology, AstStructure, AstTableCellTopology, AstTableRowTopology,
    AstTopology, BTreeMap, BTreeSet, DisplayKind, EquationContext, ListTopologyKind,
    NoFillSourceLine, Node, NodeKind, NormalizedListKind, SpecialCharacter, equation_context_order,
    special_character,
};

pub(super) fn ast_profile(root: &Node) -> (AstStructure, AstTopology) {
    let mut profile = AstStructure::default();
    let mut no_fill_lines = BTreeMap::new();
    collect_ast_structure(root, false, false, 0, &mut profile, &mut no_fill_lines);
    profile.no_fill_lines = retained_no_fill_rows(&no_fill_lines);
    profile.manual_links = semantic_link_origins(root, "Xr", NodeKind::Element).len();
    profile.external_links = semantic_link_origins(root, "Lk", NodeKind::Element).len()
        + semantic_link_origins(root, "UR", NodeKind::Block).len();
    profile.email_links = semantic_link_origins(root, "Mt", NodeKind::Element).len()
        + semantic_link_origins(root, "MT", NodeKind::Block).len();
    profile.section_links = semantic_link_origins(root, "Sx", NodeKind::Element).len();
    let mut topology = AstTopology::default();
    collect_ast_topology(root, false, &mut topology);
    topology.lists.sort_by_key(|list| list.source_line);
    topology.equations.sort_by_key(|equation| {
        (
            equation.source_line,
            equation_context_order(equation.context),
        )
    });
    (profile, topology)
}

fn collect_ast_structure(
    node: &Node,
    inherited_nonprinting: bool,
    inside_table: bool,
    relative_indent_depth: usize,
    profile: &mut AstStructure,
    no_fill_lines: &mut BTreeMap<u32, NoFillSourceLine>,
) {
    let nonprinting = inherited_nonprinting || node.flags.no_print || is_stateful_request(node);
    if node.flags.no_fill
        && !nonprinting
        && node.kind == NodeKind::Text
        && node.flags.line_start
        && node.text.as_deref().is_some_and(is_no_fill_row_text)
        && node.line > 0
    {
        let text = node.text.as_deref().unwrap_or_default();
        let line = no_fill_lines.entry(node.line).or_default();
        line.zero_width_blank |= is_zero_width_guard_line(text);
        line.printable |= is_printable_no_fill_text(text);
        line.continues_line |= node.flags.line_continuation;
    }
    if node.kind == NodeKind::Block {
        match node.macro_name.as_deref() {
            Some("Bd" | "D1" | "Dl")
                if node.macro_name.as_deref() != Some("Bd")
                    || matches!(
                        node.display_kind,
                        Some(DisplayKind::Literal | DisplayKind::Unfilled)
                    ) && node
                        .children
                        .iter()
                        .filter(|part| part.kind == NodeKind::Body)
                        .any(has_visible_text) =>
            {
                profile.literal_displays += 1;
            }
            Some("PP" | "P" | "LP" | "HP") if has_visible_flow_text(node) => {
                profile.paragraph_boundaries += 1;
            }
            Some("Bl") => match mdoc_list_topology_kind(node) {
                Some(MdocContainerKind::Definition) => {
                    profile.definition_items += direct_list_item_count(node);
                }
                Some(MdocContainerKind::Table) => {
                    profile.table_rows += mdoc_column_rows(node).len();
                }
                Some(MdocContainerKind::Generic) | None => {
                    profile.generic_list_items += direct_list_item_count(node);
                }
            },
            Some("IP" | "TP") if ast_tag_is_bullet(node) => profile.generic_list_items += 1,
            // `.TQ` only adds an alias to the next described `.TP` item.  It
            // intentionally has no standalone IR item.
            Some("TP" | "IP") if has_visible_definition_description(node) => {
                profile.definition_items += 1;
            }
            _ => {}
        }
    }
    if node.macro_name.as_deref() == Some("br") && node.kind == NodeKind::Element {
        profile.hard_breaks += 1;
    }
    if node.kind == NodeKind::Table && node.table_row_kind.is_some() {
        profile.table_rows += 1;
        profile.table_spanning_cells += node
            .table_cells
            .iter()
            .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
            .count();
    }
    if node.kind == NodeKind::Equation {
        match node
            .equation
            .as_ref()
            .map(libmandoc_rs::EquationBox::readable_text)
        {
            None => profile.equation_configurations += 1,
            Some(value) if value.trim().is_empty() => profile.equation_configurations += 1,
            Some(_) if inside_table => profile.table_equations += 1,
            Some(_) if node.flags.line_start => profile.display_equations += 1,
            Some(_) => profile.inline_equations += 1,
        }
    }
    let child_inside_table = inside_table || node.kind == NodeKind::Table;
    let visible_rs = node.kind == NodeKind::Block
        && node.macro_name.as_deref() == Some("RS")
        && node_part_children(node, NodeKind::Body)
            .iter()
            .any(has_visible_text);
    let child_indent_depth = relative_indent_depth + usize::from(visible_rs);
    if visible_rs && !rs_has_nonpositive_literal_offset(node) {
        profile.positive_relative_indent_scopes += 1;
    }
    profile.max_relative_indent_depth = profile.max_relative_indent_depth.max(child_indent_depth);
    for child in &node.children {
        collect_ast_structure(
            child,
            nonprinting,
            child_inside_table,
            child_indent_depth,
            profile,
            no_fill_lines,
        );
    }
}

/// A signed RS offset is not necessarily an indentation to the right.
/// Native `man_term.c` `pre_RS` adds the signed distance and clamps at page left.
/// Recognize only bounded, finite character-unit literals here: expressions,
/// other units and missing arguments retain the conservative legacy obligation.
/// This is not a replacement formatter or a cumulative-position oracle.
fn rs_has_nonpositive_literal_offset(node: &Node) -> bool {
    let Some(argument) = node_part_children(node, NodeKind::Head)
        .first()
        .and_then(|head| head.text.as_deref())
    else {
        return false;
    };
    let number = argument
        .strip_suffix('n')
        .or_else(|| argument.strip_suffix('m'))
        .unwrap_or(argument);
    if !number
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.'))
    {
        return false;
    }
    number
        .parse::<f64>()
        .is_ok_and(|value| value.is_finite() && (-4096.0..=0.0).contains(&value))
}

/// Return unique, printable source occurrences for one semantic link macro.
///
/// libmandoc can expose more than one structural view of one macro occurrence,
/// and malformed empty closers such as a bare `.MT` have no target at all.
/// Source coordinates keep the audit focused on links the lowering path can
/// actually be expected to preserve.
pub(super) fn semantic_link_origins(
    node: &Node,
    macro_name: &str,
    kind: NodeKind,
) -> BTreeSet<(u32, u32)> {
    let mut origins = BTreeSet::new();
    collect_semantic_link_origins(node, macro_name, kind, &mut origins);
    origins
}

fn collect_semantic_link_origins(
    node: &Node,
    macro_name: &str,
    kind: NodeKind,
    origins: &mut BTreeSet<(u32, u32)>,
) {
    let has_target = if kind == NodeKind::Block {
        node_part_children(node, NodeKind::Head)
            .iter()
            .any(has_visible_text)
    } else {
        has_visible_text(node)
    };
    if node.kind == kind
        && node.macro_name.as_deref() == Some(macro_name)
        && node.line > 0
        && !node.flags.generated
        && has_target
    {
        origins.insert((node.line, node.column));
    }
    for child in &node.children {
        collect_semantic_link_origins(child, macro_name, kind, origins);
    }
}

fn node_part_children(node: &Node, kind: NodeKind) -> &[Node] {
    node.children
        .iter()
        .find(|part| part.kind == kind)
        .map_or(&[], |part| part.children.as_slice())
}

/// libmandoc keeps a source-line node for a standalone `\f` font switch in a
/// no-fill display. The switch has no printable glyph and therefore cannot
/// demand a `LineBreak` in `ManT` IR.
pub(super) fn is_no_fill_row_text(text: &str) -> bool {
    !text.is_empty() && !is_roff_font_switch(text)
}

fn is_printable_no_fill_text(text: &str) -> bool {
    is_no_fill_row_text(text) && !is_zero_width_guard_line(text)
}

pub(super) fn is_zero_width_guard_line(text: &str) -> bool {
    let mut remainder = text.trim();
    let mut found = false;
    while let Some(rest) = remainder.strip_prefix(r"\&") {
        found = true;
        remainder = rest.trim();
    }
    found && remainder.is_empty()
}

/// Count printable no-fill rows plus bounded runs of zero-width blank rows.
///
/// A terminal-visible `\&` row matters only between printable rows in the
/// same source run. A trailing guard immediately before `.Ve` or `.fi` merely
/// separates blocks and must not manufacture content. Consecutive guard rows
/// collapse to one visual separator, matching the lowering contract.
pub(super) fn retained_no_fill_rows(lines: &BTreeMap<u32, NoFillSourceLine>) -> usize {
    let printable = lines
        .values()
        .filter(|line| line.printable && !line.continues_line)
        .count();
    let mut blank_runs = 0;
    let ordered = lines.iter().collect::<Vec<_>>();
    let mut index = 0;
    while index < ordered.len() {
        if !ordered[index].1.zero_width_blank || ordered[index].1.printable {
            index += 1;
            continue;
        }
        let start = index;
        while index + 1 < ordered.len()
            && ordered[index + 1].0 == &ordered[index].0.saturating_add(1)
            && ordered[index + 1].1.zero_width_blank
            && !ordered[index + 1].1.printable
        {
            index += 1;
        }
        let end = index;
        let bounded_before = start > 0
            && ordered[start - 1].0.saturating_add(1) == *ordered[start].0
            && ordered[start - 1].1.printable;
        let bounded_after = end + 1 < ordered.len()
            && ordered[end].0.saturating_add(1) == *ordered[end + 1].0
            && ordered[end + 1].1.printable;
        blank_runs += usize::from(bounded_before && bounded_after);
        index += 1;
    }
    printable + blank_runs
}

fn is_roff_font_switch(text: &str) -> bool {
    let mut remainder = text.trim();
    let mut found = false;
    while let Some(font) = remainder.strip_prefix(r"\f") {
        let consumed = if font.starts_with('(') {
            font.char_indices()
                .nth(3)
                .map_or(font.len(), |(index, _)| index)
        } else if font.starts_with('[') {
            let Some(end) = font.find(']') else {
                return false;
            };
            end + 1
        } else if font.is_empty() {
            return false;
        } else {
            font.char_indices()
                .nth(1)
                .map_or(font.len(), |(index, _)| index)
        };
        found = true;
        remainder = font[consumed..].trim();
    }
    found && remainder.is_empty()
}

fn is_stateful_request(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some(
            "Es" | "Sm"
                | "PD"
                | "ad"
                | "fi"
                | "ft"
                | "hy"
                | "in"
                | "na"
                | "ne"
                | "nf"
                | "nh"
                | "nr"
                | "ta"
                | "ti"
        )
    )
}

fn direct_list_item_count(node: &Node) -> usize {
    node.children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .flat_map(|body| &body.children)
        .filter(|child| child.macro_name.as_deref() == Some("It"))
        .count()
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum MdocContainerKind {
    Generic,
    Definition,
    Table,
}

pub(super) fn mdoc_list_topology_kind(node: &Node) -> Option<MdocContainerKind> {
    if node.macro_name.as_deref() != Some("Bl") {
        return None;
    }
    Some(match node.list_kind {
        Some(NormalizedListKind::Column) => MdocContainerKind::Table,
        Some(NormalizedListKind::Definition) => MdocContainerKind::Definition,
        None if node.children.iter().any(|child| {
            child.kind == NodeKind::Body
                && child.children.iter().any(|item| {
                    item.macro_name.as_deref() == Some("It")
                        && item
                            .children
                            .iter()
                            .any(|part| part.kind == NodeKind::Head && !part.children.is_empty())
                })
        }) =>
        {
            MdocContainerKind::Definition
        }
        Some(
            NormalizedListKind::Bullet
            | NormalizedListKind::Dash
            | NormalizedListKind::Ordered
            | NormalizedListKind::Plain,
        )
        | None => MdocContainerKind::Generic,
    })
}

pub(super) fn ast_tag_is_bullet(node: &Node) -> bool {
    let Some(head) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)
    else {
        return false;
    };
    // man(7) prints an IP tag literally (CVS pre_IP / groff an.tmac).
    // Only the authored named bullet is a list obligation; punctuation and
    // editor keys such as `*` and `o` must not become fabricated expectations.
    // IP's remaining head children are layout operands, not tag text.
    let tag = if node.macro_name.as_deref() == Some("IP") {
        let Some(tag) = head.children.first() else {
            return false;
        };
        tag
    } else {
        head
    };
    let term = strip_equation_font_escapes(&ast_visible_text(tag));
    matches!(term.trim(), r"\[bu]" | r"\(bu")
}

fn has_visible_definition_description(node: &Node) -> bool {
    node.children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .any(has_visible_text)
}

fn has_visible_text(node: &Node) -> bool {
    !node.flags.no_print
        && node.kind != NodeKind::Comment
        && (node.text.as_deref().is_some_and(|text| !text.is_empty())
            || node.children.iter().any(has_visible_text))
}

fn ast_visible_text(node: &Node) -> String {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return String::new();
    }
    let mut text = node.text.clone().unwrap_or_default();
    for child in &node.children {
        text.push_str(&ast_visible_text(child));
    }
    text
}

fn has_visible_flow_text(node: &Node) -> bool {
    node.children.iter().any(|child| {
        !child.flags.no_print
            && child.kind != NodeKind::Comment
            && child.kind != NodeKind::Block
            && (child.text.as_deref().is_some_and(|text| !text.is_empty())
                || has_visible_flow_text(child))
    })
}

fn mdoc_column_rows(node: &Node) -> Vec<AstTableRowTopology> {
    node.children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .flat_map(|body| &body.children)
        .filter(|item| item.macro_name.as_deref() == Some("It"))
        .filter_map(|item| {
            let cells = item
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Body)
                .map(|_| AstTableCellTopology {
                    column_span: 1,
                    row_span: 1,
                    vertical_continuation: false,
                })
                .collect::<Vec<_>>();
            (!cells.is_empty()).then_some(AstTableRowTopology {
                table_source_line: node.line,
                table_source_column: node.column,
                row_index: 0,
                kind: mant_ir::TableRowKind::Data,
                cells,
            })
        })
        .enumerate()
        .map(|(index, mut row)| {
            row.row_index = index;
            row
        })
        .collect()
}

fn collect_ast_topology(node: &Node, inside_table: bool, topology: &mut AstTopology) {
    if node.kind == NodeKind::Equation
        && let Some(value) = node
            .equation
            .as_ref()
            .map(libmandoc_rs::EquationBox::readable_text)
        && !value.is_empty()
    {
        topology.equations.push(AstEquationTopology {
            source_line: node.line,
            context: if inside_table {
                EquationContext::TableCell
            } else if node.flags.line_start {
                EquationContext::Display
            } else {
                EquationContext::Inline
            },
            value: equation_visible_text(&value),
        });
    }
    if node.kind == NodeKind::Block
        && mdoc_list_topology_kind(node) == Some(MdocContainerKind::Table)
        && node.line > 0
    {
        topology.table_rows.extend(mdoc_column_rows(node));
    } else if node.kind == NodeKind::Block
        && let Some(kind) = mdoc_list_topology_kind(node)
        && node.line > 0
    {
        let kind = if kind == MdocContainerKind::Definition
            && mdoc_definition_is_recoverable_ordinal_list(node)
        {
            // Lowering deliberately recovers explicit `1.`, `2.` tag heads
            // as an ordered generic list. The audit must compare semantic
            // topology, not the implementation-private native list subtype.
            ListTopologyKind::Generic
        } else {
            match kind {
                MdocContainerKind::Generic => ListTopologyKind::Generic,
                MdocContainerKind::Definition => ListTopologyKind::Definition,
                MdocContainerKind::Table => unreachable!("column lists are tables"),
            }
        };
        topology.lists.push(AstListTopology {
            source_line: node.line,
            kind,
            items: direct_list_item_count(node),
        });
    }

    let mut table_origin = None;
    let mut row_index = 0;
    for child in &node.children {
        if child.kind == NodeKind::Table
            && let Some(kind) = native_table_kind(child)
        {
            if child.flags.table_start || table_origin.is_none() {
                table_origin = Some((child.line, child.column));
                row_index = 0;
            }
            let (table_source_line, table_source_column) = table_origin.expect("native tbl start");
            topology.table_rows.push(AstTableRowTopology {
                table_source_line,
                table_source_column,
                row_index,
                kind,
                cells: child
                    .table_cells
                    .iter()
                    .map(|cell| AstTableCellTopology {
                        column_span: cell.column_span,
                        row_span: cell.row_span,
                        vertical_continuation: cell.vertical_continuation,
                    })
                    .collect(),
            });
            row_index += 1;
            collect_ast_topology(child, true, topology);
            continue;
        }
        collect_ast_topology(
            child,
            inside_table || node.kind == NodeKind::Table,
            topology,
        );
    }
}

fn mdoc_definition_is_recoverable_ordinal_list(node: &Node) -> bool {
    let mut heads = node
        .children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .flat_map(|body| &body.children)
        .filter(|item| item.macro_name.as_deref() == Some("It"))
        .filter_map(|item| {
            item.children
                .iter()
                .find(|part| part.kind == NodeKind::Head)
        });
    let mut previous = None;
    let mut count = 0usize;
    for head in &mut heads {
        let text = head
            .children
            .iter()
            .filter_map(|child| child.text.as_deref())
            .collect::<String>();
        let Some(number) = text
            .trim()
            .strip_suffix('.')
            .and_then(|value| value.parse::<u32>().ok())
        else {
            return false;
        };
        if previous.is_some_and(|prior| number != prior + 1) {
            return false;
        }
        previous = Some(number);
        count += 1;
    }
    count > 0
}

pub(super) fn equation_visible_text(source: &str) -> String {
    let source = strip_equation_font_escapes(source);
    let mut output = String::with_capacity(source.len());
    let mut rest = source.as_str();
    while let Some(index) = rest.find('\\') {
        output.push_str(&rest[..index]);
        let escape = &rest[index + 1..];
        let (name, consumed) = if let Some(after_open) = escape.strip_prefix('[') {
            let Some(end) = after_open.find(']') else {
                output.push_str(&rest[index..]);
                return output;
            };
            (&after_open[..end], end + 3)
        } else if let Some(after_open) = escape.strip_prefix('(') {
            let mut characters = after_open.char_indices();
            let Some((_, _)) = characters.next() else {
                output.push_str(&rest[index..]);
                return output;
            };
            let consumed_name = characters
                .next()
                .map_or(after_open.len(), |(offset, character)| {
                    offset + character.len_utf8()
                });
            (&after_open[..consumed_name], consumed_name + 2)
        } else {
            output.push('\\');
            rest = escape;
            continue;
        };
        match special_character(name) {
            Some(SpecialCharacter::Visible(character)) => output.push(character),
            Some(SpecialCharacter::ZeroWidth) => {}
            None => output.push_str(&rest[index..index + consumed]),
        }
        rest = &rest[index + consumed..];
    }
    output.push_str(rest);
    output
}

fn strip_equation_font_escapes(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(index) = rest.find("\\f") {
        output.push_str(&rest[..index]);
        let operand = &rest[index + 2..];
        if let Some(bracketed) = operand.strip_prefix('[') {
            let Some(end) = bracketed.find(']') else {
                output.push_str(&rest[index..]);
                return output;
            };
            rest = &bracketed[end + 1..];
        } else if let Some(character) = operand.chars().next() {
            rest = &operand[character.len_utf8()..];
        } else {
            break;
        }
    }
    output.push_str(rest);
    output
}

fn native_table_kind(node: &Node) -> Option<mant_ir::TableRowKind> {
    use libmandoc_rs::{TableRowKind, TableRuleCellKind};
    node.table_row_kind.as_ref().map(|kind| match kind {
        TableRowKind::Data => mant_ir::TableRowKind::Data,
        TableRowKind::HorizontalRule => mant_ir::TableRowKind::HorizontalRule,
        TableRowKind::DoubleHorizontalRule => mant_ir::TableRowKind::DoubleHorizontalRule,
        TableRowKind::LayoutRule { cells } => mant_ir::TableRowKind::LayoutRule {
            cells: cells
                .iter()
                .map(|cell| match cell {
                    TableRuleCellKind::Horizontal => mant_ir::TableRuleCellKind::Horizontal,
                    TableRuleCellKind::DoubleHorizontal => {
                        mant_ir::TableRuleCellKind::DoubleHorizontal
                    }
                })
                .collect(),
        },
    })
}
