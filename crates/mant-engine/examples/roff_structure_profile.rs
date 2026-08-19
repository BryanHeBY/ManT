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
use mant_ir::{Block, Document, Inline, Section};
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-structure-profile/v1";

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AstStructure {
    no_fill_lines: usize,
    literal_displays: usize,
    generic_list_items: usize,
    definition_items: usize,
    table_rows: usize,
    table_spanning_cells: usize,
    indented_scopes: usize,
    hard_breaks: usize,
    navigation_links: usize,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct IrStructure {
    preformatted_blocks: usize,
    preformatted_lines: usize,
    generic_list_items: usize,
    definition_items: usize,
    table_rows: usize,
    table_spanning_cells: usize,
    indented_blocks: usize,
    hard_breaks: usize,
    navigation_links: usize,
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
    let expected = ast_structure(&report.document.root);
    let document = parse_manual_page(&ManualPage {
        name: "audit".to_owned(),
        section: "1".to_owned(),
        path,
        manual_root: root,
    })
    .map_err(|error| error.to_string())?;
    let observed = ir_structure(&document);
    let violations = compare_structure(&expected, &observed);

    Ok(json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "expected": expected,
        "observed": observed,
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

fn ast_structure(root: &Node) -> AstStructure {
    let mut profile = AstStructure::default();
    let mut no_fill_lines = BTreeMap::new();
    collect_ast_structure(root, false, &mut profile, &mut no_fill_lines);
    profile.no_fill_lines = no_fill_lines
        .values()
        .filter(|continues_line| !**continues_line)
        .count();
    profile
}

fn collect_ast_structure(
    node: &Node,
    inherited_nonprinting: bool,
    profile: &mut AstStructure,
    no_fill_lines: &mut BTreeMap<u32, bool>,
) {
    let nonprinting = inherited_nonprinting || node.flags.no_print || is_stateful_request(node);
    if node.flags.no_fill
        && !nonprinting
        && node.kind == NodeKind::Text
        && node.text.as_deref().is_some_and(|text| !text.is_empty())
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
            Some("Bl") => match node.list_kind {
                Some(NormalizedListKind::Definition | NormalizedListKind::Column) => {
                    profile.definition_items += direct_list_item_count(node);
                }
                _ => profile.generic_list_items += direct_list_item_count(node),
            },
            Some("TP" | "IP" | "TQ") => profile.definition_items += 1,
            Some("RS") => profile.indented_scopes += 1,
            _ => {}
        }
    }
    match node.macro_name.as_deref() {
        Some("br") if node.kind == NodeKind::Element => profile.hard_breaks += 1,
        Some("Xr" | "Lk" | "Mt" | "UR") => profile.navigation_links += 1,
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
    for child in &node.children {
        collect_ast_structure(child, nonprinting, profile, no_fill_lines);
    }
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

fn ir_structure(document: &Document) -> IrStructure {
    let mut profile = IrStructure::default();
    collect_blocks(&document.blocks, &mut profile);
    for section in &document.sections {
        collect_section(section, &mut profile);
    }
    profile
}

fn collect_section(section: &Section, profile: &mut IrStructure) {
    collect_blocks(&section.blocks, profile);
    for child in &section.children {
        collect_section(child, profile);
    }
}

fn collect_blocks(blocks: &[Block], profile: &mut IrStructure) {
    for block in blocks {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                profile.indented_blocks += usize::from(layout.indent_columns > 0);
                collect_inlines(children, profile);
            }
            Block::Unsupported { layout, .. } | Block::Equation { layout, .. } => {
                profile.indented_blocks += usize::from(layout.indent_columns > 0);
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                profile.preformatted_blocks += 1;
                profile.indented_blocks += usize::from(layout.indent_columns > 0);
                if has_visible_inline(children) {
                    profile.preformatted_lines += 1;
                }
                profile.preformatted_lines += line_break_count(children);
                collect_inlines(children, profile);
            }
            Block::List { items, layout, .. } => {
                profile.generic_list_items += items.len();
                profile.indented_blocks += usize::from(layout.indent_columns > 0);
                for item in items {
                    collect_blocks(&item.blocks, profile);
                }
            }
            Block::DefinitionList { items, layout, .. } => {
                profile.definition_items += items.len();
                profile.indented_blocks += usize::from(layout.indent_columns > 0);
                for item in items {
                    for term in &item.terms {
                        collect_inlines(term, profile);
                    }
                    collect_blocks(&item.description, profile);
                }
            }
            Block::Table { rows, layout, .. } => {
                profile.indented_blocks += usize::from(layout.indent_columns > 0);
                profile.table_rows += rows.len();
                for row in rows {
                    profile.table_spanning_cells += row
                        .cells
                        .iter()
                        .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
                        .count();
                    for cell in &row.cells {
                        collect_blocks(&cell.blocks, profile);
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
            Inline::Link { children, .. } => {
                profile.navigation_links += 1;
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

fn compare_structure(expected: &AstStructure, observed: &IrStructure) -> Vec<String> {
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
        "definition-items",
        expected.definition_items,
        observed.definition_items,
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
        "indented-scopes",
        expected.indented_scopes,
        observed.indented_blocks,
    );
    underflow(
        &mut violations,
        "hard-breaks",
        expected.hard_breaks,
        observed.hard_breaks,
    );
    underflow(
        &mut violations,
        "navigation-links",
        expected.navigation_links,
        observed.navigation_links,
    );
    violations
}

fn underflow(violations: &mut Vec<String>, property: &str, expected: usize, observed: usize) {
    if expected > observed {
        violations.push(format!(
            "{property}: expected at least {expected}, observed {observed}"
        ));
    }
}
