//! Search-form presentation and the original owners of shared context.
use super::operands::{explain, load};
use mant_ir::{
    Block, DefinitionItem, Document, EntryOwner, ResolvedContent, inline_plain_text, visit,
};
use mant_protocol::{
    ContentSelector, DocumentResponse, EvidenceBasis, EvidenceClass, ExplanationSupport,
    QueryExcerpt, QueryExplanation,
};

const HEADER: &str =
    ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n";
const LIST: &str = ".Bl -tag -width Ds\n";
const COMMANDS: &str = ".It Cm n\n.It Cm N\nGROUP_BODY\n.El\n";

pub(super) fn source(before: &str, section: &str, head: &str) -> String {
    format!("{HEADER}{before}{section}{LIST}{head}{COMMANDS}")
}

pub(super) fn list_groups(document: &Document) -> Vec<Vec<(usize, usize)>> {
    struct Groups(Vec<Vec<(usize, usize)>>);
    impl<'ir> visit::Visit<'ir> for Groups {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::DefinitionList {
                declaration_groups, ..
            } = block
            {
                self.0.push(
                    declaration_groups
                        .iter()
                        .map(|group| (group.start_item, group.end_item))
                        .collect(),
                );
            }
            visit::walk_block(self, block);
        }
    }
    let mut groups = Groups(Vec::new());
    visit::Visit::visit_document(&mut groups, document);
    groups.0
}

fn check_head(item: &DefinitionItem, form: &str, names: &[&str]) {
    let owner = EntryOwner::Definition(item);
    assert_eq!(owner.validated_names().unwrap(), names, "{form}");
    assert!(owner.facts().unwrap().alias_groups.is_empty(), "{form}");
    assert_eq!(
        owner
            .forms()
            .unwrap()
            .iter()
            .map(inline_plain_text)
            .collect::<Vec<_>>(),
        [form]
    );
}

pub(super) fn check_three(
    source: &str,
    form: &str,
    names: &[&str],
    group_start: Option<usize>,
) -> ResolvedContent {
    let query = load(source);
    let document = query.document.as_ref().unwrap();
    let response = DocumentResponse::from(document);
    let decoded: DocumentResponse =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    assert_eq!(Document::from(decoded), *document);
    let items = super::definitions(document);
    let [template, n, upper_n] = items.as_slice() else {
        panic!("{source}");
    };
    check_head(template, form, names);
    for (item, name) in [(n, "n"), (upper_n, "N")] {
        check_head(item, name, &[name]);
    }
    assert!(template.description.is_empty(), "{source}");
    assert!(n.description.is_empty(), "{source}");
    assert!(mant_ir::blocks_have_readable_content(&upper_n.description));
    assert_eq!(
        list_groups(document),
        [group_start
            .into_iter()
            .map(|start| (start, 3))
            .collect::<Vec<_>>()],
        "{source}"
    );

    for (expected, name) in [(n, "n"), (upper_n, "N")] {
        let result = explain(&query, name);
        let result: QueryExplanation =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert_eq!(result.counts.direct_entry.total, 1, "{source}");
        let hit = result
            .evidence
            .iter()
            .find(|row| row.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(hit.source, expected.source);
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        );
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
        );
        let shared = result.supports.iter().find_map(|support| {
            let ExplanationSupport::DeclarationGroup { group, .. } = support else {
                return None;
            };
            Some((group, support.items().unwrap()))
        });
        if let Some(start) = group_start {
            let (range, owners) = shared.unwrap();
            assert_eq!((range.start_item, range.end_item), (start, 3));
            assert_eq!(owners.last().unwrap(), *upper_n);
        } else {
            assert!(shared.is_none(), "{source}");
        }
        let excerpt =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        let excerpt: QueryExcerpt =
            serde_json::from_str(&serde_json::to_string(&excerpt).unwrap()).unwrap();
        assert_eq!(
            mant_render::render_excerpt_text(&excerpt)
                .matches("GROUP_BODY")
                .count(),
            usize::from(name == "N"),
            "{source}"
        );
        assert_eq!(
            mant_render::render_explanation_text(&result)
                .matches("GROUP_BODY")
                .count(),
            usize::from(name == "N" || group_start.is_some()),
            "{source}"
        );
    }
    if names.is_empty() && !form.chars().any(char::is_control) {
        let template_only = explain(&query, form);
        assert!(
            template_only.evidence.iter().all(|hit| {
                hit.bases
                    .iter()
                    .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
            }),
            "{source}"
        );
        assert!(template_only.supports.is_empty(), "{source}");
    }
    query
}

