//! Batch semantic-entry precision profiler for local roff audits.
//!
//! This development-only example accepts one JSON object per stdin line and
//! reports the final semantic entries plus high-confidence classification
//! anomalies that target and visible-content audits cannot observe.

use std::{collections::BTreeMap, path::PathBuf};

use libmandoc_rs::{Compression, IncludePolicy, Node, ParseOptions, Parser};
use mant_engine::lower_mandoc_document;
use mant_ir::{
    Block, Document, EntryKind, Inline, ParameterKind, Section, SemanticEntry, SemanticIndex,
    ValueDomain,
};
use serde::Serialize;
use serde_json::{Value, json};

#[path = "roff_semantic_profile/conversions.rs"]
mod conversions;
#[path = "roff_semantic_profile/declarations.rs"]
mod declarations;
#[path = "roff_semantic_profile/queries.rs"]
mod queries;

use conversions::{conversion_violations, ordinal_conversions};

#[path = "support/profile_io.rs"]
mod profile_io;

const PROFILE_SCHEMA: &str = "mant.roff-semantic-profile/v5";
const SAMPLE_LIMIT: usize = 32;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryRecord {
    id: String,
    kind: &'static str,
    names: Vec<String>,
    /// Explicit relationships, never inferred from a shared definition head.
    alias_groups: Vec<Vec<String>>,
    alias_of: Option<String>,
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
    profile_io::run(PROFILE_SCHEMA, profile_request, false)
}

fn profile_request(line: &str) -> Result<Value, String> {
    let request: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "request.id must be a string".to_owned())?;
    if let Some(snapshot) = request.get("snapshot") {
        let bundle: mant_protocol::QueryBundle =
            serde_json::from_value(snapshot.clone()).map_err(|error| error.to_string())?;
        let content = mant_ir::ResolvedContent::from(bundle);
        let queries = request
            .get("queries")
            .and_then(Value::as_array)
            .ok_or("snapshot replay requires a queries array")?;
        return Ok(json!({"id": id, "schema": PROFILE_SCHEMA,
            "mode": "snapshot-replay-with-current-query-engine",
            "queryProfiles": queries::profile(None, &content, queries)?}));
    }
    let path = path_field(&request, "path")?;
    let mode = request
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("production");
    let (document, report) = match mode {
        "production" => mant_engine::parse_manual_source_with_report(&path)
            .map_err(|error| error.to_string())?,
        "confined-include-parser-audit" => {
            let report = Parser::new(ParseOptions {
                includes: IncludePolicy::Root(path_field(&request, "root")?),
                compression: Compression::Auto,
            })
            .parse_file(&path)
            .map_err(|error| error.to_string())?;
            (lower_mandoc_document(&path, &report), report)
        }
        _ => return Err(format!("unknown profile mode {mode:?}")),
    };
    let mut profile = profile_document(
        id,
        &report.document.root,
        &document,
        report.diagnostics.len(),
    );
    profile["mode"] = mode.into();
    if let Some(queries) = request.get("queries") {
        let queries = queries.as_array().ok_or("queries must be an array")?;
        let content = mant_ir::ResolvedContent {
            label: id.into(),
            address: None,
            document: Some(document),
            tldr: None,
        };
        profile["queryProfiles"] = serde_json::to_value(queries::profile(
            Some(&report.document.root),
            &content,
            queries,
        )?)
        .map_err(|error| error.to_string())?;
    }
    Ok(profile)
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
            entry.names.is_empty() && entry.forms.iter().all(|form| form.trim().is_empty())
        })
        .collect::<Vec<_>>();
    let ordinal_definitions = ordinal_definition_candidates(document);
    let value_domain_violations = value_domain_violations(document);
    let ordinal_conversions = ordinal_conversions(native_root, document);
    let ordinal_conversion_violations = conversion_violations(&ordinal_conversions);
    let declaration_groups = declarations::profile(native_root, document);
    let declaration_group_violations = declarations::violations(&declaration_groups);
    let aliasless_generic_terms = entries
        .iter()
        .filter(|entry| entry.kind == "term" && entry.names.is_empty())
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
    violations.extend(declaration_group_violations.iter().cloned());
    let semantic_diagnostics = mant_ir::validate_document(document)
        .into_iter()
        .filter(|diagnostic| !mant_engine::semantics_complete(std::slice::from_ref(diagnostic)))
        .map(|diagnostic| diagnostic.message)
        .collect::<std::collections::BTreeSet<_>>();
    violations.extend(semantic_diagnostics.iter().cloned());

    json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "entries": entries,
        "entryCounts": counts,
        "relationshipCounts": relationship_counts(&entries),
        "semanticsComplete": mant_engine::semantics_complete(&document.diagnostics) && semantic_diagnostics.is_empty(),
        "semanticViolations": semantic_diagnostics,
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
        "declarationGroups": declaration_groups,
        "declarationGroupViolations": declaration_group_violations,
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

