//! Batch AST-to-IR structure profiler for local roff audits.
//!
//! This development-only example accepts one JSON object per stdin line:
//!
//! `{ "id": "...", "path": "/.../git.1.gz", "root": "/usr/share/man" }`
//!
//! It parses the source twice on purpose. The first pass retains the owned
//! libmandoc AST as the structural expectation; the second uses the same
//! bounded, source-aware `ManualPage` path as indexed product queries. The
//! resulting JSON identifies likely topology loss without comparing terminal
//! wrapping or trusting a host reference renderer.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use flate2::read::GzDecoder;
use libmandoc_rs::{
    Compression, DisplayKind, IncludePolicy, Node, NodeKind, NormalizedListKind, ParseOptions,
    Parser, SpecialCharacter, special_character,
};
use mant_ir::{Block, Document, Inline, LinkTarget, Section};
use mant_loader::{ManualPage, parse_manual_page};
use serde::Serialize;
use serde_json::{Value, json};

#[path = "support/profile_io.rs"]
mod profile_io;

const PROFILE_SCHEMA: &str = "mant.roff-structure-profile/v4";

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstStructure {
    no_fill_lines: usize,
    literal_displays: usize,
    paragraph_boundaries: usize,
    generic_list_items: usize,
    definition_items: usize,
    table_rows: usize,
    table_spanning_cells: usize,
    max_relative_indent_depth: usize,
    positive_relative_indent_scopes: usize,
    hard_breaks: usize,
    manual_links: usize,
    external_links: usize,
    email_links: usize,
    section_links: usize,
    equation_configurations: usize,
    display_equations: usize,
    inline_equations: usize,
    table_equations: usize,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrStructure {
    preformatted_blocks: usize,
    preformatted_lines: usize,
    paragraph_blocks: usize,
    generic_list_items: usize,
    definition_items: usize,
    table_rows: usize,
    table_spanning_cells: usize,
    max_indent_columns: i32,
    hard_breaks: usize,
    manual_links: usize,
    external_links: usize,
    email_links: usize,
    section_links: usize,
    unresolved_section_references: usize,
    display_equations: usize,
    inline_equation_candidates: usize,
    table_equation_candidates: usize,
}

#[derive(Clone, Copy, Default)]
struct NoFillSourceLine {
    printable: bool,
    zero_width_blank: bool,
    continues_line: bool,
}

/// Source-addressable topology for semantic containers.  Counts catch broad
/// loss; these signatures catch a list or table retaining the right total in
/// the wrong parent or shape.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstTopology {
    lists: Vec<AstListTopology>,
    table_rows: Vec<AstTableRowTopology>,
    equations: Vec<AstEquationTopology>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrTopology {
    lists: Vec<IrListTopology>,
    table_rows: Vec<IrTableRowTopology>,
    equations: Vec<IrEquationTopology>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum EquationContext {
    Display,
    Inline,
    TableCell,
}

impl EquationContext {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Inline => "inline",
            Self::TableCell => "table-cell",
        }
    }
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstEquationTopology {
    source_line: u32,
    context: EquationContext,
    value: String,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrEquationTopology {
    source_line: u32,
    context: EquationContext,
    value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ListTopologyKind {
    Generic,
    Definition,
}

impl ListTopologyKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Definition => "definition",
        }
    }
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstListTopology {
    source_line: u32,
    kind: ListTopologyKind,
    items: usize,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrListTopology {
    source_line: u32,
    kind: ListTopologyKind,
    items: usize,
}

#[derive(Eq, PartialEq, Serialize)]
struct AstTableRowTopology {
    cells: Vec<AstTableCellTopology>,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstTableCellTopology {
    column_span: u16,
    row_span: u16,
    vertical_continuation: bool,
}

#[derive(Eq, PartialEq, Serialize)]
struct IrTableRowTopology {
    cells: Vec<IrTableCellTopology>,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrTableCellTopology {
    column_span: u16,
    row_span: u16,
    empty: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("roff_structure_profile: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    profile_io::run(PROFILE_SCHEMA, profile_request, true)
}

fn profile_request(line: &str) -> Result<Value, String> {
    let request: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "request.id must be a string".to_owned())?;
    let path = path_field(&request, "path")?;
    let root = path_field(&request, "root")?;

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(root.clone()),
        compression: Compression::Auto,
    })
    .parse_file(&path)
    .map_err(|error| error.to_string())?;
    let (mut expected, mut expected_topology) = ast_profile(&report.document.root);
    let source = read_source(&path)?;
    for (line, expression) in source_table_equations(&String::from_utf8_lossy(&source)) {
        let value = normalize_equation_fragment(&expression)?;
        if value.is_empty()
            || expected_topology.equations.iter().any(|equation| {
                equation.source_line == line
                    && equation.context == EquationContext::TableCell
                    && equation.value == value
            })
        {
            continue;
        }
        expected.table_equations += 1;
        expected_topology.equations.push(AstEquationTopology {
            source_line: line,
            context: EquationContext::TableCell,
            value,
        });
    }
    expected_topology.equations.sort_by_key(|equation| {
        (
            equation.source_line,
            equation_context_order(equation.context),
        )
    });
    let document = parse_manual_page(&ManualPage {
        name: "audit".to_owned(),
        section: "1".to_owned(),
        path,
        manual_root: root,
    })
    .map_err(|error| error.to_string())?;
    // The raw libmandoc pass follows `.so` through IncludePolicy but does not
    // own ManT's logical alias metadata.  Classify the source identity from
    // the normal indexed-page path, which is also the path whose IR we audit.
    let is_alias = document.meta.alias_target.is_some();
    let (observed, observed_topology) = ir_profile(&document);
    let violations = if is_alias {
        Vec::new()
    } else {
        compare_structure(&expected, &observed, &expected_topology, &observed_topology)
    };

    Ok(json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "expected": expected,
        "observed": observed,
        "topology": {
            "expected": expected_topology,
            "observed": observed_topology,
        },
        "diagnostics": {
            "parser": report.diagnostics.len(),
            "ir": document.diagnostics.len(),
        },
        "sourceLinkOrigins": {
            "manual": semantic_link_origins(&report.document.root, "Xr", NodeKind::Element),
            "externalMdoc": semantic_link_origins(&report.document.root, "Lk", NodeKind::Element),
            "externalMan": semantic_link_origins(&report.document.root, "UR", NodeKind::Block),
            "emailMdoc": semantic_link_origins(&report.document.root, "Mt", NodeKind::Element),
            "emailMan": semantic_link_origins(&report.document.root, "MT", NodeKind::Block),
            "section": semantic_link_origins(&report.document.root, "Sx", NodeKind::Element),
        },
        "alias": is_alias,
        "violations": violations,
    }))
}