#[test]
fn declared_search_parameters_follow_the_executed_word_boundary() {
    // Pinned mdoc_term.c::termp_sm_pre(), termp_ns_pre() and termp_pf_post()
    // alter term.c::term_word's inter-word blank. Filled Xo treats a new
    // source row as an input event, not a required separator.
    for sign in ['/', '?'] {
        let simple = format!("{sign}RE");
        for (before, section, head, form) in [
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Pf {sign} Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It No {sign} Ns Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It {sign} Ns Ar RE\n"),
                simple.clone(),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(".It No {sign} Ar RE\n"),
                simple.clone(),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n.Ss Search\n",
                format!(".It No {sign} Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".Sm\n.It No {sign} Ar RE\n.Sm\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.Sm off\n.No {sign}\n.Ar RE\n.Sm on\n.Xc\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.No {sign}\\c\n.Ar RE\n.Xc\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.No {sign} Ns\n.Ar RE\n.Xc\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Pf \\fB\\^{sign}\\fP Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It No \\z{sign} Ar RE\n"),
                simple,
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(".It No {sign} Ar RE {sign} Ar OFFSET\n"),
                format!("{sign}RE{sign}OFFSET"),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(r#".It No {sign} Ar "\fB語, (n) \(dq--fake\(dq\fP""#) + "\n",
                format!("{sign}語, (n) \"--fake\""),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.No {sign}\n.Ar \"R\\p \\p DROPPED\"\n.Xc\n"),
                format!("{sign}R"),
            ),
        ] {
            check_three(&source(before, section, &head), &form, &[], Some(1));
        }
    }
}

#[test]
fn a_search_form_requires_surviving_source_owned_signs_and_argument() {
    // Pinned term.c::encode1() lets later glyphs overstrike a pending \z
    // cell, and term_fill() consumes \p at the retained word boundary.
    for sign in ['/', '?'] {
        let spaced = format!("{sign} RE");
        for (before, head, form) in [
            ("", format!(".It No {sign} Ar RE\n"), spaced.clone()),
            ("", format!(".It Pf {sign} Ar \" RE\"\n"), spaced),
            (
                "",
                format!(".It Xo\n.No {sign}\n.Sm off\n.Ar RE\n.Xc\n.Sm on\n"),
                format!("{sign}{}RE", if sign == '?' { "  " } else { " " }),
            ),
            (
                ".Sm off\n",
                format!(".It No \\z{sign} Ar RE\n"),
                "RE".into(),
            ),
            (
                ".Sm off\n",
                format!(".It No {sign} Ar \\zR Ar E\n"),
                format!("{sign}E"),
            ),
            (
                ".Sm off\n",
                format!(".It No {sign} Ar RE Ns \\z{sign} Ar OFFSET\n"),
                format!("{sign}REOFFSET"),
            ),
            (
                ".Sm off\n",
                format!(".It No {sign} Ar \"A\\p B\"\n"),
                format!("{sign}A\nB"),
            ),
            (
                ".Sm off\n",
                format!(".It Bq No {sign} Ar RE\n"),
                format!("[{sign}RE]"),
            ),
            (
                ".Sm off\n",
                format!(".It Pq No {sign} Ar RE\n"),
                format!("({sign}RE)"),
            ),
        ] {
            check_three(&source(before, ".Sh COMMANDS\n", &head), &form, &[], None);
        }
    }
    for (head, form) in [
        (".It Bq No \\z? Ar RE\n", "[RE]"),
        (".It Pq No \\z? Ar RE\n", "(RE)"),
        (".It Bo No \\z? Ar RE Bc\n", "[RE]"),
    ] {
        // The enclosure's own words remain, but the overwritten operator
        // cannot turn the Ar slot into a standalone operand definition.
        check_three(
            &source(".Sm off\n", ".Sh COMMANDS\n", head),
            form,
            &[],
            None,
        );
    }
    check_three(
        &source(".Sm off\n", ".Sh COMMANDS\n", ".It Bq Ar file\n"),
        "[file]",
        &["file"],
        Some(0),
    );
    for (head, form, group) in [
        (r".It No ? Ar \& Ar RE", "?RE", None),
        (r".It No ? Ar x?y Ns \z? Ar OFFSET", "?x?yOFFSET", None),
        (
            ".It Xo\n.No /\n.Ar R/\n/\n.Ar offset\n.Xc",
            "/R//offset",
            Some(1),
        ),
        (
            ".It Xo\n.No /\n.Ar R/\n\\z/\n.Ar offset\n.Xc",
            "/R/offset",
            None,
        ),
    ] {
        check_three(
            &source(".Sm off\n", ".Sh COMMANDS\n", &format!("{head}\n")),
            form,
            &[],
            group,
        );
    }
}

