//! Structure observation and loss-detection controls.
use std::collections::BTreeMap;

use super::{
    NoFillSourceLine, equation_visible_text, is_no_fill_row_text, is_zero_width_guard_line,
    retained_no_fill_rows,
};

#[path = "link_execution_tests.rs"]
mod link_execution_tests;
#[path = "link_tests.rs"]
mod link_tests;

pub(super) fn parsed_structure(
    source: &str,
) -> (super::AstStructure, super::AstTopology, super::Document) {
    let report = super::Parser::new(super::ParseOptions {
        includes: super::IncludePolicy::Deny,
        compression: super::Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, topology) = super::ast_profile(&report);
    let document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    (expected, topology, document)
}

#[test]
fn address_obligations_follow_the_target_operand() {
    // Pinned CVS man_UR_pre and mdoc_lk_pre preserve each first operand as
    // its own href, including mailto schemes, header fields and roff escapes.
    let man = concat!(
        ".TH VALIDATE 1 \"November 18, 2026\"\n.SH DESCRIPTION\n",
        ".UR mailto:team@example.test\nmail\n.UE\n",
        ".UR https://example.test\nweb\n.UE\n",
        ".MT other@example.test\ncontact\n.ME\n",
        ".UR mailto\\&:escaped@example.test\nescaped\n.UE\n",
        ".UR mailto:list@example.test?subject=Hello\nform\n.UE\n",
    );
    let mdoc = concat!(
        ".Dd November 18, 2026\n.Dt VALIDATE 1\n.Os\n",
        ".Sh NAME\n.Nm validate\n.Nd address audit\n.Sh DESCRIPTION\n",
        ".Lk mailto:team@example.test mail\n.Lk https://example.test web\n",
        ".Mt other@example.test\n.Lk mailto\\&:escaped@example.test escaped\n",
        ".Lk mailto:list@example.test?subject=Hello form\n",
    );
    for source in [man, mdoc] {
        let (expected, expected_topology, document) = parsed_structure(source);
        let (mut observed, topology) = super::ir_profile(&document);
        assert_eq!(expected.external_links, 2);
        assert_eq!(expected.email_links, 3);
        assert_eq!(expected.unclassified_address_links, 0);
        assert_eq!(observed.external_links, 2);
        assert_eq!(observed.email_links, 3);
        let compare = |observed: &super::IrStructure| {
            super::compare_structure(&expected, observed, &expected_topology, &topology)
        };
        assert!(compare(&observed).is_empty());

        observed.email_links = 2;
        assert_eq!(
            compare(&observed),
            ["email-links: expected at least 3, observed 2"]
        );
        observed.email_links = 3;
        observed.external_links = 1;
        assert!(
            compare(&observed)
                .iter()
                .any(|item| item.starts_with("external-links:"))
        );
    }
}

#[test]
fn expanded_address_links_at_one_coordinate_keep_both_obligations() {
    // The two link nodes share one coordinate in pinned CVS -Ttree, while
    // -Thtml emits their separate mailto and HTTPS destinations.
    let man = concat!(
        ".TH VALIDATE 1 \"November 18, 2026\"\n.SH DESCRIPTION\n.de LX\n",
        ".UR mailto:team@example.test\nmail\n.UE\n",
        ".UR https://example.test\nweb\n.UE\n..\n.LX\n",
    );
    let mdoc = concat!(
        ".Dd November 18, 2026\n.Dt VALIDATE 1\n.Os\n",
        ".Sh NAME\n.Nm validate\n.Nd address audit\n.Sh DESCRIPTION\n.de LX\n",
        ".Lk mailto:team@example.test mail\n.Lk https://example.test web\n..\n.LX\n",
    );
    for source in [man, mdoc] {
        let (expected, expected_topology, document) = parsed_structure(source);
        let (mut observed, topology) = super::ir_profile(&document);
        assert_eq!((expected.email_links, expected.external_links), (1, 1));
        assert!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology)
                .is_empty()
        );
        observed.external_links = 0;
        assert_eq!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology),
            ["external-links: expected at least 1, observed 0"]
        );
    }
}