fn path_field(request: &Value, field: &str) -> Result<PathBuf, String> {
    request
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("request.{field} must be a non-empty string"))
}

fn ast_profile(root: &Node) -> (AstStructure, AstTopology) {
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
                    || node.display_kind == Some(DisplayKind::Literal)
                        && node
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
    if node.kind == NodeKind::Table && !node.table_cells.is_empty() {
        profile.table_rows += 1;
        profile.table_spanning_cells += node
            .table_cells
            .iter()
            .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
            .count();
    }
    if node.kind == NodeKind::Equation {
        match node.equation.as_deref().map(str::trim) {
            None | Some("") => profile.equation_configurations += 1,
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
fn semantic_link_origins(node: &Node, macro_name: &str, kind: NodeKind) -> BTreeSet<(u32, u32)> {
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
fn is_no_fill_row_text(text: &str) -> bool {
    !text.is_empty() && !is_roff_font_switch(text)
}

fn is_printable_no_fill_text(text: &str) -> bool {
    is_no_fill_row_text(text) && !is_zero_width_guard_line(text)
}

fn is_zero_width_guard_line(text: &str) -> bool {
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
fn retained_no_fill_rows(lines: &BTreeMap<u32, NoFillSourceLine>) -> usize {
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
enum MdocContainerKind {
    Generic,
    Definition,
    Table,
}

fn mdoc_list_topology_kind(node: &Node) -> Option<MdocContainerKind> {
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
            NormalizedListKind::Bullet | NormalizedListKind::Ordered | NormalizedListKind::Plain,
        )
        | None => MdocContainerKind::Generic,
    })
}

fn ast_tag_is_bullet(node: &Node) -> bool {
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
            (!cells.is_empty()).then_some(AstTableRowTopology { cells })
        })
        .collect()
}

fn collect_ast_topology(node: &Node, inside_table: bool, topology: &mut AstTopology) {
    if node.kind == NodeKind::Equation
        && let Some(value) = node.equation.as_deref().map(str::trim)
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
            value: equation_visible_text(value),
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

    for child in &node.children {
        if child.kind == NodeKind::Table && !child.table_cells.is_empty() {
            topology.table_rows.push(AstTableRowTopology {
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

fn ir_profile(document: &Document) -> (IrStructure, IrTopology) {
    let mut profile = IrStructure {
        unresolved_section_references: document
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_deref() == Some("unresolved-section-reference"))
            .count(),
        ..IrStructure::default()
    };
    let mut topology = IrTopology::default();
    collect_blocks(&document.blocks, false, &mut profile, &mut topology);
    for section in &document.sections {
        collect_section(section, &mut profile, &mut topology);
    }
    topology.lists.sort_by_key(|list| list.source_line);
    topology.equations.sort_by_key(|equation| {
        (
            equation.source_line,
            equation_context_order(equation.context),
        )
    });
    (profile, topology)
}

fn collect_section(section: &Section, profile: &mut IrStructure, topology: &mut IrTopology) {
    collect_inlines(
        &section.heading.content,
        section.source.map_or(0, |source| source.line),
        false,
        profile,
        topology,
    );
    collect_blocks(&section.blocks, false, profile, topology);
    for child in &section.children {
        collect_section(child, profile, topology);
    }
}

#[allow(clippy::too_many_lines)]
fn collect_blocks(
    blocks: &[Block],
    inside_table: bool,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    for block in blocks {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                profile.paragraph_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                collect_inlines(
                    children,
                    source_line(block),
                    inside_table,
                    profile,
                    topology,
                );
            }
            Block::Unsupported { layout, .. } => {
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
            }
            Block::Equation {
                value,
                display,
                layout,
                source,
            } => {
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if *display {
                    profile.display_equations += 1;
                    topology.equations.push(IrEquationTopology {
                        source_line: source.map_or(0, |span| span.line),
                        context: EquationContext::Display,
                        value: value.clone(),
                    });
                }
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                profile.preformatted_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if has_visible_inline(children) {
                    profile.preformatted_lines += 1;
                }
                profile.preformatted_lines += line_break_count(children);
                collect_inlines(
                    children,
                    source_line(block),
                    inside_table,
                    profile,
                    topology,
                );
            }
            Block::List {
                items,
                layout,
                source,
                ..
            } => {
                profile.generic_list_items += items.len();
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if !items.is_empty() {
                    profile.max_indent_columns = profile.max_indent_columns.max(1);
                }
                if let Some(source) = source {
                    topology.lists.push(IrListTopology {
                        source_line: source.line,
                        kind: ListTopologyKind::Generic,
                        items: items.len(),
                    });
                }
                for item in items {
                    collect_blocks(&item.blocks, inside_table, profile, topology);
                }
            }
            Block::DefinitionList {
                items,
                layout,
                source,
                ..
            } => {
                profile.definition_items += items.len();
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if !items.is_empty() {
                    profile.max_indent_columns = profile.max_indent_columns.max(1);
                }
                if let Some(source) = source {
                    topology.lists.push(IrListTopology {
                        source_line: source.line,
                        kind: ListTopologyKind::Definition,
                        items: items.len(),
                    });
                }
                for item in items {
                    for term in &item.terms {
                        collect_inlines(term, 0, inside_table, profile, topology);
                    }
                    collect_blocks(&item.description, inside_table, profile, topology);
                }
            }
            Block::Table { rows, layout, .. } => {
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                profile.table_rows += rows.len();
                topology.table_rows.extend(rows.iter().map(|row| {
                    IrTableRowTopology {
                        cells: row
                            .cells
                            .iter()
                            .map(|cell| IrTableCellTopology {
                                column_span: cell.column_span,
                                row_span: cell.row_span,
                                empty: cell.blocks.is_empty(),
                            })
                            .collect(),
                    }
                }));
                for row in rows {
                    profile.table_spanning_cells += row
                        .cells
                        .iter()
                        .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
                        .count();
                    for cell in &row.cells {
                        collect_blocks(&cell.blocks, true, profile, topology);
                    }
                }
            }
            Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => {}
        }
    }
}

fn has_visible_inline(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text { value } | Inline::Code { value } => !value.is_empty(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_visible_inline(children),
        Inline::Anchor { .. } | Inline::LineBreak => false,
    })
}

fn collect_inlines(
    inlines: &[Inline],
    source_line: u32,
    inside_table: bool,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    for inline in inlines {
        match inline {
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect_inlines(children, source_line, inside_table, profile, topology);
            }
            Inline::Link {
                target, children, ..
            } => {
                match target {
                    LinkTarget::Manual { .. } => profile.manual_links += 1,
                    LinkTarget::External { .. } => profile.external_links += 1,
                    LinkTarget::Email { .. } => profile.email_links += 1,
                    LinkTarget::Section { .. } => profile.section_links += 1,
                    LinkTarget::Document { .. } => {}
                }
                collect_inlines(children, source_line, inside_table, profile, topology);
            }
            Inline::LineBreak => profile.hard_breaks += 1,
            Inline::Code { value } => {
                if inside_table {
                    profile.table_equation_candidates += 1;
                } else {
                    profile.inline_equation_candidates += 1;
                }
                topology.equations.push(IrEquationTopology {
                    source_line,
                    context: if inside_table {
                        EquationContext::TableCell
                    } else {
                        EquationContext::Inline
                    },
                    value: value.clone(),
                });
            }
            Inline::Text { .. } | Inline::Anchor { .. } => {}
        }
    }
}

fn line_break_count(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => line_break_count(children),
            Inline::LineBreak => 1,
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => 0,
        })
        .sum()
}

