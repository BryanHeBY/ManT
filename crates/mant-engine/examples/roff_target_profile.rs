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
    collections::BTreeSet,
    io::{self, BufRead, BufWriter, Write},
    path::PathBuf,
};

use libmandoc_rs::{Compression, IncludePolicy, ParseOptions, Parser};
use mant_engine::lower_mandoc_document;
use mant_ir::Document;
use serde::Serialize;
use serde_json::{Value, json};

const PROFILE_SCHEMA: &str = "mant.roff-target-profile/v4";

#[path = "roff_target_profile/matching.rs"]
mod matching;
#[path = "roff_target_profile/native.rs"]
mod native;
#[path = "roff_target_profile/observed.rs"]
mod observed;

#[cfg(test)]
use matching::generated_identity_matches;
use matching::{match_targets, unexpected_targets};
use native::native_target_profile;
#[cfg(test)]
use native::{
    ExplicitTarget, LogicalOwner, bind_explicit_targets, classify_target_owner,
    logical_owner_path_for,
};
use observed::observed_targets;

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
    owner_source_line: u32,
    owner_path: String,
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
        ObservedTargets, OwnerDisposition, TargetRole, bind_explicit_targets,
        classify_target_owner, generated_identity_matches, logical_owner_path_for, match_targets,
        native_target_profile, unexpected_targets,
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
            definition_list_style: None,
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

    #[test]
    fn target_profile_binds_argumentless_tg_to_its_preceding_paragraph_owner() {
        let paragraph = node(
            NodeKind::Element,
            Some("Pp"),
            None,
            Some("group"),
            7,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            Vec::new(),
        );
        let prose = node(
            NodeKind::Text,
            None,
            Some("prose"),
            None,
            8,
            NodeFlags::default(),
            Vec::new(),
        );
        let request = node(
            NodeKind::Element,
            Some("Tg"),
            None,
            None,
            9,
            NodeFlags::default(),
            Vec::new(),
        );
        let target_text = node(
            NodeKind::Text,
            None,
            Some("group"),
            None,
            10,
            NodeFlags::default(),
            Vec::new(),
        );
        let target = node(
            NodeKind::Element,
            Some("Ic"),
            None,
            None,
            10,
            NodeFlags::default(),
            vec![target_text],
        );

        let profile = native_target_profile(&root(vec![paragraph, prose, request, target]));
        assert!(profile.unclassified.is_empty());
        assert_eq!(profile.expected.len(), 1);
        assert_eq!(profile.expected[0].id, "group");
        assert_eq!(profile.expected[0].owner_macro, "Pp");
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
            owner_source_line: 11,
            owner_path: format!("section[0]/{container}/owner"),
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
    fn target_on_a_same_kind_sibling_is_both_missing_and_unexpected() {
        let target = expected("same", true, TargetRole::Anchor, "item");
        let mut misplaced = observed("same", &["same"], ObservedRole::Anchor, "list-item");
        misplaced.owner_source_line = 22;
        misplaced.owner_path = "section[0]/block[0]/item[1]".to_owned();
        misplaced.ir_path = format!("{}/block[0]/inline[0]", misplaced.owner_path);
        let observed = ObservedTargets {
            occurrences: vec![misplaced.clone()],
            identities: ["same".to_owned()].into_iter().collect(),
            fragments: ["same".to_owned()].into_iter().collect(),
            anchors: ["same".to_owned()].into_iter().collect(),
            ..ObservedTargets::default()
        };

        let (missing, matched, used) = match_targets(&[target], &observed.occurrences);
        let unexpected = unexpected_targets(&observed, &used);

        assert_eq!(missing.len(), 1);
        assert!(matched.is_empty());
        assert!(!unexpected.is_empty());
        assert!(unexpected.iter().any(|finding| finding.contains("item[1]")));
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
            argumentless: false,
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
