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
use mant_ir::{Block, Document, FragmentAlias, Inline, Section};
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-target-profile/v3";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TargetRole {
    Section,
    Anchor,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedTarget {
    id: String,
    normalized_id: String,
    source_line: u32,
    owner_source_line: u32,
    owner_macro: String,
    owner_kind: String,
    ast_path: String,
    logical_owner_key: String,
    section_ordinal: usize,
    section_source_line: u32,
    expected_role: TargetRole,
    expected_container: &'static str,
    explicit: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ObservedRole {
    Document,
    Section,
    Entry,
    Anchor,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservedTarget {
    identity: String,
    fragment_aliases: Vec<String>,
    role: ObservedRole,
    container: &'static str,
    section_ordinal: usize,
    section_source_line: u32,
    ir_path: String,
}

#[derive(Clone, Copy)]
struct SectionPosition {
    ordinal: usize,
    source_line: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchedTarget {
    logical_owner_key: String,
    observed_ir_path: String,
    observed_identity: String,
    matched_by: &'static str,
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
    expected_role: TargetRole,
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
    ast_path: String,
    logical_owner_key: String,
    raw_owner_count: usize,
    reason: &'static str,
}

#[derive(Default)]
struct ObservedTargets {
    occurrences: Vec<ObservedTarget>,
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
    let (missing, matched, used_observed) = if alias {
        (Vec::new(), Vec::new(), BTreeSet::new())
    } else {
        match_targets(&native_profile.expected, &observed.occurrences)
    };
    let unexpected_targets = if alias {
        Vec::new()
    } else {
        unexpected_targets(&observed, &used_observed)
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
        "classifiedOwnerCount": native_profile.classified_owner_count,
        "logicalOwnerCount": native_profile.logical_owner_count,
        "classifiedLogicalOwnerCount": native_profile.classified_logical_owner_count,
        "ownerClasses": native_profile.owner_classes,
        "unclassifiedOwners": native_profile.unclassified,
        "observed": observed_spellings,
        "observedOccurrences": observed.occurrences,
        "matched": matched,
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
    classified_owner_count: usize,
    logical_owner_count: usize,
    classified_logical_owner_count: usize,
    owner_classes: Vec<OwnerClass>,
    unclassified: Vec<UnclassifiedOwner>,
}

struct AstNodeRef<'a> {
    node: &'a Node,
    path: String,
    section_heading: bool,
    section_ordinal: usize,
    section_source_line: u32,
    order: usize,
}

#[derive(Clone)]
struct ExplicitTarget {
    id: String,
    source_line: u32,
    ast_path: String,
    section_ordinal: usize,
    section_source_line: u32,
    order: usize,
}

struct LogicalOwner {
    target: Option<String>,
    owner_source_line: u32,
    owner_macro: String,
    owner_kind: String,
    ast_path: String,
    section_heading: bool,
    section_ordinal: usize,
    section_source_line: u32,
    order: usize,
    raw_owner_count: usize,
    explicit: Option<ExplicitTarget>,
}

fn native_target_profile(root: &Node) -> NativeTargetProfile {
    let mut flattened = Vec::new();
    let mut next_section_ordinal = 0;
    flatten_nodes(
        root,
        "0",
        0,
        0,
        false,
        &mut next_section_ordinal,
        &mut flattened,
    );
    let explicit = explicit_targets(&flattened);
    let (mut owners, owner_count) = logical_owners(&flattened);
    let unmatched_explicit = bind_explicit_targets(&mut owners, &explicit);
    assemble_native_profile(owners, unmatched_explicit, owner_count)
}

fn logical_owners(flattened: &[AstNodeRef<'_>]) -> (Vec<LogicalOwner>, usize) {
    let mut grouped = BTreeMap::<(String, Option<String>), LogicalOwner>::new();
    let mut owner_count = 0;
    for reference in flattened {
        if !reference.node.flags.deep_link_target {
            continue;
        }
        owner_count += 1;
        let target = target_name(reference.node);
        let logical_path = logical_owner_path(reference);
        let key = (logical_path.clone(), target.clone());
        grouped
            .entry(key)
            .and_modify(|owner| {
                owner.raw_owner_count += 1;
                owner.section_heading |= reference.section_heading;
            })
            .or_insert_with(|| LogicalOwner {
                target,
                owner_source_line: reference.node.line,
                owner_macro: reference
                    .node
                    .macro_name
                    .clone()
                    .unwrap_or_else(|| "<none>".to_owned()),
                owner_kind: format!("{:?}", reference.node.kind).to_ascii_lowercase(),
                ast_path: logical_path,
                section_heading: reference.section_heading,
                section_ordinal: reference.section_ordinal,
                section_source_line: reference.section_source_line,
                order: reference.order,
                raw_owner_count: 1,
                explicit: None,
            });
    }
    let mut owners = grouped.into_values().collect::<Vec<_>>();
    owners.sort_by_key(|owner| owner.order);
    (owners, owner_count)
}

fn bind_explicit_targets(
    owners: &mut [LogicalOwner],
    explicit: &[ExplicitTarget],
) -> Vec<ExplicitTarget> {
    let mut unmatched_explicit = Vec::new();
    for (index, target) in explicit.iter().cloned().enumerate() {
        let next_explicit_order = explicit
            .get(index + 1)
            .map_or(usize::MAX, |next| next.order);
        let containing_owner = owners
            .iter_mut()
            .filter(|owner| {
                owner.explicit.is_none()
                    && owner.target.as_deref() == Some(target.id.as_str())
                    && target.ast_path.starts_with(&format!("{}.", owner.ast_path))
            })
            .max_by_key(|owner| owner.ast_path.len());
        if let Some(owner) = containing_owner {
            owner.explicit = Some(target);
            continue;
        }
        let following_owner = owners
            .iter_mut()
            .filter(|owner| {
                owner.explicit.is_none()
                    && owner.target.as_deref() == Some(target.id.as_str())
                    && owner.order >= target.order
                    && owner.order < next_explicit_order
            })
            .min_by_key(|owner| owner.order);
        if let Some(owner) = following_owner {
            owner.explicit = Some(target);
        } else {
            unmatched_explicit.push(target);
        }
    }
    unmatched_explicit
}

fn assemble_native_profile(
    owners: Vec<LogicalOwner>,
    unmatched_explicit: Vec<ExplicitTarget>,
    owner_count: usize,
) -> NativeTargetProfile {
    let logical_owner_count = owners.len() + unmatched_explicit.len();
    let mut targets = Vec::new();
    let mut classes = BTreeMap::<(String, String, OwnerDisposition, &'static str), usize>::new();
    let mut unclassified = Vec::new();
    let mut unclassified_raw_count = 0;

    for logical in owners {
        let owner = classify_target_owner(&logical);
        *classes
            .entry((
                owner.owner_macro.clone(),
                owner.owner_kind.clone(),
                owner.disposition,
                owner.reason,
            ))
            .or_default() += logical.raw_owner_count;
        if owner.disposition == OwnerDisposition::Unclassified {
            unclassified_raw_count += logical.raw_owner_count;
            unclassified.push(UnclassifiedOwner {
                target: owner.target,
                source_line: owner.source_line,
                owner_macro: owner.owner_macro,
                owner_kind: owner.owner_kind,
                ast_path: logical.ast_path.clone(),
                logical_owner_key: logical_owner_key(&logical),
                raw_owner_count: logical.raw_owner_count,
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
        let expected_container = expected_container(&owner.owner_macro);
        targets.push(ExpectedTarget {
            normalized_id: document_id_slug(&id),
            explicit: owner.explicit,
            id,
            source_line: logical
                .explicit
                .as_ref()
                .map_or(logical.owner_source_line, |target| target.source_line),
            owner_source_line: logical.owner_source_line,
            owner_macro: owner.owner_macro,
            owner_kind: owner.owner_kind,
            ast_path: logical.ast_path.clone(),
            logical_owner_key: logical_owner_key(&logical),
            section_ordinal: logical.section_ordinal,
            section_source_line: logical.section_source_line,
            expected_role: owner.expected_role,
            expected_container,
        });
    }

    for target in unmatched_explicit {
        let normalized_id = document_id_slug(&target.id);
        targets.push(ExpectedTarget {
            id: target.id,
            normalized_id,
            source_line: target.source_line,
            owner_source_line: target.source_line,
            owner_macro: "Tg".to_owned(),
            owner_kind: "element".to_owned(),
            ast_path: target.ast_path.clone(),
            logical_owner_key: format!("tg:{}", target.ast_path),
            section_ordinal: target.section_ordinal,
            section_source_line: target.section_source_line,
            expected_role: TargetRole::Anchor,
            expected_container: "content",
            explicit: true,
        });
    }

    let classified_logical_owner_count = logical_owner_count - unclassified.len();
    NativeTargetProfile {
        expected: targets,
        owner_count,
        classified_owner_count: owner_count - unclassified_raw_count,
        logical_owner_count,
        classified_logical_owner_count,
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

fn classify_target_owner(logical: &LogicalOwner) -> ClassifiedOwner {
    let target = logical
        .explicit
        .as_ref()
        .map(|target| target.id.clone())
        .or_else(|| logical.target.clone());
    let is_explicit = logical.explicit.is_some();
    let owner_macro = logical.owner_macro.clone();
    let owner_kind = logical.owner_kind.clone();
    let is_section_owner =
        logical.section_heading || matches!(owner_macro.as_str(), "SH" | "SS" | "Sh" | "Ss");
    let expected_role = if is_section_owner {
        TargetRole::Section
    } else {
        TargetRole::Anchor
    };
    let (disposition, reason) = if is_section_owner && !is_explicit {
        (
            OwnerDisposition::Excluded,
            "section uses the complete visible heading as its normalized identity",
        )
    } else if owner_macro == "Tg" && target.is_none() {
        (
            OwnerDisposition::Excluded,
            "argument-less Tg delegates its destination to the following owner",
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
            | "Ms"
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
        source_line: logical.owner_source_line,
        owner_macro,
        owner_kind,
        explicit: is_explicit,
        expected_role,
        disposition,
        reason,
    }
}

fn explicit_targets(nodes: &[AstNodeRef<'_>]) -> Vec<ExplicitTarget> {
    let mut targets = Vec::new();
    for (index, reference) in nodes.iter().enumerate() {
        if reference.node.macro_name.as_deref() != Some("Tg") {
            continue;
        }
        let target = explicit_target_argument(reference.node).or_else(|| {
            nodes[index + 1..]
                .iter()
                .filter(|candidate| candidate.node.line > reference.node.line)
                .find_map(|candidate| source_token(candidate.node))
        });
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            targets.push(ExplicitTarget {
                id: target,
                source_line: reference.node.line,
                ast_path: reference.path.clone(),
                section_ordinal: reference.section_ordinal,
                section_source_line: reference.section_source_line,
                order: reference.order,
            });
        }
    }
    targets
}

fn logical_owner_path(reference: &AstNodeRef<'_>) -> String {
    logical_owner_path_for(&reference.path, reference.node.kind)
}

fn logical_owner_path_for(path: &str, kind: NodeKind) -> String {
    if matches!(kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) {
        path.rsplit_once('.')
            .map_or_else(|| path.to_owned(), |(parent, _)| parent.to_owned())
    } else {
        path.to_owned()
    }
}

fn logical_owner_key(owner: &LogicalOwner) -> String {
    format!(
        "{}:{}:{}",
        owner.section_ordinal,
        owner.ast_path,
        owner.target.as_deref().unwrap_or("<missing>")
    )
}

fn expected_container(owner_macro: &str) -> &'static str {
    match owner_macro {
        "SH" | "SS" | "Sh" | "Ss" => "section",
        "IP" | "TP" | "TQ" | "It" => "item",
        _ => "content",
    }
}

fn target_name(node: &Node) -> Option<String> {
    if node.macro_name.as_deref() == Some("Tg") {
        return explicit_target_argument(node);
    }
    node.tag
        .as_deref()
        .map(str::to_owned)
        .or_else(|| source_token(node))
        .filter(|target| !target.is_empty())
}

fn explicit_target_argument(node: &Node) -> Option<String> {
    if node.macro_name.as_deref() != Some("Tg") {
        return None;
    }
    first_text_on_line(node, node.line)
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(str::to_owned)
}

fn source_token(node: &Node) -> Option<String> {
    first_text(node)
        .map(first_token)
        .filter(|target| !target.is_empty())
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text && !node.flags.no_print {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

fn first_text_on_line(node: &Node, line: u32) -> Option<&str> {
    if node.kind == NodeKind::Text && node.line == line {
        return node.text.as_deref();
    }
    node.children
        .iter()
        .find_map(|child| first_text_on_line(child, line))
}

fn first_token(value: &str) -> String {
    value
        .trim_start_matches('-')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn flatten_nodes<'a>(
    node: &'a Node,
    path: &str,
    parent_section_line: u32,
    parent_section_ordinal: usize,
    inside_section_heading: bool,
    next_section_ordinal: &mut usize,
    output: &mut Vec<AstNodeRef<'a>>,
) {
    let is_section = node.kind == NodeKind::Block
        && matches!(node.macro_name.as_deref(), Some("SH" | "SS" | "Sh" | "Ss"));
    let section_source_line = if is_section {
        node.line
    } else {
        parent_section_line
    };
    let section_ordinal = if is_section {
        *next_section_ordinal += 1;
        *next_section_ordinal
    } else {
        parent_section_ordinal
    };
    let section_heading = inside_section_heading
        || (node.kind == NodeKind::Head
            && matches!(node.macro_name.as_deref(), Some("SH" | "SS" | "Sh" | "Ss")));
    let order = output.len();
    output.push(AstNodeRef {
        node,
        path: path.to_owned(),
        section_heading,
        section_ordinal,
        section_source_line,
        order,
    });
    for (index, child) in node.children.iter().enumerate() {
        flatten_nodes(
            child,
            &format!("{path}.{index}"),
            section_source_line,
            section_ordinal,
            section_heading,
            next_section_ordinal,
            output,
        );
    }
}

fn observed_targets(document: &Document) -> ObservedTargets {
    let mut observed = ObservedTargets::default();
    let root_position = SectionPosition {
        ordinal: 0,
        source_line: 0,
    };
    record_observed(
        &mut observed,
        "document",
        &document.fragment_aliases,
        ObservedRole::Document,
        "document",
        root_position,
        "document".to_owned(),
    );
    collect_blocks(
        &document.blocks,
        &mut observed,
        root_position,
        "document",
        "content",
    );
    let mut next_section_ordinal = 0;
    for (index, section) in document.sections.iter().enumerate() {
        collect_section(
            section,
            &mut observed,
            &format!("section[{index}]"),
            &mut next_section_ordinal,
        );
    }
    observed
}

fn record_observed(
    observed: &mut ObservedTargets,
    identity: &str,
    fragment_aliases: &[FragmentAlias],
    role: ObservedRole,
    container: &'static str,
    section: SectionPosition,
    ir_path: String,
) {
    let fragment_aliases = fragment_aliases
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    observed.identities.insert(identity.to_owned());
    observed.fragments.extend(fragment_aliases.iter().cloned());
    match role {
        ObservedRole::Section => {
            observed.sections.insert(identity.to_owned());
        }
        ObservedRole::Entry => {
            observed.entries.insert(identity.to_owned());
        }
        ObservedRole::Anchor => {
            observed.anchors.insert(identity.to_owned());
        }
        ObservedRole::Document => {}
    }
    observed.occurrences.push(ObservedTarget {
        identity: identity.to_owned(),
        fragment_aliases,
        role,
        container,
        section_ordinal: section.ordinal,
        section_source_line: section.source_line,
        ir_path,
    });
}

fn collect_section(
    section: &Section,
    observed: &mut ObservedTargets,
    path: &str,
    next_section_ordinal: &mut usize,
) {
    *next_section_ordinal += 1;
    let section_position = SectionPosition {
        ordinal: *next_section_ordinal,
        source_line: section.source.map_or(0, |source| source.line),
    };
    record_observed(
        observed,
        section.id.as_str(),
        &section.fragment_aliases,
        ObservedRole::Section,
        "section",
        section_position,
        path.to_owned(),
    );
    observed.identities.insert(section.id.to_string());
    collect_blocks(&section.blocks, observed, section_position, path, "content");
    for (index, child) in section.children.iter().enumerate() {
        collect_section(
            child,
            observed,
            &format!("{path}/section[{index}]"),
            next_section_ordinal,
        );
    }
}

fn collect_blocks(
    blocks: &[Block],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    owner_container: &'static str,
) {
    for (block_index, block) in blocks.iter().enumerate() {
        let path = format!("{parent_path}/block[{block_index}]");
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                let container = if owner_container == "content" {
                    match block {
                        Block::Paragraph { .. } => "paragraph",
                        Block::Preformatted { .. } => "preformatted",
                        _ => unreachable!(),
                    }
                } else {
                    owner_container
                };
                collect_inlines(children, observed, section, &path, container);
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.blocks,
                        observed,
                        section,
                        &format!("{path}/item[{item_index}]"),
                        "list-item",
                    );
                }
            }
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let item_path = format!("{path}/definition[{item_index}]");
                    if let Some(identity) = &item.identity {
                        record_observed(
                            observed,
                            identity.id.as_str(),
                            &[],
                            ObservedRole::Entry,
                            "definition",
                            section,
                            item_path.clone(),
                        );
                    }
                    for (term_index, term) in item.terms.iter().enumerate() {
                        collect_inlines(
                            term,
                            observed,
                            section,
                            &format!("{item_path}/term[{term_index}]"),
                            "definition",
                        );
                    }
                    collect_blocks(
                        &item.description,
                        observed,
                        section,
                        &format!("{item_path}/description"),
                        "definition",
                    );
                }
            }
            Block::Table { rows, .. } => {
                for (row_index, row) in rows.iter().enumerate() {
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        collect_blocks(
                            &cell.blocks,
                            observed,
                            section,
                            &format!("{path}/row[{row_index}]/cell[{cell_index}]"),
                            "table-cell",
                        );
                    }
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_inlines(
    nodes: &[Inline],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    container: &'static str,
) {
    for (index, node) in nodes.iter().enumerate() {
        let path = format!("{parent_path}/inline[{index}]");
        match node {
            Inline::Anchor {
                id,
                fragment_aliases,
            } => {
                record_observed(
                    observed,
                    id.as_str(),
                    fragment_aliases,
                    ObservedRole::Anchor,
                    container,
                    section,
                    path,
                );
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
                collect_inlines(children, observed, section, &path, container);
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
}

fn match_targets(
    expected: &[ExpectedTarget],
    observed: &[ObservedTarget],
) -> (Vec<ExpectedTarget>, Vec<MatchedTarget>, BTreeSet<String>) {
    let mut alias_index = BTreeMap::<(usize, String), Vec<(usize, usize)>>::new();
    let mut anchors_by_section = BTreeMap::<usize, Vec<usize>>::new();
    for (index, candidate) in observed.iter().enumerate() {
        for (fragment_index, fragment) in candidate.fragment_aliases.iter().enumerate() {
            alias_index
                .entry((candidate.section_ordinal, fragment.clone()))
                .or_default()
                .push((index, fragment_index));
        }
        if candidate.role == ObservedRole::Anchor {
            anchors_by_section
                .entry(candidate.section_ordinal)
                .or_default()
                .push(index);
        }
    }

    let mut used = BTreeSet::<String>::new();
    let mut assignments = vec![None; expected.len()];

    for (expected_index, target) in expected
        .iter()
        .enumerate()
        .filter(|(_, target)| target.explicit)
    {
        let candidates = alias_index
            .get(&(target.section_ordinal, target.id.clone()))
            .map_or(&[][..], Vec::as_slice);
        if let Some((index, fragment_index)) =
            candidates.iter().copied().find(|(index, fragment_index)| {
                let candidate = &observed[*index];
                role_matches(target.expected_role, candidate.role)
                    && container_matches(target.expected_container, candidate.container)
                    && !used.contains(&format!("{index}:identity"))
                    && !used.contains(&format!("{index}:fragment:{fragment_index}"))
            })
        {
            used.insert(format!("{index}:fragment:{fragment_index}"));
            used.insert(format!("{index}:identity"));
            assignments[expected_index] =
                Some(matched_target(target, &observed[index], "fragment-alias"));
        } else if target.id == target.normalized_id {
            let candidate = observed.iter().enumerate().find(|(index, candidate)| {
                candidate.section_ordinal == target.section_ordinal
                    && candidate.identity == target.id
                    && role_matches(target.expected_role, candidate.role)
                    && container_matches(target.expected_container, candidate.container)
                    && !used.contains(&format!("{index}:identity"))
            });
            if let Some((index, candidate)) = candidate {
                used.insert(format!("{index}:identity"));
                assignments[expected_index] =
                    Some(matched_target(target, candidate, "canonical-identity"));
            }
        }
    }

    let mut section_cursors = BTreeMap::<usize, usize>::new();
    for (expected_index, target) in expected.iter().enumerate() {
        if target.explicit || assignments[expected_index].is_some() {
            continue;
        }
        let candidates = anchors_by_section
            .get(&target.section_ordinal)
            .map_or(&[][..], Vec::as_slice);
        let cursor = section_cursors.entry(target.section_ordinal).or_default();
        let candidate = candidates[*cursor..]
            .iter()
            .enumerate()
            .find_map(|(offset, index)| {
                let index = *index;
                let candidate = &observed[index];
                let identity_claim = format!("{index}:identity");
                let fragment_prefix = format!("{index}:fragment:");
                (container_matches(target.expected_container, candidate.container)
                    && !used.contains(&identity_claim)
                    && !used.iter().any(|claim| claim.starts_with(&fragment_prefix))
                    && generated_identity_matches(&target.normalized_id, &candidate.identity))
                .then_some((offset, index))
            });
        if let Some((offset, index)) = candidate {
            *cursor += offset + 1;
            used.insert(format!("{index}:identity"));
            assignments[expected_index] = Some(matched_target(
                target,
                &observed[index],
                "ordered-identity-occurrence",
            ));
        }
    }

    let mut missing = Vec::new();
    let mut confirmed = Vec::new();
    for (target, candidate) in expected.iter().zip(assignments) {
        if let Some(candidate) = candidate {
            confirmed.push(candidate);
        } else {
            missing.push(target.clone());
        }
    }
    (missing, confirmed, used)
}

fn matched_target(
    expected: &ExpectedTarget,
    observed: &ObservedTarget,
    matched_by: &'static str,
) -> MatchedTarget {
    MatchedTarget {
        logical_owner_key: expected.logical_owner_key.clone(),
        observed_ir_path: observed.ir_path.clone(),
        observed_identity: observed.identity.clone(),
        matched_by,
    }
}

fn container_matches(expected: &str, observed: &str) -> bool {
    match expected {
        "section" => observed == "section",
        "item" => matches!(observed, "definition" | "list-item" | "table-cell"),
        "content" => observed != "section" && observed != "document",
        _ => false,
    }
}

const fn role_matches(expected: TargetRole, observed: ObservedRole) -> bool {
    matches!(
        (expected, observed),
        (TargetRole::Section, ObservedRole::Section) | (TargetRole::Anchor, ObservedRole::Anchor)
    )
}

fn unexpected_targets(observed: &ObservedTargets, used: &BTreeSet<String>) -> Vec<String> {
    let mut unexpected = Vec::new();
    for (index, target) in observed.occurrences.iter().enumerate() {
        for (fragment_index, fragment) in target.fragment_aliases.iter().enumerate() {
            let claim = format!("{index}:fragment:{fragment_index}");
            if !used.contains(&claim) {
                unexpected.push(format!(
                    "fragment alias {fragment:?} on {} {}",
                    target.container, target.ir_path
                ));
            }
        }
        if target.role == ObservedRole::Anchor
            && !observed.entries.contains(&target.identity)
            && !used.contains(&format!("{index}:identity"))
        {
            unexpected.push(format!(
                "anchor {:?} in {} {}",
                target.identity, target.container, target.ir_path
            ));
        }
    }
    unexpected
}

fn generated_identity_matches(base: &str, candidate: &str) -> bool {
    candidate == base || collision_base(candidate) == Some(base)
}

fn collision_base(candidate: &str) -> Option<&str> {
    let (base, suffix) = candidate.rsplit_once('-')?;
    let value = suffix.parse::<usize>().ok()?;
    (value >= 2 && suffix == value.to_string()).then_some(base)
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

#[cfg(test)]
mod tests {
    use libmandoc_rs::{Node, NodeFlags, NodeKind};

    use super::{
        ExpectedTarget, ExplicitTarget, LogicalOwner, ObservedRole, ObservedTarget,
        OwnerDisposition, TargetRole, bind_explicit_targets, classify_target_owner,
        generated_identity_matches, logical_owner_path_for, match_targets, native_target_profile,
    };

    fn node(
        kind: NodeKind,
        macro_name: Option<&str>,
        text: Option<&str>,
        tag: Option<&str>,
        line: u32,
        flags: NodeFlags,
        children: Vec<Node>,
    ) -> Node {
        Node {
            kind,
            macro_name: macro_name.map(ToOwned::to_owned),
            text: text.map(ToOwned::to_owned),
            tag: tag.map(ToOwned::to_owned),
            line,
            column: 1,
            flags,
            list_kind: None,
            display_kind: None,
            font: None,
            author_mode: None,
            enclosure: None,
            compact: false,
            offset: None,
            width: None,
            table_cells: Vec::new(),
            equation: None,
            children,
        }
    }

    fn root(children: Vec<Node>) -> Node {
        node(
            NodeKind::Root,
            None,
            None,
            None,
            0,
            NodeFlags::default(),
            children,
        )
    }

    #[test]
    fn target_profile_uses_authored_tg_arguments_instead_of_stale_tags() {
        let argument = node(
            NodeKind::Text,
            None,
            Some("--Exact.Target"),
            None,
            7,
            NodeFlags {
                no_print: true,
                ..NodeFlags::default()
            },
            Vec::new(),
        );
        let target = node(
            NodeKind::Element,
            Some("Tg"),
            None,
            Some("stale-automatic-target"),
            7,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            vec![argument],
        );

        let profile = native_target_profile(&root(vec![target]));
        assert!(profile.unclassified.is_empty());
        assert_eq!(profile.expected.len(), 1);
        assert_eq!(profile.expected[0].id, "--Exact.Target");
        assert!(profile.expected[0].explicit);
    }

    #[test]
    fn target_profile_binds_argumentless_tg_to_the_following_source_owner() {
        let request = node(
            NodeKind::Element,
            Some("Tg"),
            None,
            Some("stale-automatic-target"),
            7,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            Vec::new(),
        );
        let derived_text = node(
            NodeKind::Text,
            None,
            Some("derived-target"),
            None,
            8,
            NodeFlags::default(),
            Vec::new(),
        );
        let derived = node(
            NodeKind::Element,
            Some("Sy"),
            None,
            Some("derived-target"),
            8,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            vec![derived_text],
        );

        let profile = native_target_profile(&root(vec![request, derived]));
        assert!(profile.unclassified.is_empty());
        assert_eq!(profile.expected.len(), 1);
        assert_eq!(profile.expected[0].id, "derived-target");
        assert_eq!(profile.expected[0].owner_macro, "Sy");
        assert!(profile.expected[0].explicit);
    }

    fn expected(
        id: &str,
        explicit: bool,
        role: TargetRole,
        container: &'static str,
    ) -> ExpectedTarget {
        ExpectedTarget {
            id: id.to_owned(),
            normalized_id: id.to_owned(),
            source_line: 10,
            owner_source_line: 11,
            owner_macro: if role == TargetRole::Section {
                "Sh"
            } else {
                "Pp"
            }
            .to_owned(),
            owner_kind: "element".to_owned(),
            ast_path: format!("0.{id}"),
            logical_owner_key: format!("1:0.{id}:{id}"),
            section_ordinal: 1,
            section_source_line: 1,
            expected_role: role,
            expected_container: container,
            explicit,
        }
    }

    fn observed(
        id: &str,
        aliases: &[&str],
        role: ObservedRole,
        container: &'static str,
    ) -> ObservedTarget {
        ObservedTarget {
            identity: id.to_owned(),
            fragment_aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            role,
            container,
            section_ordinal: 1,
            section_source_line: 1,
            ir_path: format!("section[0]/{container}/{id}"),
        }
    }

    fn logical_owner(id: &str, ast_path: &str) -> LogicalOwner {
        LogicalOwner {
            target: Some(id.to_owned()),
            owner_source_line: 11,
            owner_macro: "It".to_owned(),
            owner_kind: "head".to_owned(),
            ast_path: ast_path.to_owned(),
            section_heading: false,
            section_ordinal: 1,
            section_source_line: 1,
            order: 10,
            raw_owner_count: 1,
            explicit: None,
        }
    }

    #[test]
    fn exact_generated_suffixes_are_numeric_and_canonical() {
        for identity in ["target", "target-2", "target-31"] {
            assert!(generated_identity_matches("target", identity));
        }
        for identity in [
            "target-1",
            "target-02",
            "target-a",
            "target-deadbeef",
            "target-123abc",
        ] {
            assert!(!generated_identity_matches("target", identity));
        }
    }

    #[test]
    fn each_same_named_explicit_owner_requires_its_own_alias_occurrence() {
        let expected = [
            expected("same", true, TargetRole::Anchor, "content"),
            expected("same", true, TargetRole::Anchor, "content"),
        ];
        let observed = [observed(
            "same",
            &["same"],
            ObservedRole::Anchor,
            "paragraph",
        )];
        let (missing, matched, _) = match_targets(&expected, &observed);
        assert_eq!(matched.len(), 1);
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn canonical_explicit_target_does_not_require_a_redundant_alias() {
        let target = expected("same", true, TargetRole::Anchor, "content");
        let candidate = observed("same", &[], ObservedRole::Anchor, "paragraph");
        let (missing, matched, _) = match_targets(&[target], &[candidate]);
        assert!(missing.is_empty());
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].matched_by, "canonical-identity");
    }

    #[test]
    fn explicit_and_automatic_owners_cannot_share_one_ir_location() {
        let expected = [
            expected("same", true, TargetRole::Anchor, "content"),
            expected("same", false, TargetRole::Anchor, "content"),
        ];
        let observed = [observed(
            "same",
            &["same"],
            ObservedRole::Anchor,
            "paragraph",
        )];
        let (missing, matched, _) = match_targets(&expected, &observed);
        assert_eq!(matched.len(), 1);
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn role_section_and_container_mismatches_do_not_satisfy_an_owner() {
        let target = expected("target", false, TargetRole::Anchor, "item");
        for candidate in [
            observed("target", &[], ObservedRole::Section, "section"),
            observed("target", &[], ObservedRole::Entry, "definition"),
            observed("target", &[], ObservedRole::Anchor, "paragraph"),
            ObservedTarget {
                section_ordinal: 99,
                ..observed("target", &[], ObservedRole::Anchor, "list-item")
            },
        ] {
            let (missing, matched, _) = match_targets(std::slice::from_ref(&target), &[candidate]);
            assert_eq!(missing.len(), 1);
            assert!(matched.is_empty());
        }
    }

    #[test]
    fn independent_same_named_owners_consume_distinct_numeric_identities() {
        let expected = [
            expected("same", false, TargetRole::Anchor, "content"),
            expected("same", false, TargetRole::Anchor, "content"),
        ];
        let observed = [
            observed("same", &[], ObservedRole::Anchor, "paragraph"),
            observed("same-2", &[], ObservedRole::Anchor, "paragraph"),
        ];
        let (missing, matched, _) = match_targets(&expected, &observed);
        assert!(missing.is_empty());
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn wrapper_roles_share_one_logical_owner_but_siblings_do_not() {
        assert_eq!(logical_owner_path_for("0.4.0", NodeKind::Head), "0.4");
        assert_eq!(logical_owner_path_for("0.4.1", NodeKind::Body), "0.4");
        assert_eq!(logical_owner_path_for("0.4.2", NodeKind::Tail), "0.4");
        assert_eq!(logical_owner_path_for("0.4.3", NodeKind::Element), "0.4.3");
        assert_ne!(
            logical_owner_path_for("0.4.0", NodeKind::Head),
            logical_owner_path_for("0.5.0", NodeKind::Head)
        );
    }

    #[test]
    fn nested_explicit_target_binds_to_its_containing_owner() {
        let mut owners = [logical_owner("same", "0.4")];
        let explicit = [ExplicitTarget {
            id: "same".to_owned(),
            source_line: 12,
            ast_path: "0.4.1.0".to_owned(),
            section_ordinal: 1,
            section_source_line: 1,
            order: 12,
        }];

        assert!(bind_explicit_targets(&mut owners, &explicit).is_empty());
        assert!(owners[0].explicit.is_some());
    }

    #[test]
    fn semantic_macro_inside_a_section_heading_is_not_a_second_owner() {
        let mut owner = logical_owner("function", "0.4.0.0");
        owner.owner_macro = "Fn".to_owned();
        owner.owner_kind = "element".to_owned();
        owner.section_heading = true;

        let classified = classify_target_owner(&owner);
        assert_eq!(classified.disposition, OwnerDisposition::Excluded);
        assert_eq!(classified.expected_role, TargetRole::Section);
    }

    #[test]
    fn mathematical_symbol_targets_are_retained_navigation_owners() {
        let mut owner = logical_owner("sigma", "0.4.1");
        owner.owner_macro = "Ms".to_owned();
        owner.owner_kind = "element".to_owned();

        let classified = classify_target_owner(&owner);
        assert_eq!(classified.disposition, OwnerDisposition::Retained);
        assert_eq!(classified.expected_role, TargetRole::Anchor);
        assert_eq!(classified.target.as_deref(), Some("sigma"));
    }
}