fn compare_structure(
    expected: &AstStructure,
    observed: &IrStructure,
    expected_topology: &AstTopology,
    observed_topology: &IrTopology,
) -> Vec<String> {
    let mut violations = Vec::new();
    underflow(
        &mut violations,
        "no-fill-lines",
        expected.no_fill_lines,
        observed.preformatted_lines,
    );
    exact(
        &mut violations,
        "display-equations",
        expected.display_equations,
        observed.display_equations,
    );
    underflow(
        &mut violations,
        "literal-displays",
        expected.literal_displays,
        observed.preformatted_lines,
    );
    underflow(
        &mut violations,
        "generic-list-items",
        expected.generic_list_items,
        observed.generic_list_items,
    );
    underflow(
        &mut violations,
        "table-rows",
        expected.table_rows,
        observed.table_rows,
    );
    underflow(
        &mut violations,
        "table-spanning-cells",
        expected.table_spanning_cells,
        observed.table_spanning_cells,
    );
    underflow(
        &mut violations,
        "manual-links",
        expected.manual_links,
        observed.manual_links,
    );
    underflow(
        &mut violations,
        "external-links",
        expected.external_links,
        observed.external_links,
    );
    underflow(
        &mut violations,
        "email-links",
        expected.email_links,
        observed.email_links,
    );
    underflow(
        &mut violations,
        "section-links-or-diagnostics",
        expected.section_links,
        observed
            .section_links
            .saturating_add(observed.unresolved_section_references),
    );
    if expected.positive_relative_indent_scopes > 0 && observed.max_indent_columns == 0 {
        violations.push(format!(
            "relative-indent: expected positive indentation from {} RS scopes (nested depth {}), observed no indented IR block",
            expected.positive_relative_indent_scopes, expected.max_relative_indent_depth
        ));
    }
    compare_list_topology(
        &mut violations,
        &expected_topology.lists,
        &observed_topology.lists,
    );
    compare_table_topology(
        &mut violations,
        &expected_topology.table_rows,
        &observed_topology.table_rows,
    );
    compare_equation_topology(
        &mut violations,
        &expected_topology.equations,
        &observed_topology.equations,
    );
    violations
}