#[test]
fn identical_visible_search_spellings_can_be_real_names() {
    // A styled, separated '?' is an independent command. No/Sy argument
    // words can also spell a literal command without an Ar parameter.
    for (head, mode, form, name) in [
        (".It No \\fB?\\fP Ar RE\n", ".Sm on\n", "? RE", "?"),
        (".It No ? No RE\n", ".Sm off\n", "?RE", "?RE"),
        (".It No ? Sy RE\n", ".Sm off\n", "?RE", "?RE"),
        (".It No ? Ns Ar RE , Cm later\n", "", "?RE, later", "later"),
        (
            ".It No \\z? Ar RE , Cm later\n",
            ".Sm off\n",
            "RE,later",
            "later",
        ),
    ] {
        check_three(
            &source(mode, ".Sh COMMANDS\n", head),
            form,
            &[name],
            Some(0),
        );
    }
}

#[test]
fn only_a_proved_template_or_own_body_releases_run_boundaries() {
    let before = ".Sm off\n";
    let section = ".Sh COMMANDS\n";
    for (head, expected) in [
        (".It No ? Ar RE\nTEMPLATE_BODY\n", vec![(1, 3)]),
        (".It No UNKNOWN Ns Ar PARAM\n.It No ? Ar RE\n", vec![]),
        (".It No ? Ar RE\n.It No UNKNOWN Ns Ar PARAM\n", vec![]),
        (
            ".It Cm a\n.It Cm A\nFIRST_BODY\n.It No ? Ar RE\n",
            vec![(0, 2), (3, 5)],
        ),
        (".It No ? Ar RE\n.It No / Ar RE\n", vec![(2, 4)]),
    ] {
        let query = load(&source(before, section, head));
        assert_eq!(
            list_groups(query.document.as_ref().unwrap()),
            [expected],
            "{head}"
        );
        let shared_text = mant_render::render_explanation_text(&explain(&query, "n"));
        assert!(!shared_text.contains("FIRST_BODY") && !shared_text.contains("TEMPLATE_BODY"));
    }
    let nested = format!(
        "{HEADER}{before}{section}{LIST}.It Cm outer\nOUTER_BODY\n{LIST}.It No ? Ar RE\n{COMMANDS}.El\n"
    );
    let query = load(&nested);
    assert_eq!(
        list_groups(query.document.as_ref().unwrap()),
        [vec![], vec![(1, 3)]]
    );
    assert!(mant_render::render_explanation_text(&explain(&query, "n")).contains("GROUP_BODY"));
    let broken = format!(
        "{HEADER}{before}{section}{LIST}.It No ? Ar RE\n.It Cm n\n.Pp\n.It Cm N\nGROUP_BODY\n.El\n"
    );
    let query = load(&broken);
    assert_eq!(list_groups(query.document.as_ref().unwrap()), [vec![]]);
    assert!(explain(&query, "n").supports.is_empty(), "{broken}");
}

#[test]
fn expanded_heads_with_identical_coordinates_keep_distinct_owners() {
    // Each PAIR's three It BLOCKs share their invocation coordinate in
    // pinned -Ttree. The second expansion is a fresh sequence of owners.
    let source = format!(
        "{HEADER}.Sm off\n.Sh COMMANDS\n{LIST}.de PAIR\n.It No ? Ar RE\n.It Cm n\n.It Cm N\n\\\\$1\n..\n.PAIR FIRST_BODY\n.PAIR SECOND_BODY\n.El\n"
    );
    let query = load(&source);
    let document = query.document.as_ref().unwrap();
    let items = super::definitions(document);
    assert_eq!(items.len(), 6);
    for triple in items.as_chunks::<3>().0 {
        assert_eq!(triple[0].source, triple[1].source);
        assert_eq!(triple[1].source, triple[2].source);
        for (item, spelling, names) in [
            (triple[0], "?RE", &[][..]),
            (triple[1], "n", &["n"][..]),
            (triple[2], "N", &["N"][..]),
        ] {
            check_head(item, spelling, names);
        }
    }
    assert_eq!(list_groups(document), [vec![(1, 3), (4, 6)]]);
    let result = explain(&query, "n");
    result.validate_references().unwrap();
    assert_eq!(result.counts.direct_entry.total, 2);
    let supports = result
        .supports
        .iter()
        .filter_map(ExplanationSupport::items)
        .collect::<Vec<_>>();
    assert_eq!(supports.len(), 2);
    for (members, provider) in supports.into_iter().zip([items[2], items[5]]) {
        assert_eq!(members.last().unwrap(), provider);
    }
    let text = mant_render::render_explanation_text(&result);
    for token in ["FIRST_BODY", "SECOND_BODY"] {
        assert_eq!(text.matches(token).count(), 1);
    }
}
