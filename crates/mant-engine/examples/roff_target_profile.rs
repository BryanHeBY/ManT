//! Batch target-conservation profiler for local roff audits.
//!
//! This development-only example accepts one JSON object per stdin line:
//!
//! `{ "id": "...", "path": "/.../git.1.gz", "root": "/usr/share/man" }`
//!
//! It compares navigation targets intentionally retained by libmandoc with
//! section and inline identities in the source-aware `ManT` IR. Unlike visible
//! text and layout audits, zero-width anchors are the primary evidence here.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use libmandoc_rs::{Compression, IncludePolicy, Node, NodeKind, ParseOptions, Parser};
use mant_engine::{ManualPage, parse_manual_page};
use mant_ir::{Block, Document, Inline, Section};
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-target-profile/v1";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedTarget {
    id: String,
    normalized_id: String,
    source_line: u32,
    owner_macro: String,
    owner_kind: String,
    explicit: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("roff_target_profile: {error}");
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
        includes: IncludePolicy::Root(root.clone()),
        compression: Compression::Auto,
    })
    .parse_file(&path)
    .map_err(|error| error.to_string())?;
    let expected = expected_targets(&report.document.root);
    let target_owners = target_owner_summary(&report.document.root);
    let document = parse_manual_page(&ManualPage {
        name: "audit".to_owned(),
        section: "1".to_owned(),
        path,
        manual_root: root,
    })
    .map_err(|error| error.to_string())?;
    let alias = document.meta.alias_target.is_some();
    let observed = observed_identities(&document);
    let missing = if alias {
        Vec::new()
    } else {
        missing_targets(&expected, &observed)
    };
    let violations = missing
        .iter()
        .map(|target| {
            format!(
                "missing target {:?} from {} {} at line {}",
                target.id, target.owner_kind, target.owner_macro, target.source_line
            )
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "expected": expected,
        "targetOwners": target_owners,
        "observed": observed,
        "missing": missing,
        "alias": alias,
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

fn expected_targets(root: &Node) -> Vec<ExpectedTarget> {
    let mut flattened = Vec::new();
    flatten_nodes(root, &mut flattened);
    let explicit = explicit_targets(&flattened);
    let mut targets = BTreeMap::<(String, u32, String, String), ExpectedTarget>::new();

    for node in flattened {
        if !node.flags.deep_link_target || !structural_target_owner(node) {
            continue;
        }
        let Some(id) = target_name(node) else {
            continue;
        };
        let is_explicit = explicit.contains(&id);
        if matches!(node.macro_name.as_deref(), Some("Sh" | "Ss")) && !is_explicit {
            // Every lowered section is independently addressable, but ManT's
            // source-neutral section ID is derived from the complete visible
            // title rather than mandoc's sometimes truncated renderer tag.
            continue;
        }
        let owner_macro = node.macro_name.clone().unwrap_or_default();
        let owner_kind = format!("{:?}", node.kind).to_ascii_lowercase();
        let target = ExpectedTarget {
            normalized_id: document_id_slug(&id),
            explicit: is_explicit,
            id: id.clone(),
            source_line: node.line,
            owner_macro: owner_macro.clone(),
            owner_kind: owner_kind.clone(),
        };
        targets.insert((id, node.line, owner_macro, owner_kind), target);
    }

    // A target can remain on an inline `.Tg` or on a visible inline macro.
    // It remains an obligation even though it has no structural owner.
    for id in explicit {
        if targets.values().any(|target| target.id == id) {
            continue;
        }
        targets.insert(
            (id.clone(), 0, "Tg".to_owned(), "explicit".to_owned()),
            ExpectedTarget {
                normalized_id: document_id_slug(&id),
                id,
                source_line: 0,
                owner_macro: "Tg".to_owned(),
                owner_kind: "explicit".to_owned(),
                explicit: true,
            },
        );
    }
    targets.into_values().collect()
}

fn target_owner_summary(root: &Node) -> BTreeMap<String, usize> {
    let mut nodes = Vec::new();
    flatten_nodes(root, &mut nodes);
    let mut owners = BTreeMap::new();
    for node in nodes {
        if !node.flags.deep_link_target {
            continue;
        }
        let key = format!(
            "{}/{}",
            node.macro_name.as_deref().unwrap_or("<none>"),
            format!("{:?}", node.kind).to_ascii_lowercase()
        );
        *owners.entry(key).or_default() += 1;
    }
    owners
}

fn explicit_targets(nodes: &[&Node]) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.macro_name.as_deref() != Some("Tg") {
            continue;
        }
        let target = first_text(node).map(str::to_owned).or_else(|| {
            nodes[index + 1..]
                .iter()
                .find(|candidate| candidate.flags.deep_link_target)
                .and_then(|candidate| target_name(candidate))
        });
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            targets.insert(target);
        }
    }
    targets
}

