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
    collections::BTreeMap,
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use libmandoc_rs::{
    Compression, DisplayKind, IncludePolicy, Node, NodeKind, NormalizedListKind, ParseOptions,
    Parser,
};
use mant_engine::{ManualPage, parse_manual_page};
use mant_ir::{Block, Document, Inline, LinkTarget, Section};
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-structure-profile/v2";

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
    hard_breaks: usize,
    manual_links: usize,
    external_links: usize,
    email_links: usize,
    section_links: usize,
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
    max_indent_columns: u16,
    hard_breaks: usize,
    manual_links: usize,
    external_links: usize,
    email_links: usize,
    section_links: usize,
}

/// Source-addressable topology for semantic containers.  Counts catch broad
/// loss; these signatures catch a list or table retaining the right total in
/// the wrong parent or shape.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstTopology {
    lists: Vec<AstListTopology>,
    table_rows: Vec<AstTableRowTopology>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrTopology {
    lists: Vec<IrListTopology>,
    table_rows: Vec<IrTableRowTopology>,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
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
    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());
    for (index, line) in stdin.lock().lines().enumerate() {
        let line = line.map_err(|error| format!("read request {}: {error}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = profile_request(&line).unwrap_or_else(|error| {
            json!({
                "schema": PROFILE_SCHEMA,
                "id": request_id(&line),
                "error": error,
            })
        });
        serde_json::to_writer(&mut stdout, &response)
            .map_err(|error| format!("encode response {}: {error}", index + 1))?;
        stdout
            .write_all(b"\n")
            .map_err(|error| format!("write response {}: {error}", index + 1))?;
        stdout
            .flush()
            .map_err(|error| format!("flush response {}: {error}", index + 1))?;
    }
    stdout
        .flush()
        .map_err(|error| format!("flush stdout: {error}"))
}

fn request_id(line: &str) -> Value {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null)
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
    let (expected, expected_topology) = ast_profile(&report.document.root);
    let document = parse_manual_page(&ManualPage {
        name: "audit".to_owned(),
        section: "1".to_owned(),
        path,
        manual_root: root,
    })
    .map_err(|error| error.to_string())?;
    let (observed, observed_topology) = ir_profile(&document);
    let violations =
        compare_structure(&expected, &observed, &expected_topology, &observed_topology);

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
    collect_ast_structure(root, false, 0, &mut profile, &mut no_fill_lines);
    profile.no_fill_lines = no_fill_lines
        .values()
        .filter(|continues_line| !**continues_line)
        .count();
    let mut topology = AstTopology::default();
    collect_ast_topology(root, &mut topology);
    topology.lists.sort_by_key(|list| list.source_line);
    (profile, topology)
}

fn collect_ast_structure(
    node: &Node,
    inherited_nonprinting: bool,
    relative_indent_depth: usize,
    profile: &mut AstStructure,
    no_fill_lines: &mut BTreeMap<u32, bool>,
) {
    let nonprinting = inherited_nonprinting || node.flags.no_print || is_stateful_request(node);
    if node.flags.no_fill
        && !nonprinting
        && node.kind == NodeKind::Text
        && node.flags.line_start
        && node.text.as_deref().is_some_and(is_visible_no_fill_text)
        && node.line > 0
    {
        no_fill_lines
            .entry(node.line)
            .and_modify(|continues_line| *continues_line |= node.flags.line_continuation)
            .or_insert(node.flags.line_continuation);
    }
    if node.kind == NodeKind::Block {
        match node.macro_name.as_deref() {
            Some("Bd" | "D1" | "Dl")
                if node.macro_name.as_deref() != Some("Bd")
                    || node.display_kind == Some(DisplayKind::Literal) =>
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
            Some("IP") if ast_ip_is_bullet(node) => profile.generic_list_items += 1,
            // `.TQ` only adds an alias to the next described `.TP` item.  It
            // intentionally has no standalone IR item.
            Some("TP" | "IP") if has_visible_definition_description(node) => {
                profile.definition_items += 1;
            }
            _ => {}
        }
    }
    match node.macro_name.as_deref() {
        Some("br") if node.kind == NodeKind::Element => profile.hard_breaks += 1,
        Some("Xr" | "Lk" | "Mt" | "Sx") if node.kind == NodeKind::Element => {
            match node.macro_name.as_deref() {
                Some("Xr") => profile.manual_links += 1,
                Some("Lk") => profile.external_links += 1,
                Some("Mt") => profile.email_links += 1,
                Some("Sx") => profile.section_links += 1,
                _ => unreachable!("guarded semantic link macro"),
            }
        }
        Some("UR") if node.kind == NodeKind::Block => profile.external_links += 1,
        Some("MT") if node.kind == NodeKind::Block => profile.email_links += 1,
        _ => {}
    }
    if node.kind == NodeKind::Table && !node.table_cells.is_empty() {
        profile.table_rows += 1;
        profile.table_spanning_cells += node
            .table_cells
            .iter()
            .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
            .count();
    }
    let child_indent_depth = relative_indent_depth
        + usize::from(node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("RS"));
    profile.max_relative_indent_depth = profile.max_relative_indent_depth.max(child_indent_depth);
    for child in &node.children {
        collect_ast_structure(
            child,
            nonprinting,
            child_indent_depth,
            profile,
            no_fill_lines,
        );
    }
}

