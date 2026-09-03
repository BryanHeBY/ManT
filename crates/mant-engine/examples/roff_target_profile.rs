//! Batch target-conservation profiler for local roff audits.
//!
//! This development-only example accepts one JSON object per stdin line:
//!
//! `{ "id": "...", "path": "/.../git.1.gz", "root": "/usr/share/man" }`
//!
//! It compares navigation targets intentionally retained by libmandoc with
//! section and inline identities in the lowered `ManT` IR. Unlike visible text
//! and layout audits, zero-width anchors are the primary evidence here.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use libmandoc_rs::{Compression, IncludePolicy, Node, NodeKind, ParseOptions, Parser};
use mant_engine::lower_mandoc_document;
use mant_ir::{Block, Document, Inline, Section};
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-target-profile/v2";

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum OwnerDisposition {
    Retained,
    Excluded,
    Unclassified,
}

impl OwnerDisposition {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Retained => "retained",
            Self::Excluded => "excluded",
            Self::Unclassified => "unclassified",
        }
    }
}

#[derive(Clone, Debug)]
struct ClassifiedOwner {
    target: Option<String>,
    source_line: u32,
    owner_macro: String,
    owner_kind: String,
    explicit: bool,
    disposition: OwnerDisposition,
    reason: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnerClass {
    owner_macro: String,
    owner_kind: String,
    disposition: &'static str,
    reason: &'static str,
    count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UnclassifiedOwner {
    target: Option<String>,
    source_line: u32,
    owner_macro: String,
    owner_kind: String,
    reason: &'static str,
}

#[derive(Default)]
struct ObservedTargets {
    identities: BTreeSet<String>,
    fragments: BTreeSet<String>,
    anchors: BTreeSet<String>,
    sections: BTreeSet<String>,
    entries: BTreeSet<String>,
    section_links: BTreeSet<String>,
}

struct AuditFindings {
    role_collisions: Vec<String>,
    identity_violations: Vec<String>,
    duplicate_target_count: usize,
    dangling_target_count: usize,
    violations: Vec<String>,
}

impl ObservedTargets {
    fn all_spellings(&self) -> BTreeSet<String> {
        self.identities.union(&self.fragments).cloned().collect()
    }
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
    let native_profile = native_target_profile(&report.document.root);
    // Compare both sides of one parser session. Re-parsing through the product
    // resolver would change the input contract for embedded `.so` trees and
    // can manufacture thousands of false losses on aggregate pages such as
    // zshall(1).
    let document = lower_mandoc_document(&path, &report);
    let alias = document.meta.alias_target.is_some();
    let observed = observed_targets(&document);
    let observed_spellings = observed.all_spellings();
    let missing = if alias {
        Vec::new()
    } else {
        missing_targets(&native_profile.expected, &observed_spellings)
    };
    let unexpected_targets = if alias {
        Vec::new()
    } else {
        unexpected_targets(&native_profile.expected, &observed)
    };
    let findings = audit_findings(
        &document,
        &missing,
        &unexpected_targets,
        &native_profile.unclassified,
    );

    Ok(json!({
        "schema": PROFILE_SCHEMA,
        "id": id,
        "expected": native_profile.expected,
        "targetOwnerCount": native_profile.owner_count,
        "classifiedOwnerCount": native_profile.owner_count - native_profile.unclassified.len(),
        "ownerClasses": native_profile.owner_classes,
        "unclassifiedOwners": native_profile.unclassified,
        "observed": observed_spellings,
        "observedIdentities": observed.identities,
        "observedFragmentAliases": observed.fragments,
        "observedEntryIdentities": observed.entries,
        "observedSectionIdentities": observed.sections,
        "anchors": observed.anchors,
        "sectionLinkTargets": observed.section_links,
        "missing": missing,
        "unexpectedTargets": unexpected_targets,
        "roleCollisions": findings.role_collisions,
        "identityViolations": findings.identity_violations,
        "duplicateTargetCount": findings.duplicate_target_count,
        "danglingTargetCount": findings.dangling_target_count,
        "alias": alias,
        "diagnostics": {
            "parser": report.diagnostics.len(),
            "ir": document.diagnostics.len(),
        },
        "violations": findings.violations,
    }))
}

fn audit_findings(
    document: &Document,
    missing: &[ExpectedTarget],
    unexpected_targets: &[String],
    unclassified: &[UnclassifiedOwner],
) -> AuditFindings {
    let role_collisions = diagnostics_with_code(document, "ir.identity-role-collision");
    let identity_violations = document
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code.as_deref(),
                Some(
                    "ir.empty-identity"
                        | "ir.invalid-identity"
                        | "ir.empty-fragment-alias"
                        | "ir.invalid-fragment-alias"
                        | "ir.ambiguous-fragment-alias"
                )
            )
        })
        .map(|diagnostic| diagnostic.message.clone())
        .collect::<Vec<_>>();
    let duplicate_target_count = diagnostics_with_code(document, "ir.duplicate-identity").len();
    let dangling_target_count = diagnostics_with_code(document, "ir.dangling-section-link").len();
    let mut violations = missing
        .iter()
        .map(|target| {
            format!(
                "missing target {:?} from {} {} at line {}",
                target.id, target.owner_kind, target.owner_macro, target.source_line
            )
        })
        .collect::<Vec<_>>();
    violations.extend(
        unexpected_targets
            .iter()
            .map(|target| format!("unexpected IR target: {target}")),
    );
    violations.extend(unclassified.iter().map(|owner| {
        format!(
            "unclassified target owner {}/{} at line {}: {}",
            owner.owner_macro, owner.owner_kind, owner.source_line, owner.reason
        )
    }));
    violations.extend(
        role_collisions
            .iter()
            .map(|collision| format!("IR identity-role collision: {collision}")),
    );
    violations.extend(
        identity_violations
            .iter()
            .map(|violation| format!("invalid IR target identity: {violation}")),
    );
    if duplicate_target_count > 0 {
        violations.push(format!(
            "{duplicate_target_count} duplicate IR target identities"
        ));
    }
    if dangling_target_count > 0 {
        violations.push(format!(
            "{dangling_target_count} dangling section-link targets"
        ));
    }
    AuditFindings {
        role_collisions,
        identity_violations,
        duplicate_target_count,
        dangling_target_count,
        violations,
    }
}

