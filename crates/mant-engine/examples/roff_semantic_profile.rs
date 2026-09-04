//! Batch semantic-entry precision profiler for local roff audits.
//!
//! This development-only example accepts one JSON object per stdin line and
//! reports the final semantic entries plus high-confidence classification
//! anomalies that target and visible-content audits cannot observe.

use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use libmandoc_rs::{Compression, IncludePolicy, Node, ParseOptions, Parser};
use mant_engine::lower_mandoc_document;
use mant_ir::{
    Block, DefinitionRole, Document, EntryKind, Inline, ParameterKind, Section, SemanticEntry,
    SemanticIndex, ValueDomain,
};
use serde::Serialize;
use serde_json::{Value, json};

#[path = "roff_semantic_profile/conversions.rs"]
mod conversions;

use conversions::{conversion_violations, ordinal_conversions};

const PROFILE_SCHEMA: &str = "mant.roff-semantic-profile/v2";
const SAMPLE_LIMIT: usize = 32;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryRecord {
    id: String,
    kind: &'static str,
    aliases: Vec<String>,
    forms: Vec<String>,
    targets: Vec<String>,
    containing_section: Option<String>,
    containing_section_title: Option<String>,
    containing_section_source_line: u32,
    nested_depth: usize,
    value_domain_origin: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DefinitionCandidate {
    form: String,
    identity: Option<String>,
    role: Option<&'static str>,
    containing_section: Option<String>,
    containing_section_title: Option<String>,
    containing_section_source_line: u32,
    ir_path: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("roff_semantic_profile: {error}");
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
    }
    stdout.flush().map_err(|error| error.to_string())
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
    let document = lower_mandoc_document(&path, &report);
    Ok(profile_document(
        id,
        &report.document.root,
        &document,
        report.diagnostics.len(),
    ))
}

fn profile_document(
    id: &str,
    native_root: &Node,
    document: &Document,
    parser_diagnostics: usize,
) -> Value {
    let entries = entry_records(document);
    let ordinal_entries = entries
        .iter()
        .filter(|entry| {
            matches!(entry.kind, "term" | "value")
                && entry.forms.iter().any(|form| ordinal_marker(form))
        })
        .collect::<Vec<_>>();
    let empty_entries = entries
        .iter()
        .filter(|entry| {
            entry.aliases.is_empty() && entry.forms.iter().all(|form| form.trim().is_empty())
        })
        .collect::<Vec<_>>();
    let mut ordinal_definitions = Vec::new();
    collect_definition_candidates(
        &document.blocks,
        None,
        None,
        0,
        "document",
        &mut ordinal_definitions,
    );
    for (index, section) in document.sections.iter().enumerate() {
        collect_section_definition_candidates(
            section,
            &format!("section[{index}]"),
            &mut ordinal_definitions,
        );
    }
    let value_domain_violations = value_domain_violations(document);
    let ordinal_conversions = ordinal_conversions(native_root, document);
    let ordinal_conversion_violations = conversion_violations(&ordinal_conversions);
    let aliasless_generic_terms = entries
        .iter()
        .filter(|entry| entry.kind == "term" && entry.aliases.is_empty())
        .collect::<Vec<_>>();
    let note_like_entries = entries
        .iter()
        .filter(|entry| {
            entry
                .containing_section_title
                .as_deref()
                .is_some_and(note_like_title)
        })
        .collect::<Vec<_>>();
    let mut counts = BTreeMap::<&str, usize>::new();
    for entry in &entries {
        *counts.entry(entry.kind).or_default() += 1;
    }
    let mut violations = ordinal_entries
        .iter()
        .map(|entry| {
            format!(
                "ordinal semantic {} {:?} in {}",
                entry.kind,
                entry.forms,
                entry
                    .containing_section
                    .as_deref()
                    .unwrap_or("document root")
            )
        })
        .collect::<Vec<_>>();
    violations.extend(ordinal_definitions.iter().map(|candidate| {
        format!(
            "ordinal definition {:?} remains at {}",
            candidate.form, candidate.ir_path
        )
    }));
    violations.extend(empty_entries.iter().map(|entry| {
        format!(
            "semantic {} {:?} has no alias or visible form",
            entry.kind, entry.id
        )
    }));
    violations.extend(value_domain_violations.iter().cloned());
    violations.extend(ordinal_conversion_violations.iter().cloned());

    json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "entries": entries,
        "entryCounts": counts,
        "ordinalEntries": ordinal_entries,
        "ordinalDefinitions": ordinal_definitions,
        "emptyEntries": empty_entries,
        "aliaslessGenericTermCount": aliasless_generic_terms.len(),
        "aliaslessGenericTermSamples": aliasless_generic_terms.into_iter().take(SAMPLE_LIMIT).collect::<Vec<_>>(),
        "noteLikeEntryCount": note_like_entries.len(),
        "noteLikeEntrySamples": note_like_entries.into_iter().take(SAMPLE_LIMIT).collect::<Vec<_>>(),
        "valueDomainViolations": value_domain_violations,
        "ordinalConversions": ordinal_conversions,
        "ordinalConversionViolations": ordinal_conversion_violations,
        "diagnostics": {
            "parser": parser_diagnostics,
            "ir": document.diagnostics.len(),
        },
        "violations": violations,
    })
}

