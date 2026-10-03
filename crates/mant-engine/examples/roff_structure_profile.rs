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
    path::PathBuf,
};

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
#[path = "roff_structure_profile/items.rs"]
mod structure_items;

#[path = "roff_structure_profile/comparison.rs"]
mod comparison;
#[path = "roff_structure_profile/ir.rs"]
mod ir;
#[path = "roff_structure_profile/native.rs"]
mod native;
use comparison::compare_structure;
#[cfg(test)]
use comparison::{compare_table_topology, underflow};
use ir::ir_profile;
use native::{ast_profile, semantic_link_origins};
#[cfg(test)]
use native::{
    equation_visible_text, is_no_fill_row_text, is_zero_width_guard_line, retained_no_fill_rows,
};
#[cfg(test)]
#[path = "roff_structure_profile/observer_tests.rs"]
mod observer_tests;
#[cfg(test)]
#[path = "roff_structure_profile/tests.rs"]
mod tests;

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

#[derive(PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstTableRowTopology {
    table_source_line: u32,
    table_source_column: u32,
    row_index: usize,
    kind: mant_ir::TableRowKind,
    cells: Vec<AstTableCellTopology>,
}

#[derive(Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstTableCellTopology {
    column_span: u16,
    row_span: u16,
    vertical_continuation: bool,
}

#[derive(PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrTableRowTopology {
    table_source_line: u32,
    table_source_column: u32,
    row_index: usize,
    kind: mant_ir::TableRowKind,
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
    let (expected, mut expected_topology) = ast_profile(&report.document.root);
    // Only actual owned Equation nodes establish an obligation. CVS
    // roff_parseln bypasses equation delimiters while tbl consumes a cell;
    // a root-source scan cannot invent an executed eqn environment here.
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

    let item_census = request
        .get("itemCensus")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .then(|| structure_items::item_census(&report.document.root, &document));
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
        "itemCensus": item_census,
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

const fn equation_context_order(context: EquationContext) -> u8 {
    match context {
        EquationContext::Display => 0,
        EquationContext::Inline => 1,
        EquationContext::TableCell => 2,
    }
}