fn structural_target_owner(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("Tg" | "Pp" | "Bd" | "D1" | "Dl" | "Bl" | "It" | "Sh" | "Ss" | "Fo")
    ) || matches!(node.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail)
        && matches!(
            node.macro_name.as_deref(),
            Some("Bd" | "Bl" | "It" | "Sh" | "Ss" | "Fo")
        )
}

fn target_name(node: &Node) -> Option<String> {
    node.tag
        .as_deref()
        .map(str::to_owned)
        .or_else(|| first_text(node).map(first_token))
        .filter(|target| !target.is_empty())
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text && !node.flags.no_print {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

fn first_token(value: &str) -> String {
    value
        .trim_start_matches('-')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn flatten_nodes<'a>(node: &'a Node, output: &mut Vec<&'a Node>) {
    output.push(node);
    for child in &node.children {
        flatten_nodes(child, output);
    }
}

fn observed_identities(document: &Document) -> BTreeSet<String> {
    let mut identities = BTreeSet::new();
    collect_blocks(&document.blocks, &mut identities);
    for section in &document.sections {
        collect_section(section, &mut identities);
    }
    identities
}

fn collect_section(section: &Section, identities: &mut BTreeSet<String>) {
    identities.insert(section.id.to_string());
    collect_blocks(&section.blocks, identities);
    for child in &section.children {
        collect_section(child, identities);
    }
}

fn collect_blocks(blocks: &[Block], identities: &mut BTreeSet<String>) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                collect_inlines(children, identities);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.blocks, identities);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    if let Some(identity) = &item.identity {
                        identities.insert(identity.id.to_string());
                    }
                    for term in &item.terms {
                        collect_inlines(term, identities);
                    }
                    collect_blocks(&item.description, identities);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_blocks(&cell.blocks, identities);
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_inlines(nodes: &[Inline], identities: &mut BTreeSet<String>) {
    for node in nodes {
        match node {
            Inline::Anchor { id } => {
                identities.insert(id.to_string());
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_inlines(children, identities),
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
}

fn missing_targets(
    expected: &[ExpectedTarget],
    observed: &BTreeSet<String>,
) -> Vec<ExpectedTarget> {
    let mut automatic_required = BTreeMap::<&str, usize>::new();
    let mut missing = Vec::new();
    for target in expected {
        if target.explicit {
            if !observed.contains(&target.id) {
                missing.push(target.clone());
            }
        } else {
            *automatic_required
                .entry(target.normalized_id.as_str())
                .or_default() += 1;
        }
    }
    for (base, required) in automatic_required {
        let retained = observed
            .iter()
            .filter(|candidate| generated_identity_matches(base, candidate))
            .count();
        if retained >= required {
            continue;
        }
        missing.extend(
            expected
                .iter()
                .filter(|target| !target.explicit && target.normalized_id == base)
                .skip(retained)
                .cloned(),
        );
    }
    missing
}

fn generated_identity_matches(base: &str, candidate: &str) -> bool {
    candidate == base
        || candidate
            .strip_prefix(base)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix
                        .chars()
                        .all(|character| character.is_ascii_hexdigit())
                    || suffix.parse::<usize>().is_ok()
            })
}

fn document_id_slug(value: &str) -> String {
    if value.trim_start_matches(['-', '/']) == "?" {
        return "help".to_owned();
    }
    let slug = value
        .trim_start_matches(['-', '/'])
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "entry".to_owned()
    } else {
        slug
    }
}