fn path_field(request: &Value, field: &str) -> Result<PathBuf, String> {
    request
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("request.{field} must be a non-empty string"))
}

fn entry_records(document: &Document) -> Vec<EntryRecord> {
    let index = SemanticIndex::build(document);
    let mut output = Vec::new();
    collect_entries(index.root(), None, None, 0, 0, &mut output);
    for section in &document.sections {
        collect_section_entries(section, &index, &mut output);
    }
    output
}

fn collect_section_entries(
    section: &Section,
    index: &SemanticIndex,
    output: &mut Vec<EntryRecord>,
) {
    collect_entries(
        index.section(section.id.as_str()),
        Some(section.id.as_str()),
        Some(&section.title),
        section.source.map_or(0, |source| source.line),
        0,
        output,
    );
    for child in &section.children {
        collect_section_entries(child, index, output);
    }
}

fn collect_entries(
    entries: &[SemanticEntry],
    section: Option<&str>,
    section_title: Option<&str>,
    section_source_line: u32,
    depth: usize,
    output: &mut Vec<EntryRecord>,
) {
    for entry in entries {
        output.push(EntryRecord {
            id: entry.id.to_string(),
            kind: entry_kind(entry.kind),
            aliases: entry.aliases.clone(),
            forms: entry.forms.clone(),
            targets: vec![entry.id.to_string()],
            containing_section: section.map(str::to_owned),
            containing_section_title: section_title.map(str::to_owned),
            containing_section_source_line: section_source_line,
            nested_depth: depth,
            value_domain_origin: entry.value_domain.as_ref().map(value_domain_origin),
        });
        collect_entries(
            &entry.children,
            section,
            section_title,
            section_source_line,
            depth + 1,
            output,
        );
    }
}

const fn entry_kind(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Command => "command",
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option,
        } => "option",
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Marker,
        } => "marker",
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Operand,
        } => "operand",
        EntryKind::ConfigurationKey => "configuration-key",
        EntryKind::EnvironmentVariable => "environment-variable",
        EntryKind::Variable => "variable",
        EntryKind::Value => "value",
        EntryKind::Term => "term",
    }
}

const fn value_domain_origin(domain: &ValueDomain) -> &'static str {
    match domain {
        ValueDomain::Choices { .. } => "child-choices",
        ValueDomain::EntrySet { .. } => "external-entry-set",
    }
}

fn collect_section_definition_candidates(
    section: &Section,
    path: &str,
    output: &mut Vec<DefinitionCandidate>,
) {
    let line = section.source.map_or(0, |source| source.line);
    collect_definition_candidates(
        &section.blocks,
        Some(section.id.as_str()),
        Some(&section.title),
        line,
        path,
        output,
    );
    for (index, child) in section.children.iter().enumerate() {
        collect_section_definition_candidates(child, &format!("{path}/section[{index}]"), output);
    }
}