fn diagnostics_with_code(document: &Document, code: &str) -> Vec<String> {
    document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some(code))
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

fn path_field(request: &Value, field: &str) -> Result<PathBuf, String> {
    request
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("request.{field} must be a non-empty string"))
}

struct NativeTargetProfile {
    expected: Vec<ExpectedTarget>,
    owner_count: usize,
    owner_classes: Vec<OwnerClass>,
    unclassified: Vec<UnclassifiedOwner>,
}

fn native_target_profile(root: &Node) -> NativeTargetProfile {
    let mut flattened = Vec::new();
    flatten_nodes(root, &mut flattened);
    let explicit = explicit_targets(&flattened);
    let mut targets = BTreeMap::<(String, u32), ExpectedTarget>::new();
    let mut classes = BTreeMap::<(String, String, OwnerDisposition, &'static str), usize>::new();
    let mut unclassified = Vec::new();
    let mut owner_count = 0;

    for node in flattened {
        if !node.flags.deep_link_target {
            continue;
        }
        owner_count += 1;
        let owner = classify_target_owner(node, &explicit);
        *classes
            .entry((
                owner.owner_macro.clone(),
                owner.owner_kind.clone(),
                owner.disposition,
                owner.reason,
            ))
            .or_default() += 1;
        if owner.disposition == OwnerDisposition::Unclassified {
            unclassified.push(UnclassifiedOwner {
                target: owner.target,
                source_line: owner.source_line,
                owner_macro: owner.owner_macro,
                owner_kind: owner.owner_kind,
                reason: owner.reason,
            });
            continue;
        }
        if owner.disposition == OwnerDisposition::Excluded {
            continue;
        }
        let Some(id) = owner.target else {
            continue;
        };
        let owner_macro = node.macro_name.clone().unwrap_or_default();
        let owner_kind = format!("{:?}", node.kind).to_ascii_lowercase();
        let target = ExpectedTarget {
            normalized_id: document_id_slug(&id),
            explicit: owner.explicit,
            id: id.clone(),
            source_line: node.line,
            owner_macro: owner_macro.clone(),
            owner_kind: owner_kind.clone(),
        };
        // libmandoc often places one logical target on a block and one or
        // more of its head/body wrappers. They are one preservation
        // obligation, not multiple required IR anchors.
        targets.entry((id, node.line)).or_insert(target);
    }

    // A target can remain on an inline `.Tg` or on a visible inline macro.
    // It remains an obligation even though it has no structural owner.
    for id in explicit.keys() {
        if targets.values().any(|target| &target.id == id) {
            continue;
        }
        let source_line = explicit
            .get(id)
            .and_then(|lines| lines.iter().next().copied())
            .unwrap_or(0);
        targets.insert(
            (id.clone(), source_line),
            ExpectedTarget {
                normalized_id: document_id_slug(id),
                id: id.clone(),
                source_line,
                owner_macro: "Tg".to_owned(),
                owner_kind: "explicit".to_owned(),
                explicit: true,
            },
        );
    }

    NativeTargetProfile {
        expected: targets.into_values().collect(),
        owner_count,
        owner_classes: classes
            .into_iter()
            .map(
                |((owner_macro, owner_kind, disposition, reason), count)| OwnerClass {
                    owner_macro,
                    owner_kind,
                    disposition: disposition.as_str(),
                    reason,
                    count,
                },
            )
            .collect(),
        unclassified,
    }
}

fn classify_target_owner(
    node: &Node,
    explicit: &BTreeMap<String, BTreeSet<u32>>,
) -> ClassifiedOwner {
    let target = target_name(node);
    let is_explicit = target
        .as_ref()
        .is_some_and(|target| explicit.contains_key(target));
    let owner_macro = node
        .macro_name
        .clone()
        .unwrap_or_else(|| "<none>".to_owned());
    let owner_kind = format!("{:?}", node.kind).to_ascii_lowercase();
    let (disposition, reason) =
        if matches!(owner_macro.as_str(), "SH" | "SS" | "Sh" | "Ss") && !is_explicit {
            (
                OwnerDisposition::Excluded,
                "section uses the complete visible heading as its normalized identity",
            )
        } else if target.is_none() {
            (
                OwnerDisposition::Unclassified,
                "deep-link owner has no validated target name",
            )
        } else if is_explicit || owner_macro == "Tg" {
            (OwnerDisposition::Retained, "source-authored Tg destination")
        } else if matches!(
            owner_macro.as_str(),
            "IP" | "TP"
                | "TQ"
                | "Pp"
                | "Bd"
                | "D1"
                | "Dl"
                | "Bl"
                | "It"
                | "Fo"
                | "Fn"
                | "Fl"
                | "Cm"
                | "Em"
                | "No"
                | "Sy"
                | "Dv"
                | "Ic"
                | "Li"
                | "Er"
                | "Va"
                | "Ev"
        ) {
            (
                OwnerDisposition::Retained,
                "validated non-section navigation destination",
            )
        } else {
            (
                OwnerDisposition::Unclassified,
                "owner macro has no target-conservation policy",
            )
        };
    ClassifiedOwner {
        target,
        source_line: node.line,
        owner_macro,
        owner_kind,
        explicit: is_explicit,
        disposition,
        reason,
    }
}

fn explicit_targets(nodes: &[&Node]) -> BTreeMap<String, BTreeSet<u32>> {
    let mut targets = BTreeMap::<String, BTreeSet<u32>>::new();
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
            targets.entry(target).or_default().insert(node.line);
        }
    }
    targets
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