#[test]
fn opaque_table_words_and_code_styles_are_not_equation_obligations() {
    // The exact source ran pristine ASCII/UTF8/HTML/tree/lint first.
    // roff_parseln excludes tbl from roff_eqndelim; code font is style.
    let (expected, expected_topology, document) = parsed_structure(
        ".TH AUDIT 1\n.SH BODY\n.EQ\ndelim $$\n.EN\n.TS\nl fCR.\n$x over 2$\n.TE\n\\f[CR]ordinary code\\fP\n",
    );
    let (observed, topology) = super::ir_profile(&document);
    assert_eq!(expected.table_equations, 0);
    assert_eq!(observed.table_equation_candidates, 0);
    assert_eq!(observed.inline_equation_candidates, 0);
    assert!(expected_topology.equations.is_empty());
    assert!(topology.equations.is_empty());
    assert!(
        super::compare_structure(&expected, &observed, &expected_topology, &topology).is_empty()
    );
}

#[test]
fn section_heading_links_are_counted_and_loss_is_detected() {
    let (expected, expected_topology, mut document) = parsed_structure(concat!(
        ".Dd September 11, 2026\n.Dt AUDIT 1\n.Os\n",
        ".Sh NAME\n.Nm audit\n.Nd heading link probe\n",
        ".Sh DESCRIPTION\n.Ss Eo\n.Xr printf 3\nReference\n.Ec\n.Pp\nVisible body.\n",
    ));
    let (observed, topology) = super::ir_profile(&document);
    assert_eq!(expected.manual_links, 1);
    assert_eq!(observed.manual_links, 1);
    assert!(
        super::compare_structure(&expected, &observed, &expected_topology, &topology).is_empty()
    );

    let heading = &mut document.sections[1].children[0].heading;
    // Preserve the words, but remove the typed link: topology must notice.
    heading.content = vec![super::Inline::Text {
        value: heading.plain_text(),
    }];
    let (mutated, topology) = super::ir_profile(&document);
    let violations = super::compare_structure(&expected, &mutated, &expected_topology, &topology);
    assert!(
        violations
            .iter()
            .any(|item| item.starts_with("manual-links:"))
    );
}

#[test]
fn nonpositive_rs_literals_do_not_require_positive_layout() {
    for argument in ["-7", "-2n", "-1.5m", "0", "+0n"] {
        let (expected, expected_topology, document) = parsed_structure(&format!(
            ".TH AUDIT 1\n.SH BODY\n.RS {argument}\n.PP\nVisible body.\n.RE\n"
        ));
        assert_eq!(expected.max_relative_indent_depth, 1, "{argument}");
        assert_eq!(expected.positive_relative_indent_scopes, 0, "{argument}");
        let (observed, topology) = super::ir_profile(&document);
        assert_eq!(observed.max_indent_columns, 0, "{argument}");
        assert!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology)
                .is_empty()
        );
    }
}

#[test]
fn positive_rs_loss_is_detected_even_below_a_negative_scope() {
    for scopes in [".RS 4\n", ".RS\n", ".RS -2\n.RS 4\n"] {
        let (expected, expected_topology, mut document) = parsed_structure(&format!(
            ".TH AUDIT 1\n.SH BODY\n{scopes}.PP\nVisible body.\n.RE\n"
        ));
        assert_eq!(expected.positive_relative_indent_scopes, 1, "{scopes}");
        let (observed, topology) = super::ir_profile(&document);
        assert!(observed.max_indent_columns > 0, "{scopes}");
        assert!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology)
                .is_empty()
        );
        for block in &mut document.sections[0].blocks {
            if let super::Block::Paragraph { layout, .. } = block {
                layout.indent_columns = 0;
            }
        }
        let (mutated, topology) = super::ir_profile(&document);
        assert_eq!(mutated.max_indent_columns, 0);
        let violations =
            super::compare_structure(&expected, &mutated, &expected_topology, &topology);
        assert!(
            violations
                .iter()
                .any(|item| item.starts_with("relative-indent:")),
            "{scopes}"
        );
    }
}

#[test]
fn unsupported_rs_arguments_retain_the_conservative_obligation() {
    for argument in ["-2q", "-1i", "-999999n", "-2+1", "NaN"] {
        let (expected, _, _) = parsed_structure(&format!(
            ".TH AUDIT 1\n.SH BODY\n.RS {argument}\n.PP\nVisible body.\n.RE\n"
        ));
        assert_eq!(expected.positive_relative_indent_scopes, 1, "{argument}");
    }
}

