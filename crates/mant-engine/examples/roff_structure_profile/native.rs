//! Owned native observations and source topology.
use super::{
    AstStructure, AstTopology, BTreeMap, BTreeSet, NoFillSourceLine, Node, NodeKind,
    SpecialCharacter,
    head_payloads::HeadPayloads,
    native_topology::{
        MdocContainerKind, column_data_items, equation_context, mdoc_list_items,
        mdoc_list_topology_kind,
    },
    ordinal_labels::{
        SourceContext, SourceDestination,
        controls::{SourceControl, source_control},
        description::word_payload,
        skips_source_children,
    },
    special_character,
};

pub(super) fn ast_profile(report: &libmandoc_rs::ParseReport) -> (AstStructure, AstTopology) {
    let root = &report.document.root;
    let mut profile = AstStructure {
        source_context: SourceContext::collect(root),
        ..AstStructure::default()
    };
    let mut no_fill_lines = BTreeMap::new();
    super::literal_owners::record_source_displays(root, &mut profile);
    let head_payloads = HeadPayloads::collect(root, &profile.source_context);
    collect_ast_structure(root, false, false, 0, &mut profile, &mut no_fill_lines);
    profile.no_fill_lines = retained_no_fill_rows(&no_fill_lines);
    super::links::native_link_obligations(report, &mut profile);
    let mut topology = super::native_topology::collect(root, &profile.source_context);
    topology.head_payloads = head_payloads;
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
    let Some(destination) = profile.source_context.destination(node) else {
        return;
    };
    let blocks = destination == SourceDestination::Blocks;
    let nonprinting = inherited_nonprinting || is_stateful_request(node);
    if blocks && !nonprinting {
        record_no_fill_line(node, no_fill_lines);
    }
    if blocks && node.kind == NodeKind::Block {
        match node.macro_token.as_deref() {
            Some("PP" | "P" | "LP" | "HP") if has_visible_flow_text(node) => {
                profile.paragraph_boundaries += 1;
            }
            Some("Bl") => match mdoc_list_topology_kind(node) {
                Some(MdocContainerKind::Definition) => {
                    profile.definition_items += mdoc_list_items(node).count();
                }
                Some(MdocContainerKind::Table) => {
                    profile.table_rows += column_data_items(node).count();
                }
                Some(MdocContainerKind::Generic) | None => {
                    profile.generic_list_items += mdoc_list_items(node).count();
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
    if node.macro_token.as_deref() == Some("br") && node.kind == NodeKind::Element {
        profile.hard_breaks += 1;
    }
    if blocks && node.kind == NodeKind::Table && node.table_row_kind.is_some() {
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
            .filter(|equation| equation_has_payload(equation))
        {
            None => profile.equation_configurations += 1,
            Some(_) => match equation_context(node, inside_table, !blocks) {
                super::EquationContext::Display => profile.display_equations += 1,
                super::EquationContext::Inline => profile.inline_equations += 1,
                super::EquationContext::TableCell => profile.table_equations += 1,
            },
        }
    }
    let child_inside_table =
        inside_table || blocks && super::native_topology::is_table_container(node);
    let visible_rs = node.kind == NodeKind::Block
        && node.macro_token.as_deref() == Some("RS")
        && node_part_children(node, NodeKind::Body)
            .iter()
            .any(has_visible_text);
    let child_indent_depth = relative_indent_depth + usize::from(visible_rs);
    if visible_rs && !rs_has_nonpositive_literal_offset(node) {
        profile.positive_relative_indent_scopes += 1;
    }
    profile.max_relative_indent_depth = profile.max_relative_indent_depth.max(child_indent_depth);
    let count_children = usize::from(source_control(node) == Some(SourceControl::AlignedRows));
    for child in node.children.iter().skip(count_children) {
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

fn record_no_fill_line(node: &Node, lines: &mut BTreeMap<u32, NoFillSourceLine>) {
    if !node.flags.no_fill
        || node.kind != NodeKind::Text
        || !node.flags.line_start
        || node.line == 0
    {
        return;
    }
    let Some(text) = node.text.as_deref().filter(|text| !text.is_empty()) else {
        return;
    };
    let word = mant_codec::RoffWordEvidence::read(text);
    let payload = word_payload(&word, true);
    if !payload.has_text() && !payload.has_row() {
        return;
    }
    let continues = node.flags.line_continuation || word.has(mant_codec::RoffWordEffect::Continues);
    let line = lines.entry(node.line).or_default();
    line.zero_width_blank |= payload.has_row() && !payload.has_text() && !continues;
    line.printable |= payload.has_text();
    line.continues_line |= continues;
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
    visit_semantic_link_nodes(node, macro_name, kind, &mut |node| {
        origins.insert((node.line, node.column));
    });
    origins
}

fn visit_semantic_link_nodes<'a>(
    node: &'a Node,
    macro_name: &str,
    kind: NodeKind,
    visit: &mut impl FnMut(&'a Node),
) {
    let has_target = if kind == NodeKind::Block {
        node_part_children(node, NodeKind::Head)
            .iter()
            .any(has_visible_text)
    } else {
        has_visible_text(node)
    };
    if node.kind == kind
        && node.macro_token.as_deref() == Some(macro_name)
        && node.line > 0
        && !node.flags.generated
        && has_target
    {
        visit(node);
    }
    for child in &node.children {
        visit_semantic_link_nodes(child, macro_name, kind, visit);
    }
}

pub(super) fn node_part_children(node: &Node, kind: NodeKind) -> &[Node] {
    node.children
        .iter()
        .find(|part| part.kind == kind)
        .map_or(&[], |part| part.children.as_slice())
}

/// Count printable no-fill rows plus bounded runs of glyphless blank rows.
///
/// Blank rows only contribute to this literal-line census between printable
/// rows in the same source run. A standalone or trailing row may be IR
/// vertical space instead of preformatted text.
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

fn is_stateful_request(node: &Node) -> bool {
    source_control(node).is_some_and(|control| control != SourceControl::AlignedRows)
        || matches!(
            node.macro_token.as_deref(),
            Some("ad" | "hy" | "na" | "ne" | "nh" | "nr")
        )
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
    let tag = if node.macro_token.as_deref() == Some("IP") {
        let Some(tag) = head.children.first() else {
            return false;
        };
        tag
    } else {
        head
    };
    let term = strip_tag_font_escapes(&ast_visible_text(tag));
    matches!(term.trim(), r"\[bu]" | r"\(bu")
}

fn has_visible_definition_description(node: &Node) -> bool {
    node.children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .any(has_visible_text)
}

fn has_visible_text(node: &Node) -> bool {
    !skips_source_children(node)
        && (node.text.as_deref().is_some_and(|text| !text.is_empty())
            || node.children.iter().any(has_visible_text))
}

fn ast_visible_text(node: &Node) -> String {
    if skips_source_children(node) {
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
        !skips_source_children(child)
            && child.kind != NodeKind::Block
            && (child.text.as_deref().is_some_and(|text| !text.is_empty())
                || has_visible_flow_text(child))
    })
}

pub(super) fn equation_visible_text(source: &str) -> String {
    let word = mant_codec::RoffWordEvidence::read(source);
    if !word.has(mant_codec::RoffWordEffect::Unclassified) {
        // Glyphless native words (including ASCII_NBRZW) stay part of the
        // authored expression's owner while adding no reading scalar.
        return word.text;
    }
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(index) = rest.find('\\') {
        output.push_str(&rest[..index]);
        let escape = &rest[index + 1..];
        if let Some(after_backslash) = escape.strip_prefix('\\') {
            output.push('\\');
            rest = after_backslash;
            continue;
        }
        if let Some(after_breakpoint) = escape.strip_prefix(':') {
            // term_word buffers ASCII_BREAK for the direct form. It adds no
            // visible character to the source-neutral reading projection.
            rest = after_breakpoint;
            continue;
        }
        if let Some(operand) = escape.strip_prefix('f') {
            rest = if let Some(bracketed) = operand.strip_prefix('[') {
                let Some(end) = bracketed.find(']') else {
                    output.push_str(&rest[index..]);
                    return output;
                };
                &bracketed[end + 1..]
            } else if let Some(two_character) = operand.strip_prefix('(') {
                let consumed = two_character.chars().take(2).map(char::len_utf8).sum();
                &two_character[consumed..]
            } else {
                operand
                    .chars()
                    .next()
                    .map_or("", |character| &operand[character.len_utf8()..])
            };
            continue;
        }
        let (name, consumed) = if let Some(after_open) = escape.strip_prefix('[') {
            let Some(end) = after_open.find(']') else {
                output.push_str(&rest[index..]);
                return output;
            };
            let name = &after_open[..end];
            let consumed = end + 3;
            // roff_escape.c accepts '-' as the only bracketed one-byte name.
            if name.len() == 1 && name != "-" {
                rest = &rest[index + consumed..];
                continue;
            }
            (name, consumed)
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
        } else if matches!(
            escape.chars().next(),
            Some(' ' | '\'' | '-' | '0' | '_' | '`' | 'e' | '~')
        ) {
            // roff_escape.c's argument-free ESCAPE_SPECIAL cases share the
            // same chars.c catalog with bracketed and two-character forms.
            (&escape[..1], 2)
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

pub(super) fn equation_has_payload(equation: &libmandoc_rs::EquationBox) -> bool {
    // A configuration leaves the native root empty. Empty authored atoms or
    // pile rows still own an expression and must not disappear from the census.
    equation.text.is_some() || !equation.children.is_empty()
}

pub(super) fn strip_tag_font_escapes(source: &str) -> String {
    // Bullet obligations retain the authored named-character spelling.
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