/// libmandoc keeps a source-line node for a standalone `\f` font switch in a
/// no-fill display. The switch has no printable glyph and therefore cannot
/// demand a `LineBreak` in `ManT` IR.
fn is_visible_no_fill_text(text: &str) -> bool {
    !text.is_empty() && !is_roff_font_switch(text)
}

fn is_roff_font_switch(text: &str) -> bool {
    let Some(font) = text.strip_prefix(r"\f") else {
        return false;
    };
    font.len() == 1 || (font.starts_with('[') && font.ends_with(']') && !font.contains(r"\f"))
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

fn ast_ip_is_bullet(node: &Node) -> bool {
    let Some(head) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)
    else {
        return false;
    };
    let Some(term) = head.children.first() else {
        return false;
    };
    is_bullet_glyph(ast_visible_text(term).trim())
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

fn is_bullet_glyph(value: &str) -> bool {
    matches!(value, "o" | r"\[bu]" | r"\(bu")
        || matches!(value.chars().collect::<Vec<_>>().as_slice(), [glyph] if !glyph.is_alphanumeric())
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

fn collect_ast_topology(node: &Node, topology: &mut AstTopology) {
    if node.kind == NodeKind::Block
        && mdoc_list_topology_kind(node) == Some(MdocContainerKind::Table)
        && node.line > 0
    {
        topology.table_rows.extend(mdoc_column_rows(node));
    } else if node.kind == NodeKind::Block
        && let Some(kind) = mdoc_list_topology_kind(node)
        && node.line > 0
    {
        topology.lists.push(AstListTopology {
            source_line: node.line,
            kind: match kind {
                MdocContainerKind::Generic => ListTopologyKind::Generic,
                MdocContainerKind::Definition => ListTopologyKind::Definition,
                MdocContainerKind::Table => unreachable!("column lists are tables"),
            },
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
        collect_ast_topology(child, topology);
    }
}

fn ir_profile(document: &Document) -> (IrStructure, IrTopology) {
    let mut profile = IrStructure::default();
    let mut topology = IrTopology::default();
    collect_blocks(&document.blocks, &mut profile, &mut topology);
    for section in &document.sections {
        collect_section(section, &mut profile, &mut topology);
    }
    topology.lists.sort_by_key(|list| list.source_line);
    (profile, topology)
}

fn collect_section(section: &Section, profile: &mut IrStructure, topology: &mut IrTopology) {
    collect_blocks(&section.blocks, profile, topology);
    for child in &section.children {
        collect_section(child, profile, topology);
    }
}

#[allow(clippy::too_many_lines)]
fn collect_blocks(blocks: &[Block], profile: &mut IrStructure, topology: &mut IrTopology) {
    for block in blocks {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                profile.paragraph_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                collect_inlines(children, profile);
            }
            Block::Unsupported { layout, .. } | Block::Equation { layout, .. } => {
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
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
                collect_inlines(children, profile);
            }
            Block::List {
                items,
                layout,
                source,
                ..
            } => {
                profile.generic_list_items += items.len();
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if let Some(source) = source {
                    topology.lists.push(IrListTopology {
                        source_line: source.line,
                        kind: ListTopologyKind::Generic,
                        items: items.len(),
                    });
                }
                for item in items {
                    collect_blocks(&item.blocks, profile, topology);
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
                if let Some(source) = source {
                    topology.lists.push(IrListTopology {
                        source_line: source.line,
                        kind: ListTopologyKind::Definition,
                        items: items.len(),
                    });
                }
                for item in items {
                    for term in &item.terms {
                        collect_inlines(term, profile);
                    }
                    collect_blocks(&item.description, profile, topology);
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
                        collect_blocks(&cell.blocks, profile, topology);
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

fn collect_inlines(inlines: &[Inline], profile: &mut IrStructure) {
    for inline in inlines {
        match inline {
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect_inlines(children, profile);
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
                collect_inlines(children, profile);
            }
            Inline::LineBreak => profile.hard_breaks += 1,
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => {}
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
    underflow(
        &mut violations,
        "literal-displays",
        expected.literal_displays,
        observed.preformatted_blocks,
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
        "section-links",
        expected.section_links,
        observed.section_links,
    );
    if expected.max_relative_indent_depth > 0 && observed.max_indent_columns == 0 {
        violations.push(format!(
            "relative-indent: expected nested RS depth {}, observed no indented IR block",
            expected.max_relative_indent_depth
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
    violations
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
        if expected_row.cells.len() != observed_row.cells.len() {
            violations.push(format!(
                "table-topology at row {}: expected {} cells, observed {}",
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
    use super::is_visible_no_fill_text;

    #[test]
    fn standalone_roff_font_switches_do_not_claim_visible_no_fill_lines() {
        assert!(!is_visible_no_fill_text(r"\f[C]"));
        assert!(!is_visible_no_fill_text(r"\fR"));
        assert!(is_visible_no_fill_text(r"\f[C]visible\f[R]"));
        assert!(is_visible_no_fill_text("visible"));
    }
}