#[test]
fn punctuated_mdoc_definition_ordinals_compare_as_recovered_ordered_lists() {
    let (expected, topology, document) = parsed_structure(concat!(
        ".Dd September 12, 2026\n.Dt AUDIT 1\n.Os\n.Sh BODY\n",
        ".Bl -tag -width Ds\n.It 1.\nFirst.\n.It 2.\nSecond.\n.El\n"
    ));
    let (observed, observed_topology) = super::ir_profile(&document);
    assert_eq!(topology.lists[0].kind, super::ListTopologyKind::Generic);
    assert!(
        super::compare_structure(&expected, &observed, &topology, &observed_topology).is_empty()
    );
}

#[test]
fn nested_ordinal_source_findings_keep_their_original_owner() {
    use super::ordinal_labels::{OrdinalDisposition, cases::execution::source};
    use serde_json::json;
    let root = std::env::temp_dir();
    let path = root.join(format!(
        "mant-nested-ordinal-structure-{}.1",
        std::process::id()
    ));
    for case in source::nested() {
        std::fs::write(&path, &case.source).unwrap();
        let profile =
            super::profile_request(&json!({"id":case.name,"path":path,"root":root}).to_string());
        std::fs::remove_file(&path).unwrap();
        let profile = profile.unwrap();
        let uncertain = case.disposition == OrdinalDisposition::Uncertain;
        let positions = if uncertain {
            json!([[case.source_line, 2]])
        } else {
            json!([])
        };
        assert_eq!(profile["ordinalCasesForReview"], positions, "{}", case.name);
        if case.name.starts_with("head-") {
            // A parent HEAD's flattened output cannot satisfy its native
            // nested list owner's separate occurrence, even at the same text.
            let prefix = format!("list-topology at line {}:2:", case.source_line);
            assert!(
                profile["violations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|issue| { issue.as_str().unwrap().starts_with(&prefix) }),
                "{}\n{profile}",
                case.name
            );
        } else {
            assert_eq!(profile["violations"], json!([]), "{}\n{profile}", case.name);
        }
    }
}

#[test]
fn equation_payload_evidence_preserves_source_review_and_independent_census() {
    use super::ordinal_labels::{OrdinalDisposition, cases::execution};
    use serde_json::json;
    let root = std::env::temp_dir();
    let path = root.join(format!(
        "mant-ordinal-equation-evidence-{}.1",
        std::process::id()
    ));
    for case in execution::equation_edges() {
        std::fs::write(&path, &case.source).unwrap();
        let profile =
            super::profile_request(&json!({"id":case.name,"path":path,"root":root}).to_string());
        std::fs::remove_file(&path).unwrap();
        let profile = profile.unwrap();
        if case.disposition == OrdinalDisposition::Uncertain {
            assert_eq!(
                profile["ordinalCasesForReview"],
                json!([[8, 2]]),
                "{}",
                case.name
            );
        } else {
            assert_eq!(profile["ordinalCasesForReview"], json!([]), "{}", case.name);
        }
        assert!(
            !profile["violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| { issue.as_str().unwrap().contains("list-topology") }),
            "{}\n{profile}",
            case.name
        );
        let (expected, topology, mut document) = parsed_structure(&case.source);
        document.sections[1].blocks.clear();
        let (observed, actual) = super::ir_profile(&document);
        let issues = super::compare_structure(&expected, &observed, &topology, &actual);
        assert!(
            issues.iter().any(|issue| issue.contains("list-topology")),
            "{}\n{issues:?}",
            case.name
        );
        // Populated glyphless expressions are still counted with their
        // source owner and exact empty value, independently of ordinal doubt.
        assert_eq!(profile["violations"], json!([]), "{}\n{profile}", case.name);
    }
}

#[test]
fn source_execution_crossings_keep_evidence_and_owner_obligations() {
    use super::ordinal_labels::{OrdinalDisposition, cases::execution};
    use serde_json::json;
    let root = std::env::temp_dir();
    let path = root.join(format!(
        "mant-structure-source-execution-{}.1",
        std::process::id()
    ));
    for case in execution::cases()
        .chain(execution::source::cases())
        .chain(execution::source::generated())
    {
        std::fs::write(&path, &case.source).unwrap();
        let profile =
            super::profile_request(&json!({"id":case.name,"path":path,"root":root}).to_string());
        std::fs::remove_file(&path).unwrap();
        let profile = profile.unwrap();
        assert_eq!(profile["violations"], json!([]), "{}\n{profile}", case.name);
        let lists_at_source = |side: &str| {
            profile["topology"][side]["lists"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|list| list["sourceLine"] == case.source_line)
                .collect::<Vec<_>>()
        };
        let expected = lists_at_source("expected");
        let observed = lists_at_source("observed");
        let kind = if case.disposition == OrdinalDisposition::Retained {
            "definition"
        } else {
            "generic"
        };
        assert_eq!(
            observed,
            vec![&json!({"sourceLine":case.source_line,"kind":kind,"items":2})],
            "{}",
            case.name
        );
        if case.disposition == OrdinalDisposition::Uncertain {
            assert_eq!(
                profile["ordinalCasesForReview"],
                json!([[case.source_line, 2]])
            );
        } else {
            assert_eq!(profile["ordinalCasesForReview"], json!([]));
            assert_eq!(expected, observed, "{}", case.name);
        }

        let (expected, topology, mut document) = parsed_structure(&case.source);
        document.sections[1].blocks.clear();
        let (observed, actual) = super::ir_profile(&document);
        let violations = super::compare_structure(&expected, &observed, &topology, &actual);
        assert!(
            violations.iter().any(|line| line.contains("list-topology")),
            "{}\n{violations:?}",
            case.name
        );
    }
}

#[test]
fn independent_ordinal_truth_rejects_same_count_owner_corruption() {
    use super::ListTopologyKind::{Definition, Generic};
    for (source, recovered) in super::ordinal_labels::cases::inputs() {
        let (expected, topology, document) = parsed_structure(&source);
        let (observed, mut observed_topology) = super::ir_profile(&document);
        let (correct, wrong) = if recovered {
            (Generic, Definition)
        } else {
            (Definition, Generic)
        };
        assert_eq!(topology.lists[0].kind, correct, "{source}");
        let compare = |observed_topology: &super::IrTopology| {
            super::compare_structure(&expected, &observed, &topology, observed_topology)
        };
        let good = compare(&observed_topology);
        assert!(good.is_empty(), "{source}: {good:?}");
        observed_topology.lists[0].kind = wrong;
        let violations = compare(&observed_topology);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("list-topology")),
            "{source}: {violations:?}"
        );
    }
}