fn compare_equation_topology(
    violations: &mut Vec<String>,
    expected: &[AstEquationTopology],
    observed: &[IrEquationTopology],
) {
    let mut used = vec![false; observed.len()];
    for equation in expected {
        let candidate = observed.iter().enumerate().position(|(index, candidate)| {
            !used[index]
                && candidate.context == equation.context
                && candidate.value == equation.value
                && (equation.context != EquationContext::Display
                    || candidate.source_line == equation.source_line)
        });
        if let Some(index) = candidate {
            used[index] = true;
            continue;
        }
        violations.push(format!(
            "{}-equation at line {}: expected normalized value {:?}, observed no matching IR value",
            equation.context.as_str(),
            equation.source_line,
            equation.value,
        ));
    }
}

const fn equation_context_order(context: EquationContext) -> u8 {
    match context {
        EquationContext::Display => 0,
        EquationContext::Inline => 1,
        EquationContext::TableCell => 2,
    }
}

fn source_line(block: &Block) -> u32 {
    match block {
        Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::VerticalSpace { source, .. }
        | Block::ThematicBreak { source, .. }
        | Block::Unsupported { source, .. } => source.map_or(0, |span| span.line),
    }
}

fn equation_visible_text(source: &str) -> String {
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

fn read_source(path: &Path) -> Result<Vec<u8>, String> {
    let extension = path.extension().and_then(|extension| extension.to_str());
    if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("gz")) {
        let source = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut decoder = GzDecoder::new(source);
        let mut output = Vec::new();
        decoder
            .read_to_end(&mut output)
            .map_err(|error| error.to_string())?;
        return Ok(output);
    }
    if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("zst")) {
        let source = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut decoder =
            zstd::stream::read::Decoder::new(source).map_err(|error| error.to_string())?;
        let mut output = Vec::new();
        decoder
            .read_to_end(&mut output)
            .map_err(|error| error.to_string())?;
        return Ok(output);
    }
    if extension.is_some_and(|extension| {
        extension.eq_ignore_ascii_case("xz") || extension.eq_ignore_ascii_case("bz2")
    }) {
        let program = if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("xz")) {
            "xz"
        } else {
            "bzip2"
        };
        let output = Command::new(program).args(["-dc", "--"]).arg(path).output();
        return decoded_command_source(program, output);
    }
    fs::read(path).map_err(|error| error.to_string())
}