fn ordinal_definition_candidates(document: &Document) -> Vec<DefinitionCandidate> {
    let mut candidates = Vec::new();
    collect_definition_candidates(&document.blocks, None, None, 0, "document", &mut candidates);
    for (index, section) in document.sections.iter().enumerate() {
        collect_section_definition_candidates(
            section,
            &format!("section[{index}]"),
            &mut candidates,
        );
    }
    candidates
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
            names: entry.names.clone(),
            alias_groups: entry.alias_groups.clone(),
            alias_of: entry.alias_of.as_ref().map(ToString::to_string),
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

fn relationship_counts(entries: &[EntryRecord]) -> Value {
    json!({
        "names": entries.iter().map(|entry| entry.names.len()).sum::<usize>(),
        "aliasGroups": entries.iter().map(|entry| entry.alias_groups.len()).sum::<usize>(),
        "aliasGroupMembers": entries.iter().flat_map(|entry| &entry.alias_groups).map(Vec::len).sum::<usize>(),
        "aliasOf": entries.iter().filter(|entry| entry.alias_of.is_some()).count(),
    })
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
                                    .entry
                                    .as_ref()
                                    .map(|identity| identity.id.to_string()),
                                role: item
                                    .entry
                                    .as_ref()
                                    .map(|identity| entry_kind(identity.kind)),
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
    fn production_profiler_file_and_stdin_api_share_input_preparation_and_queries() {
        use std::io::Write;
        let root =
            std::env::temp_dir().join(format!("mant-profiler-parity-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (index, between) in [
            ".PP\n",
            ".if 1 .PP\n",
            ".if 0 \\{\\\n.PP\n.\\}\n",
            ".BREAK\n",
            ".de UNUSED\n.PP\n..\n",
        ]
        .iter()
        .enumerate()
        {
            let source = format!(
                ".TH PROBE 1\n.de BREAK\n.PP\n..\n.SH OPTIONS\n.TP\n.B -a\n{between}.TP\n.B -b\nBody café 日本.\n"
            );
            let mut gzip =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gzip.write_all(source.as_bytes()).unwrap();
            for (extension, bytes) in [
                ("1", source.as_bytes().to_vec()),
                ("1.gz", gzip.finish().unwrap()),
                (
                    "1.zst",
                    zstd::stream::encode_all(source.as_bytes(), 1).unwrap(),
                ),
            ] {
                let path = root.join(format!("probe-{index}.{extension}"));
                std::fs::write(&path, bytes).unwrap();
                let profile = super::profile_request(
                    &serde_json::json!({"id":"probe","path":path,"queries":["-a"]}).to_string(),
                )
                .unwrap();
                assert_eq!(profile["mode"], "production");
                assert_eq!(
                    profile["declarationGroups"]["unexpectedGroups"],
                    serde_json::json!([])
                );
                let input = mant_engine::parse_manual_bytes(&path, source.as_bytes()).unwrap();
                assert_eq!(input, mant_engine::parse_manual_source(&path).unwrap());
                let content = mant_ir::ResolvedContent {
                    label: "probe".into(),
                    address: None,
                    document: Some(input),
                    tldr: None,
                };
                let expected =
                    super::queries::profile(None, &content, &[serde_json::json!("-a")]).unwrap();
                assert_eq!(
                    profile["queryProfiles"][0]["explanation"],
                    expected[0]["explanation"]
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_binding_and_producer_coverage_are_explicit_review_evidence() {
        let source = b".TH PROBE 1 2026-09-08\n.SH OPTIONS\n.TP\n.B -D<NAME>\nDefine it.\n";
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document =
            mant_engine::lower_mandoc_document(std::path::Path::new("probe.1"), &parsed);
        let clean = super::profile_document("probe", &parsed.document.root, &document, 0);
        assert_eq!(clean["semanticViolations"], serde_json::json!([]));
        assert_eq!(clean["semanticsComplete"], true);

        let mant_ir::Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[0]
        else {
            panic!("definition owner");
        };
        items[0].entry.as_mut().unwrap().name_bindings.clear();
        // The same finding can be present in producer diagnostics and shared
        // validation. It must be reported once, not misclassified as bad JSON.
        document.diagnostics = mant_ir::validate_document(&document);
        let failed = super::profile_document("probe", &parsed.document.root, &document, 0);
        assert_eq!(failed["semanticsComplete"], false);
        assert_eq!(failed["semanticViolations"].as_array().unwrap().len(), 1);
        assert_eq!(failed["violations"], failed["semanticViolations"]);
        assert_eq!(failed["emptyEntries"], serde_json::json!([]));
    }

    #[test]
    fn shared_names_and_explicit_relationships_are_counted_separately() {
        let query = mant_engine::query_markdown_text(
            r#"# Probe

<!-- mant:entries role=option case=sensitive -->
- `-a`, `--all`: Shared content without an equivalence claim.
- `-h`, `--help`: Usage. <!-- mant:entry {"id":"help","aliasGroups":[["-h","--help"]]} -->
- `--assist`: Additional examples. <!-- mant:entry {"aliasOf":"help"} -->
"#,
            None,
        )
        .unwrap();
        let document = query.document.unwrap();
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let entries = super::entry_records(&document);
        assert_eq!(
            super::relationship_counts(&entries),
            serde_json::json!({"names":5,"aliasGroups":1,"aliasGroupMembers":2,"aliasOf":1})
        );
        assert!(entries[0].alias_groups.is_empty());
    }
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