#[test]
fn ordinal_jsonl_keeps_confirmed_structure_separate_from_source_review() {
    use super::ordinal_labels::cases::{
        bare_zero_advance_with_empty_row, body_and_boundary_inputs, body_needs_review,
        generated_and_control_inputs, heads_needing_review,
    };
    use serde_json::json;
    let root = std::env::temp_dir();
    let path = root.join(format!("mant-ordinal-structure-{}.1", std::process::id()));
    for (source, expected_kind) in body_and_boundary_inputs()
        .chain(generated_and_control_inputs())
        .map(|(source, _, recovered)| {
            (
                source,
                Some(if recovered { "generic" } else { "definition" }),
            )
        })
        .chain([
            (body_needs_review(), None),
            (bare_zero_advance_with_empty_row(), None),
        ])
        .chain(heads_needing_review().map(|source| (source, None)))
    {
        std::fs::write(&path, &source).unwrap();
        let profile =
            super::profile_request(&json!({"id":"ordinal","path":path,"root":root}).to_string());
        std::fs::remove_file(&path).unwrap();
        let profile = profile.unwrap();
        assert_eq!(profile["violations"], json!([]), "{source}\n{profile}");
        if let Some(kind) = expected_kind {
            assert_eq!(profile["ordinalCasesForReview"], json!([]), "{source}");
            assert_eq!(
                profile["topology"]["expected"]["lists"][0]["kind"], kind,
                "{source}"
            );
        } else {
            assert_eq!(
                profile["ordinalCasesForReview"],
                json!([[8, 2]]),
                "{source}"
            );
            let (expected, topology, mut document) = parsed_structure(&source);
            document.sections[1].blocks.clear();
            let (observed, actual) = super::ir_profile(&document);
            let violations = super::compare_structure(&expected, &observed, &topology, &actual);
            assert!(
                violations
                    .iter()
                    .any(|error| error.contains("list-topology at line 8:2")),
                "{source}\n{violations:?}"
            );
        }
    }
}

