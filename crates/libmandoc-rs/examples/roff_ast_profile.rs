//! Batch profiler for the real libmandoc syntax tree.
//!
//! This is a development tool used by `scripts/audit-roff-fidelity.py`, not a
//! user-facing `ManT` command. It accepts one JSON object per stdin line:
//!
//! `{ "id": "...", "path": "/.../git.1.gz", "root": "/usr/share/man" }`
//!
//! and writes one JSON profile per line. Keeping the parser alive across the
//! batch makes syntax-aware corpus sampling practical without adding an audit
//! interface to the shipped binary.

use std::{
    collections::BTreeSet,
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use libmandoc_rs::{
    AuthorMode, Compression, DiagnosticLevel, DisplayKind, IncludePolicy, MacroSet, Node, NodeKind,
    NormalizedFont, NormalizedListKind, ParseOptions, Parser, TableAlignment,
};
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-ast-profile/v2";

fn main() {
    if let Err(error) = run() {
        eprintln!("roff_ast_profile: {error}");
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
        includes: IncludePolicy::Root(root),
        compression: Compression::Auto,
    })
    .parse_file(&path)
    .map_err(|error| error.to_string())?;
    let mut features = syntax_features(&report.document.root);
    features.insert(format!(
        "macro-set:{}",
        macro_set_name(report.document.macro_set)
    ));
    if report.document.metadata.alias_target.is_some() {
        features.insert("metadata:alias-target".to_owned());
    }
    if !report.document.metadata.has_body {
        features.insert("metadata:no-body".to_owned());
    }
    for diagnostic in &report.diagnostics {
        features.insert(format!(
            "diagnostic:{}",
            diagnostic_level_name(diagnostic.level)
        ));
    }
    Ok(json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "features": features,
        "diagnostics": report.diagnostics.len(),
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

fn syntax_features(root: &Node) -> BTreeSet<String> {
    let mut features = BTreeSet::new();
    collect_node_features(root, None, &mut features);
    features
}

fn collect_node_features(node: &Node, parent: Option<&Node>, features: &mut BTreeSet<String>) {
    let kind = node_kind_name(node.kind);
    features.insert(format!("node:{kind}"));
    let identity = node_identity(node);
    let mut properties = BTreeSet::new();
    if let Some(name) = node.macro_name.as_deref() {
        features.insert(format!("macro:{name}"));
        features.insert(format!("macro-kind:{name}:{kind}"));
    }
    if let Some(parent) = parent {
        features.insert(format!(
            "edge:{}>{}",
            node_identity(parent),
            node_identity(node)
        ));
    }
    if let Some(list_kind) = node.list_kind {
        properties.insert(format!("list:{}", list_kind_name(list_kind)));
    }
    if let Some(display_kind) = node.display_kind {
        properties.insert(format!("display:{}", display_kind_name(display_kind)));
    }
    if let Some(font) = node.font {
        properties.insert(format!("font:{}", font_name(font)));
    }
    if let Some(author_mode) = node.author_mode {
        properties.insert(format!("author:{}", author_mode_name(author_mode)));
    }
    if node.enclosure.is_some() {
        properties.insert("enclosure:resolved".to_owned());
    }
    if node.compact {
        properties.insert("layout:compact".to_owned());
    }
    if node.offset.is_some() {
        properties.insert("layout:offset".to_owned());
    }
    if node.width.is_some() {
        properties.insert("layout:width".to_owned());
    }
    if !node.table_cells.is_empty() {
        properties.insert("table:cells".to_owned());
        for cell in &node.table_cells {
            if cell.text_block {
                properties.insert("table:text-block".to_owned());
            }
            if cell.vertical_continuation {
                properties.insert("table:vertical-continuation".to_owned());
            }
            properties.insert(format!(
                "table:alignment:{}",
                table_alignment_name(cell.alignment)
            ));
            if cell.column_span > 1 {
                properties.insert("table:column-span".to_owned());
            }
            if cell.row_span > 1 {
                properties.insert("table:row-span".to_owned());
            }
        }
    }
    if node.equation.is_some() {
        properties.insert("equation:expression".to_owned());
    }
    for (enabled, name) in [
        (node.flags.generated, "generated"),
        (node.flags.sentence_end, "sentence-end"),
        (node.flags.no_print, "no-print"),
        (node.flags.no_fill, "no-fill"),
        (node.flags.deep_link_target, "deep-link-target"),
        (node.flags.permalink, "permalink"),
        (node.flags.line_start, "line-start"),
        (node.flags.delimiter_open, "delimiter-open"),
        (node.flags.delimiter_close, "delimiter-close"),
        (node.flags.line_continuation, "line-continuation"),
        (node.flags.synopsis_pretty, "synopsis-pretty"),
    ] {
        if enabled {
            properties.insert(format!("flag:{name}"));
        }
    }
    features.extend(properties.iter().cloned());
    for property in &properties {
        features.insert(format!("interaction:{identity}+{property}"));
        if let Some(parent) = parent {
            features.insert(format!(
                "interaction:{}>{identity}+{property}",
                node_identity(parent)
            ));
        }
    }
    let properties = properties.into_iter().collect::<Vec<_>>();
    for (index, left) in properties.iter().enumerate() {
        for right in &properties[index + 1..] {
            features.insert(format!("interaction:{left}+{right}"));
        }
    }
    for child in &node.children {
        collect_node_features(child, Some(node), features);
    }
}

fn node_identity(node: &Node) -> String {
    node.macro_name.as_deref().map_or_else(
        || node_kind_name(node.kind).to_owned(),
        |name| format!("{name}:{}", node_kind_name(node.kind)),
    )
}

const fn macro_set_name(value: MacroSet) -> &'static str {
    match value {
        MacroSet::None => "none",
        MacroSet::Mdoc => "mdoc",
        MacroSet::Man => "man",
    }
}

const fn node_kind_name(value: NodeKind) -> &'static str {
    match value {
        NodeKind::Root => "root",
        NodeKind::Block => "block",
        NodeKind::Head => "head",
        NodeKind::Body => "body",
        NodeKind::Tail => "tail",
        NodeKind::Element => "element",
        NodeKind::Text => "text",
        NodeKind::Comment => "comment",
        NodeKind::Table => "table",
        NodeKind::Equation => "equation",
    }
}