fn decoded_command_source(
    program: &str,
    result: std::io::Result<std::process::Output>,
) -> Result<Vec<u8>, String> {
    let output = result.map_err(|error| format!("source decoder {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "source decoder {program} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(512)
                .collect::<String>()
        ));
    }
    Ok(output.stdout)
}

#[derive(Clone, Copy)]
enum EquationDelimiters {
    Enabled(char, char),
    Disabled,
}

fn source_table_equations(source: &str) -> Vec<(u32, String)> {
    let mut output = Vec::new();
    let mut inside_equation = false;
    let mut pending = None;
    let mut active = None;
    let mut inside_table = false;

    for (index, line) in source.lines().enumerate() {
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(".EQ")
            && rest.chars().next().is_none_or(char::is_whitespace)
        {
            inside_equation = true;
            pending = parse_equation_delimiters(rest.trim());
            continue;
        }
        if inside_equation {
            if trimmed == ".EN" || trimmed.starts_with(".EN ") {
                if let Some(change) = pending.take() {
                    active = match change {
                        EquationDelimiters::Enabled(opening, closing) => Some((opening, closing)),
                        EquationDelimiters::Disabled => None,
                    };
                }
                inside_equation = false;
            } else if let Some(change) = parse_equation_delimiters(trimmed) {
                pending = Some(change);
            }
            continue;
        }
        if trimmed == ".TS" || trimmed.starts_with(".TS ") {
            inside_table = true;
            continue;
        }
        if trimmed == ".TE" || trimmed.starts_with(".TE ") {
            inside_table = false;
            continue;
        }
        if !inside_table || trimmed.starts_with('.') {
            continue;
        }
        if let Some((opening, closing)) = active {
            output.extend(
                delimited_expressions(line, opening, closing)
                    .into_iter()
                    .map(|expression| (line_number, expression)),
            );
        }
    }
    output
}

fn parse_equation_delimiters(value: &str) -> Option<EquationDelimiters> {
    let value = value.strip_prefix("delim")?.trim_start();
    if value == "off" {
        return Some(EquationDelimiters::Disabled);
    }
    let mut delimiters = value.chars();
    let opening = delimiters.next()?;
    let closing = delimiters.next()?;
    Some(EquationDelimiters::Enabled(opening, closing))
}