fn collect_definition_candidates(
    blocks: &[Block],
    section: Option<&str>,
    section_title: Option<&str>,
    section_source_line: u32,
    path: &str,
    output: &mut Vec<DefinitionCandidate>,
) {
    for (block_index, block) in blocks.iter().enumerate() {
        let block_path = format!("{path}/block[{block_index}]");
        match block {
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    for form in item.terms.iter().map(|term| inline_text(term)) {
                        if ordinal_marker(&form) {
                            output.push(DefinitionCandidate {
                                form,
                                identity: item
                                    .identity
                                    .as_ref()
                                    .map(|identity| identity.id.to_string()),
                                role: item
                                    .identity
                                    .as_ref()
                                    .map(|identity| definition_role(identity.role)),
                                containing_section: section.map(str::to_owned),
                                containing_section_title: section_title.map(str::to_owned),
                                containing_section_source_line: section_source_line,
                                ir_path: format!("{block_path}/definition[{item_index}]"),
                            });
                        }
                    }
                    collect_definition_candidates(
                        &item.description,
                        section,
                        section_title,
                        section_source_line,
                        &format!("{block_path}/definition[{item_index}]/description"),
                        output,
                    );
                }
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    collect_definition_candidates(
                        &item.blocks,
                        section,
                        section_title,
                        section_source_line,
                        &format!("{block_path}/item[{item_index}]"),
                        output,
                    );
                }
            }
            Block::Table { rows, .. } => {
                for (row_index, row) in rows.iter().enumerate() {
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        collect_definition_candidates(
                            &cell.blocks,
                            section,
                            section_title,
                            section_source_line,
                            &format!("{block_path}/row[{row_index}]/cell[{cell_index}]"),
                            output,
                        );
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

const fn definition_role(role: DefinitionRole) -> &'static str {
    match role {
        DefinitionRole::Option => "option",
        DefinitionRole::Marker => "marker",
        DefinitionRole::Operand => "operand",
        DefinitionRole::Command => "command",
        DefinitionRole::ConfigurationKey => "configuration-key",
        DefinitionRole::EnvironmentVariable => "environment-variable",
        DefinitionRole::Variable => "variable",
        DefinitionRole::Value => "value",
        DefinitionRole::Term => "term",
    }
}

fn inline_text(nodes: &[Inline]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => output.push_str(&inline_text(children)),
            Inline::LineBreak => output.push('\n'),
            Inline::Anchor { .. } => {}
        }
    }
    output
}

fn ordinal_marker(value: &str) -> bool {
    let value = value.trim();
    let digits = value
        .strip_suffix('.')
        .or_else(|| {
            value
                .strip_suffix(')')
                .map(|digits| digits.strip_prefix('(').unwrap_or(digits))
        })
        .or_else(|| {
            value
                .strip_prefix('[')
                .and_then(|digits| digits.strip_suffix(']'))
        });
    digits.is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
    })
}

fn note_like_title(title: &str) -> bool {
    title
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "note" | "notes" | "footnote" | "footnotes" | "reference" | "references"
            )
        })
}

fn value_domain_violations(document: &Document) -> Vec<String> {
    let mut violations = Vec::new();
    let index = SemanticIndex::build(document);
    check_value_domains(index.root(), "document", &mut violations);
    for section in &document.sections {
        check_section_value_domains(section, &index, &mut violations);
    }
    violations
}

fn check_section_value_domains(
    section: &Section,
    index: &SemanticIndex,
    violations: &mut Vec<String>,
) {
    check_value_domains(
        index.section(section.id.as_str()),
        section.id.as_str(),
        violations,
    );
    for child in &section.children {
        check_section_value_domains(child, index, violations);
    }
}

fn check_value_domains(entries: &[SemanticEntry], scope: &str, violations: &mut Vec<String>) {
    for entry in entries {
        if matches!(entry.value_domain, Some(ValueDomain::Choices { .. }))
            && entry
                .children
                .iter()
                .any(|child| child.kind != EntryKind::Value)
        {
            violations.push(format!(
                "choices entry {:?} in {scope} has a non-value child",
                entry.id
            ));
        }
        check_value_domains(&entry.children, scope, violations);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinal_probe_accepts_only_punctuated_integers() {
        for value in ["1.", "2)", "(3)", "[4]"] {
            assert!(super::ordinal_marker(value));
        }
        for value in ["0", "1", "2.2", "v1.", "1.2.", "[x]"] {
            assert!(!super::ordinal_marker(value));
        }
    }

    #[test]
    fn note_like_titles_are_token_based() {
        for title in ["NOTES", "Footnotes", "Upstream references"] {
            assert!(super::note_like_title(title));
        }
        for title in ["Noteworthy behavior", "ReferenceCount"] {
            assert!(!super::note_like_title(title));
        }
    }
}