#[test]
fn same_origin_ordinal_topology_requires_the_original_occurrence_order() {
    let (expected, topology, document) =
        parsed_structure(super::ordinal_labels::cases::SAME_ORIGIN);
    let compare = |document: &super::Document| {
        let (observed, ir) = super::ir_profile(document);
        super::compare_structure(&expected, &observed, &topology, &ir)
    };
    assert_eq!(topology.lists.len(), 3);
    assert!(
        topology
            .lists
            .iter()
            .all(|list| (list.source_line, list.source_column) == (26, 2))
    );
    assert!(compare(&document).is_empty());

    let mut swapped = document.clone();
    swapped.sections[1].blocks.swap(1, 2);
    assert_eq!(
        compare(&swapped)
            .iter()
            .filter(|error| error.contains("list-topology"))
            .count(),
        2
    );
    for removed in 0..=2 {
        let mut missing = document.clone();
        missing.sections[1].blocks.remove(removed);
        assert!(
            compare(&missing)
                .iter()
                .any(|error| error.contains("list-topology"))
        );
    }
}

#[test]
fn ordinal_recovery_uses_complete_bounded_sequences() {
    // mdoc_validate.c::post_it retains tagged HEAD/BODY owners; the fixed
    // HTML showed one dt/dd pair for each exact source item below. The audit
    // allows a ManT ordered list only for a complete consecutive sequence.
    let header = concat!(
        ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n",
    );
    let assert_topology = |style: &str, body: &str, recovered| {
        let source = format!("{header}.Bl {style}\n{body}.El\n");
        let (expected, topology, document) = parsed_structure(&source);
        let (observed, observed_topology) = super::ir_profile(&document);
        let want = if recovered {
            super::ListTopologyKind::Generic
        } else {
            super::ListTopologyKind::Definition
        };
        assert_eq!(topology.lists[0].kind, want, "{source}");
        let violations =
            super::compare_structure(&expected, &observed, &topology, &observed_topology);
        assert!(violations.is_empty(), "{source}: {violations:?}");
    };
    for (body, recovered) in [
        (".It 4294967295.\nFIRST\n.It 1.\nSECOND\n", false),
        (".It 4294967295.\nFIRST\n.It 4294967296.\nSECOND\n", true),
        (".It 18446744073709551615.\nFIRST\n.It 1.\nSECOND\n", false),
        (
            ".It 18446744073709551614.\nFIRST\n.It 18446744073709551615.\nSECOND\n",
            true,
        ),
        (".It 1.\nFIRST\n", false),
        (".It 1)\nFIRST\n.It 2)\nSECOND\n", true),
        (".It (1)\nFIRST\n.It (2)\nSECOND\n", true),
        (".It 1.\nFIRST\n.It 2)\nSECOND\n", false),
        (".It 1.\n.It 2.\nSECOND\n", false),
        (".It 1.\nFIRST\n.It 2.\n", false),
        (".It 1)\n.Sm off\n.It 2)\nSECOND\n", false),
        (".It 1)\n\\&\n.It 2)\nSECOND\n", false),
        (".It 1)\n.Tg Before\n.It 2)\nSECOND\n", false),
        (".It 1)\n.sp 0\n.It 2)\nSECOND\n", true),
        (".It 1)\n.sp 1\n.It 2)\nSECOND\n", true),
        (".It 1\\fR)\nFIRST\n.It 2\\fR)\nSECOND\n", true),
        (".It Sy 1)\nFIRST\n.It Sy 2)\nSECOND\n", true),
    ] {
        assert_topology("-tag -width Ds", body, recovered);
    }
    for style in ["-hang", "-inset", "-ohang", "-diag"] {
        assert_topology(style, ".It 1)\nFIRST\n.It 2)\nSECOND\n", false);
    }
}