fn delimited_expressions(source: &str, opening: char, closing: char) -> Vec<String> {
    let mut output = Vec::new();
    let mut remainder = source;
    while let Some(opening_index) = remainder.find(opening) {
        let after_opening = &remainder[opening_index + opening.len_utf8()..];
        let Some(closing_index) = after_opening.find(closing) else {
            break;
        };
        let expression = after_opening[..closing_index].trim();
        if !expression.is_empty() {
            output.push(expression.to_owned());
        }
        remainder = &after_opening[closing_index + closing.len_utf8()..];
    }
    output
}

fn normalize_equation_fragment(source: &str) -> Result<String, String> {
    let synthetic = format!(".TH AUDIT 7\n.EQ\n{source}\n.EN\n");
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes("audit-equation.7", synthetic.as_bytes())
    .map_err(|error| error.to_string())?;
    find_equation(&report.document.root)
        .map(equation_visible_text)
        .ok_or_else(|| format!("could not normalize table equation {source:?}"))
}

fn find_equation(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Equation
        && let Some(value) = node.equation.as_deref()
        && !value.trim().is_empty()
    {
        return Some(value.trim());
    }
    node.children.iter().find_map(find_equation)
}

fn exact(violations: &mut Vec<String>, label: &str, expected: usize, observed: usize) {
    if expected != observed {
        violations.push(format!("{label}: expected {expected}, observed {observed}"));
    }
}

fn compare_list_topology(
    violations: &mut Vec<String>,
    expected: &[AstListTopology],
    observed: &[IrListTopology],
) {
    for expected_list in expected {
        let observed_list = observed.iter().find(|candidate| {
            candidate.source_line == expected_list.source_line
                && candidate.kind == expected_list.kind
        });
        match observed_list {
            Some(observed_list) if observed_list.items == expected_list.items => {}
            Some(observed_list) => violations.push(format!(
                "list-topology at line {}: expected {} {} items, observed {}",
                expected_list.source_line,
                expected_list.items,
                expected_list.kind.as_str(),
                observed_list.items,
            )),
            None => violations.push(format!(
                "list-topology at line {}: expected {} list with {} items, observed none",
                expected_list.source_line,
                expected_list.kind.as_str(),
                expected_list.items,
            )),
        }
    }
}

fn compare_table_topology(
    violations: &mut Vec<String>,
    expected: &[AstTableRowTopology],
    observed: &[IrTableRowTopology],
) {
    if expected.len() != observed.len() {
        violations.push(format!(
            "table-topology: expected {} rows, observed {}",
            expected.len(),
            observed.len(),
        ));
    }
    for (row_index, (expected_row, observed_row)) in expected.iter().zip(observed).enumerate() {
        if observed_row.cells.len() < expected_row.cells.len() {
            violations.push(format!(
                "table-topology at row {}: expected at least {} cells, observed {}",
                row_index + 1,
                expected_row.cells.len(),
                observed_row.cells.len(),
            ));
        }
        for (cell_index, (expected_cell, observed_cell)) in expected_row
            .cells
            .iter()
            .zip(&observed_row.cells)
            .enumerate()
        {
            if expected_cell.column_span != observed_cell.column_span
                || expected_cell.row_span != observed_cell.row_span
            {
                violations.push(format!(
                    "table-topology at row {}, cell {}: expected span {}x{}, observed {}x{}",
                    row_index + 1,
                    cell_index + 1,
                    expected_cell.column_span,
                    expected_cell.row_span,
                    observed_cell.column_span,
                    observed_cell.row_span,
                ));
            }
            if expected_cell.vertical_continuation && !observed_cell.empty {
                violations.push(format!(
                    "table-topology at row {}, cell {}: vertical continuation retained visible content",
                    row_index + 1,
                    cell_index + 1,
                ));
            }
        }
    }
}