const fn list_kind_name(value: NormalizedListKind) -> &'static str {
    match value {
        NormalizedListKind::Bullet => "bullet",
        NormalizedListKind::Ordered => "ordered",
        NormalizedListKind::Definition => "definition",
        NormalizedListKind::Column => "column",
        NormalizedListKind::Plain => "plain",
    }
}

const fn display_kind_name(value: DisplayKind) -> &'static str {
    match value {
        DisplayKind::Literal => "literal",
        DisplayKind::Filled => "filled",
    }
}

const fn font_name(value: NormalizedFont) -> &'static str {
    match value {
        NormalizedFont::Emphasis => "emphasis",
        NormalizedFont::Literal => "literal",
        NormalizedFont::Symbolic => "symbolic",
    }
}

const fn author_mode_name(value: AuthorMode) -> &'static str {
    match value {
        AuthorMode::Split => "split",
        AuthorMode::NoSplit => "no-split",
    }
}

const fn table_alignment_name(value: TableAlignment) -> &'static str {
    match value {
        TableAlignment::Left => "left",
        TableAlignment::Center => "center",
        TableAlignment::Right => "right",
    }
}

const fn diagnostic_level_name(value: DiagnosticLevel) -> &'static str {
    match value {
        DiagnosticLevel::Unsupported => "unsupported",
        DiagnosticLevel::Error => "error",
        DiagnosticLevel::Warning => "warning",
        DiagnosticLevel::Style => "style",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Parser, syntax_features};

    #[test]
    fn reports_semantic_shapes_without_source_text() {
        let report = Parser::default()
            .parse_bytes(
                Path::new("profile.1"),
                b".Dd August 19, 2026\n.Dt PROFILE 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet -compact\n.It\nvalue\n.El\n",
            )
            .expect("parse profile fixture");
        let features = syntax_features(&report.document.root);

        assert!(features.contains("macro:Bl"));
        assert!(features.contains("macro-kind:It:block"));
        assert!(features.contains("edge:Bl:block>Bl:head"));
        assert!(features.contains("list:bullet"));
        assert!(features.contains("layout:compact"));
        assert!(!features.iter().any(|feature| feature.contains("value")));
    }
}