#[test]
fn ip_list_obligations_require_complete_authored_named_bullets() {
    let source = concat!(
        ".TH AUDIT 1\n.SH BODY\n",
        ".IP *\nStar key.\n.IP o\nLetter key.\n.IP +\nPlus key.\n",
        ".IP \\(bu\nBullet.\n.IP \\fB\\[bu]\\fR\nStyled bullet.\n",
        ".IP \\(buSuffix\nNot just a bullet.\n",
        ".IP \\(bu 4\nIndented bullet.\n.TP\n\\(bu\nTagged bullet.\n",
    );
    let parsed = super::Parser::new(super::ParseOptions {
        includes: super::IncludePolicy::Deny,
        compression: super::Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, _) = super::ast_profile(&parsed);
    assert_eq!(expected.generic_list_items, 4);
    let mut violations = Vec::new();
    super::underflow(
        &mut violations,
        "generic-list-items",
        expected.generic_list_items,
        0,
    );
    assert_eq!(violations.len(), 1, "losing named bullets must still fail");
}

#[test]
fn standalone_roff_font_switches_do_not_claim_visible_no_fill_lines() {
    assert!(!is_no_fill_row_text(r"\f[C]"));
    assert!(!is_no_fill_row_text(r"\fR"));
    assert!(!is_no_fill_row_text(r"    \f(CW"));
    assert!(!is_no_fill_row_text(r"\f[B]\f[R]\f[B]"));
    assert!(is_no_fill_row_text(r"\f[C]visible\f[R]"));
    assert!(is_no_fill_row_text("visible"));
}

#[test]
fn bounded_zero_width_guard_runs_claim_one_no_fill_row() {
    assert!(is_zero_width_guard_line(r"\&"));
    assert!(is_zero_width_guard_line(r"\& \&"));
    assert!(!is_zero_width_guard_line(r"\&\c"));
    let mut lines = BTreeMap::new();
    lines.insert(
        10,
        NoFillSourceLine {
            printable: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        11,
        NoFillSourceLine {
            zero_width_blank: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        12,
        NoFillSourceLine {
            zero_width_blank: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        13,
        NoFillSourceLine {
            printable: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        14,
        NoFillSourceLine {
            zero_width_blank: true,
            ..NoFillSourceLine::default()
        },
    );
    assert_eq!(retained_no_fill_rows(&lines), 3);
}

#[test]
fn equation_text_normalizes_bracketed_and_two_character_specials() {
    assert_eq!(
        equation_visible_text(r"1 + \(lf x \(rf \[->] infinity"),
        "1 + ⌊ x ⌋ → infinity"
    );
}

#[test]
fn line_initial_inline_equations_use_executed_origin_and_visible_glyphs() {
    // Exact input ran the five pristine profiles first. roff_eqndelim creates
    // the first EQ inline despite NODE_LINE. roff_escape.c classifies \- as
    // ESCAPE_SPECIAL; html_encode resolves chars.c's '-' entry to ASCII '-'.
    let (expected, native, document) = parsed_structure(
        ".TH AUDIT 1\n.SH BODY\n.EQ\ndelim $$\n.EN\nBefore\n$2\"^\" N\\-1$ after.\n.EQ\nx over y\n.EN\n",
    );
    assert_eq!(expected.inline_equations, 1);
    assert_eq!(expected.display_equations, 1);
    assert_eq!(native.equations[0].context, super::EquationContext::Inline);
    assert_eq!(native.equations[0].value, "2 ^ N -1");
    let (observed, mut lowered) = super::ir_profile(&document);
    assert!(super::compare_structure(&expected, &observed, &native, &lowered).is_empty());
    lowered
        .equations
        .retain(|equation| equation.context != super::EquationContext::Inline);
    assert!(
        super::compare_structure(&expected, &observed, &native, &lowered)
            .iter()
            .any(|violation| violation.starts_with("inline-equation at line 7:"))
    );
}

#[test]
fn equation_observation_consumes_each_escape_once() {
    // Each exact quoted-atom source ran the five pristine profiles first.
    // roff_escape.c consumes escaped backslashes atomically and all three
    // font operand forms. Its one-byte bracket-name guard rejects \[:],
    // while term_word's direct \: is a nonprinting breakpoint.
    for (atom, visible) in [
        (r"a\:b", "ab"),
        (r"a\[:]b", "ab"),
        (r"a\\:b", r"a\:b"),
        (r"a\\[->]b", r"a\[->]b"),
        (r"a\\fBb", r"a\fBb"),
        (r"a\\-b", r"a\-b"),
        (r"a\-b", "a-b"),
        (r"a\[-]b", "a-b"),
        (r"a\0b", "a\u{a0}b"),
        (r"a\eb", r"a\b"),
        (r"a\fBb\fPc", "abc"),
        (r"a\f(BIb\fPc", "abc"),
        (r"a\f[B]b\f[]c", "abc"),
    ] {
        let source = format!(".TH AUDIT 1\n.SH BODY\n.EQ\n\"{atom}\"\n.EN\n");
        let (expected, native, document) = parsed_structure(&source);
        assert_eq!(native.equations[0].value, visible, "{atom}");
        let (observed, lowered) = super::ir_profile(&document);
        assert!(
            super::compare_structure(&expected, &observed, &native, &lowered).is_empty(),
            "{atom}"
        );
    }
}