fn underflow(violations: &mut Vec<String>, property: &str, expected: usize, observed: usize) {
    if expected > observed {
        violations.push(format!(
            "{property}: expected at least {expected}, observed {observed}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        NoFillSourceLine, equation_visible_text, is_no_fill_row_text, is_zero_width_guard_line,
        retained_no_fill_rows,
    };

    fn parsed_structure(
        source: &str,
    ) -> (super::AstStructure, super::AstTopology, super::Document) {
        let report = super::Parser::new(super::ParseOptions {
            includes: super::IncludePolicy::Deny,
            compression: super::Compression::Plain,
        })
        .parse_bytes("audit.1", source.as_bytes())
        .unwrap();
        let (expected, topology) = super::ast_profile(&report.document.root);
        let document =
            mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes())
                .unwrap();
        (expected, topology, document)
    }

    #[test]
    fn section_heading_links_are_counted_and_loss_is_detected() {
        let (expected, expected_topology, mut document) = parsed_structure(concat!(
            ".Dd September 11, 2026\n.Dt AUDIT 1\n.Os\n",
            ".Sh NAME\n.Nm audit\n.Nd heading link probe\n",
            ".Sh DESCRIPTION\n.Ss Eo\n.Xr printf 3\nReference\n.Ec\n.Pp\nVisible body.\n",
        ));
        let (observed, topology) = super::ir_profile(&document);
        assert_eq!(expected.manual_links, 1);
        assert_eq!(observed.manual_links, 1);
        assert!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology)
                .is_empty()
        );

        let heading = &mut document.sections[1].children[0].heading;
        // Preserve the words, but remove the typed link: topology must notice.
        heading.content = vec![super::Inline::Text {
            value: heading.plain_text(),
        }];
        let (mutated, topology) = super::ir_profile(&document);
        let violations =
            super::compare_structure(&expected, &mutated, &expected_topology, &topology);
        assert!(
            violations
                .iter()
                .any(|item| item.starts_with("manual-links:"))
        );
    }

    #[test]
    fn nonpositive_rs_literals_do_not_require_positive_layout() {
        for argument in ["-7", "-2n", "-1.5m", "0", "+0n"] {
            let (expected, expected_topology, document) = parsed_structure(&format!(
                ".TH AUDIT 1\n.SH BODY\n.RS {argument}\n.PP\nVisible body.\n.RE\n"
            ));
            assert_eq!(expected.max_relative_indent_depth, 1, "{argument}");
            assert_eq!(expected.positive_relative_indent_scopes, 0, "{argument}");
            let (observed, topology) = super::ir_profile(&document);
            assert_eq!(observed.max_indent_columns, 0, "{argument}");
            assert!(
                super::compare_structure(&expected, &observed, &expected_topology, &topology)
                    .is_empty()
            );
        }
    }

    #[test]
    fn positive_rs_loss_is_detected_even_below_a_negative_scope() {
        for scopes in [".RS 4\n", ".RS\n", ".RS -2\n.RS 4\n"] {
            let (expected, expected_topology, mut document) = parsed_structure(&format!(
                ".TH AUDIT 1\n.SH BODY\n{scopes}.PP\nVisible body.\n.RE\n"
            ));
            assert_eq!(expected.positive_relative_indent_scopes, 1, "{scopes}");
            let (observed, topology) = super::ir_profile(&document);
            assert!(observed.max_indent_columns > 0, "{scopes}");
            assert!(
                super::compare_structure(&expected, &observed, &expected_topology, &topology)
                    .is_empty()
            );
            for block in &mut document.sections[0].blocks {
                if let super::Block::Paragraph { layout, .. } = block {
                    layout.indent_columns = 0;
                }
            }
            let (mutated, topology) = super::ir_profile(&document);
            assert_eq!(mutated.max_indent_columns, 0);
            let violations =
                super::compare_structure(&expected, &mutated, &expected_topology, &topology);
            assert!(
                violations
                    .iter()
                    .any(|item| item.starts_with("relative-indent:")),
                "{scopes}"
            );
        }
    }

    #[test]
    fn unsupported_rs_arguments_retain_the_conservative_obligation() {
        for argument in ["-2q", "-1i", "-999999n", "-2+1", "NaN"] {
            let (expected, _, _) = parsed_structure(&format!(
                ".TH AUDIT 1\n.SH BODY\n.RS {argument}\n.PP\nVisible body.\n.RE\n"
            ));
            assert_eq!(expected.positive_relative_indent_scopes, 1, "{argument}");
        }
    }

    #[test]
    fn punctuated_mdoc_definition_ordinals_compare_as_recovered_ordered_lists() {
        let (expected, topology, document) = parsed_structure(concat!(
            ".Dd September 12, 2026\n.Dt AUDIT 1\n.Os\n.Sh BODY\n",
            ".Bl -tag -width Ds\n.It 1.\nFirst.\n.It 2.\nSecond.\n.El\n"
        ));
        let (observed, observed_topology) = super::ir_profile(&document);
        assert_eq!(topology.lists[0].kind, super::ListTopologyKind::Generic);
        assert!(
            super::compare_structure(&expected, &observed, &topology, &observed_topology)
                .is_empty()
        );
    }

    #[test]
    fn unavailable_source_decoder_is_not_absent_equation_evidence() {
        let error = super::decoded_command_source(
            "xz",
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing decoder",
            )),
        )
        .unwrap_err();
        assert!(error.contains("xz"));
        assert!(error.contains("missing decoder"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_source_decoder_is_not_successful_empty_input() {
        use std::os::unix::process::ExitStatusExt;
        let output = |code| std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: b"invalid compressed data".to_vec(),
        };
        assert!(super::decoded_command_source("bzip2", Ok(output(1))).is_err());
        assert_eq!(
            super::decoded_command_source("bzip2", Ok(output(0))).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn ip_list_obligations_require_complete_authored_named_bullets() {
        let source = concat!(
            ".TH AUDIT 1\n.SH BODY\n",
            ".IP *\nStar key.\n.IP o\nLetter key.\n.IP +\nPlus key.\n",
            ".IP \\(bu\nBullet.\n.IP \\fB\\[bu]\\fR\nStyled bullet.\n",
            ".IP \\(buSuffix\nNot just a bullet.\n",
            ".IP \\(bu 4\nIndented bullet.\n.TP\n\\(bu\nTagged bullet.\n",
        );
        let parsed = super::Parser::new(super::ParseOptions {
            includes: super::IncludePolicy::Deny,
            compression: super::Compression::Plain,
        })
        .parse_bytes("audit.1", source.as_bytes())
        .unwrap();
        let (expected, _) = super::ast_profile(&parsed.document.root);
        assert_eq!(expected.generic_list_items, 4);
        let mut violations = Vec::new();
        super::underflow(
            &mut violations,
            "generic-list-items",
            expected.generic_list_items,
            0,
        );
        assert_eq!(violations.len(), 1, "losing named bullets must still fail");
    }

    #[test]
    fn standalone_roff_font_switches_do_not_claim_visible_no_fill_lines() {
        assert!(!is_no_fill_row_text(r"\f[C]"));
        assert!(!is_no_fill_row_text(r"\fR"));
        assert!(!is_no_fill_row_text(r"    \f(CW"));
        assert!(!is_no_fill_row_text(r"\f[B]\f[R]\f[B]"));
        assert!(is_no_fill_row_text(r"\f[C]visible\f[R]"));
        assert!(is_no_fill_row_text("visible"));
    }

    #[test]
    fn bounded_zero_width_guard_runs_claim_one_no_fill_row() {
        assert!(is_zero_width_guard_line(r"\&"));
        assert!(is_zero_width_guard_line(r"\& \&"));
        assert!(!is_zero_width_guard_line(r"\&\c"));
        let mut lines = BTreeMap::new();
        lines.insert(
            10,
            NoFillSourceLine {
                printable: true,
                ..NoFillSourceLine::default()
            },
        );
        lines.insert(
            11,
            NoFillSourceLine {
                zero_width_blank: true,
                ..NoFillSourceLine::default()
            },
        );
        lines.insert(
            12,
            NoFillSourceLine {
                zero_width_blank: true,
                ..NoFillSourceLine::default()
            },
        );
        lines.insert(
            13,
            NoFillSourceLine {
                printable: true,
                ..NoFillSourceLine::default()
            },
        );
        lines.insert(
            14,
            NoFillSourceLine {
                zero_width_blank: true,
                ..NoFillSourceLine::default()
            },
        );
        assert_eq!(retained_no_fill_rows(&lines), 3);
    }

    #[test]
    fn equation_text_normalizes_bracketed_and_two_character_specials() {
        assert_eq!(
            equation_visible_text(r"1 + \(lf x \(rf \[->] infinity"),
            "1 + ⌊ x ⌋ → infinity"
        );
    }
}