fn observed_targets(document: &Document) -> ObservedTargets {
    let mut observed = ObservedTargets::default();
    observed
        .fragments
        .extend(document.fragment_aliases.iter().map(ToString::to_string));
    collect_blocks(&document.blocks, &mut observed);
    for section in &document.sections {
        collect_section(section, &mut observed);
    }
    observed
}

fn collect_section(section: &Section, observed: &mut ObservedTargets) {
    observed.identities.insert(section.id.to_string());
    observed.sections.insert(section.id.to_string());
    observed
        .fragments
        .extend(section.fragment_aliases.iter().map(ToString::to_string));
    collect_blocks(&section.blocks, observed);
    for child in &section.children {
        collect_section(child, observed);
    }
}

fn collect_blocks(blocks: &[Block], observed: &mut ObservedTargets) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                collect_inlines(children, observed);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.blocks, observed);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    if let Some(identity) = &item.identity {
                        observed.identities.insert(identity.id.to_string());
                        observed.entries.insert(identity.id.to_string());
                    }
                    for term in &item.terms {
                        collect_inlines(term, observed);
                    }
                    collect_blocks(&item.description, observed);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_blocks(&cell.blocks, observed);
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_inlines(nodes: &[Inline], observed: &mut ObservedTargets) {
    for node in nodes {
        match node {
            Inline::Anchor {
                id,
                fragment_aliases,
            } => {
                observed.identities.insert(id.to_string());
                observed.anchors.insert(id.to_string());
                observed
                    .fragments
                    .extend(fragment_aliases.iter().map(ToString::to_string));
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let Inline::Link {
                    target: mant_ir::LinkTarget::Section { id },
                    ..
                } = node
                {
                    observed.section_links.insert(id.to_string());
                }
                collect_inlines(children, observed);
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
}

fn unexpected_targets(expected: &[ExpectedTarget], observed: &ObservedTargets) -> Vec<String> {
    let mut unexpected = BTreeSet::new();
    for fragment in &observed.fragments {
        if !expected
            .iter()
            .any(|target| target.explicit && target.id == *fragment)
        {
            unexpected.insert(format!("fragment alias {fragment:?}"));
        }
    }
    for anchor in &observed.anchors {
        if observed.entries.contains(anchor)
            || expected.iter().any(|target| {
                target.id == *anchor || generated_identity_matches(&target.normalized_id, anchor)
            })
        {
            continue;
        }
        unexpected.insert(format!("anchor {anchor:?}"));
    }
    unexpected.into_iter().collect()
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
