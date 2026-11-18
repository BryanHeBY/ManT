//! Audit both native link execution and deliberately corrupted observations.

use mant_ir::{
    Block, Document, Inline, LinkTarget, ResolvedContent, inline_plain_text, visit::VisitMut,
};

use super::parsed_structure;
use crate::{compare_structure, ir_profile};

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
const SECTIONS: &str = ".Sh ONE\nBODY-A.\n.Sh TWO\nBODY-B.\n";

#[test]
fn a_recovered_only_section_label_with_an_error_does_not_hide_valid_links() {
    // term.c::term_word() emits no glyph for the unknown special character.
    // The pinned Html still emits its empty href label. A product recovery
    // spelling may vanish under the existing zero-advance operation.
    for (commands, unverified, proven) in [
        (".No \\z\n.Sx \\[foo]\n.No Y\n.Sx ONE\n", 1, 1),
        (".Sx \\[foo]\n.No Y\n.Sx ONE\n", 1, 1),
        (".No \\z\n.Sx \\[foo]X\n.No Y\n.Sx ONE\n", 0, 2),
        (
            ".de QQ\n.No \\z\n.Sx \\[foo]\n.No Y\n.Sx ONE\n..\n.QQ\n",
            1,
            1,
        ),
    ] {
        let source = format!("{HEADER}{commands}.Sh ONE\nBODY-A.\n");
        let (expected, native, document) = parsed_structure(&source);
        assert_eq!(
            expected.section_outcomes.uncertain_labels().len(),
            unverified,
            "{source}"
        );
        assert_eq!(expected.section_links, proven, "{source}");
        assert!(
            document
                .diagnostics
                .iter()
                .any(|d| d.level == mant_ir::DiagnosticLevel::Error)
        );
        let compare = |document: &Document| {
            let (observed, topology) = ir_profile(document);
            compare_structure(&expected, &observed, &native, &topology)
        };
        assert!(
            compare(&document).is_empty(),
            "{source}: {:?}",
            compare(&document)
        );
        let mut missing = document;
        mutate_section_links(&mut missing, |_| Some(None));
        assert_finding(&compare(&missing), "expected \"ONE\"", "source order");
    }
}

#[test]
fn lossy_fragment_collisions_and_address_errors_are_observed_per_occurrence() {
    // The exact fixed HTML has both spellings mapped to W_X, heading IDs
    // W_X/W_X~2, and an entity-escaped angle-bracket external destination.
    let source = format!(
        "{HEADER}.Sx W^X\n.No \\z\n.Sx \\[foo]\n.No Y\n.Sx W>X\n.Lk https://e/\\%<>Y LABEL\n.Sh W^X\nBODY-A.\n.Sh W>X\nBODY-B.\n"
    );
    let (expected, native, mut document) = parsed_structure(&source);
    assert_eq!(expected.section_outcomes.uncertain_labels().len(), 1);
    assert_eq!(expected.section_links, 2);
    assert!(
        document
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.invalid-external-uri"))
    );
    let compare = |document: &Document| {
        let (observed, topology) = ir_profile(document);
        compare_structure(&expected, &observed, &native, &topology)
    };
    assert!(compare(&document).is_empty(), "{:?}", compare(&document));
    let targets = [
        document.sections[3].id.to_string(),
        document.sections[2].id.to_string(),
    ];
    mutate_section_links(&mut document, |index| {
        targets.get(index).map(|id| Some(id.as_str()))
    });
    assert_finding(&compare(&document), "expected \"W>X\"", "source order");
}

#[test]
fn section_references_need_their_own_targets_in_execution_order() {
    // CVS mdoc_sx_pre emits one anchor per Sx invocation. The -Ttree
    // coordinates can be equal after QQ expands the two distinct macros.
    for (commands, headings) in [
        (".de QQ\n.Sx ONE\n.Sx TWO\n..\n.QQ\n", SECTIONS),
        (".Sx ONE\n.Sx TWO\n", SECTIONS),
        (".de QQ\n.Sx ONE\n.Sx ONE\n..\n.QQ\n", SECTIONS),
        (
            ".de QQ\n.Sx ONE\n.Sx TWO\n.Sh ONE\nBODY-A.\n.Sh TWO\nBODY-B.\n..\n.QQ\n",
            "",
        ),
    ] {
        let source = [HEADER, commands, headings].concat();
        let (expected, native, document) = parsed_structure(&source);
        let compare = |document: &Document| {
            let (observed, topology) = ir_profile(document);
            compare_structure(&expected, &observed, &native, &topology)
        };
        assert_eq!(expected.section_links, 2);
        assert!(
            compare(&document).is_empty(),
            "{commands}: {:?}",
            compare(&document)
        );
        for occurrence in 0..2 {
            let mut removed = document.clone();
            mutate_section_links(&mut removed, |index| (occurrence == index).then_some(None));
            assert_finding(&compare(&removed), "expected", "in source order");
        }
        // Both mutations keep the link count. A same-kind surplus cannot
        // satisfy a different target, and a swap preserves the multiset.
        let mut replaced = document.clone();
        mutate_section_links(&mut replaced, |index| {
            (index == 0).then_some(Some("description"))
        });
        assert_finding(&compare(&replaced), "expected \"ONE\"", "in source order");
        if commands.contains("Sx TWO") {
            let mut swapped = document.clone();
            mutate_section_links(&mut swapped, |index| {
                ["two", "one"].get(index).map(|target| Some(*target))
            });
            assert_finding(&compare(&swapped), "expected \"TWO\"", "in source order");
        }
    }
}

#[test]
fn unresolved_section_references_need_their_own_diagnostics() {
    // CVS emits the authored fragments even when no heading has that title.
    // The reading contract retains the label and records its resolution failure.
    for (commands, resolved, unresolved) in [
        (
            ".de QQ\n.Sx ONE\n.Sx MISSING\n..\n.QQ\n",
            1,
            &["MISSING"][..],
        ),
        (".Sx ONE\n.Sx MISSING\n", 1, &["MISSING"][..]),
        (
            ".de QQ\n.Sx MISSING\n.Sx OTHER\n..\n.QQ\n",
            0,
            &["MISSING", "OTHER"][..],
        ),
        (
            ".de QQ\n.Sx MISSING\n.Sx MISSING\n..\n.QQ\n",
            0,
            &["MISSING", "MISSING"][..],
        ),
    ] {
        let (expected, native, document) = parsed_structure(&[HEADER, commands, SECTIONS].concat());
        let compare = |document: &Document| {
            let (observed, topology) = ir_profile(document);
            compare_structure(&expected, &observed, &native, &topology)
        };
        let (observed, _) = ir_profile(&document);
        assert_eq!(expected.section_links, 2);
        assert_eq!(observed.section_links, resolved);
        assert_eq!(observed.unresolved_section_references, unresolved.len());
        assert!(
            compare(&document).is_empty(),
            "{commands}: {:?}",
            compare(&document)
        );
        for (offset, title) in unresolved.iter().enumerate() {
            let position = document
                .diagnostics
                .iter()
                .enumerate()
                .filter(|(_, diagnostic)| {
                    diagnostic.code.as_deref() == Some("unresolved-section-reference")
                })
                .nth(offset)
                .unwrap()
                .0;
            let mut missing = document.clone();
            missing.diagnostics.remove(position);
            assert_finding(&compare(&missing), "expected unresolved diagnostic", title);
            let mut wrong = document.clone();
            wrong.diagnostics[position].message =
                "cannot resolve section reference: UNRELATED".to_owned();
            assert_finding(&compare(&wrong), "expected unresolved diagnostic", title);
        }
        if resolved > 0 {
            let mut missing = document.clone();
            mutate_section_links(&mut missing, |index| (index == 0).then_some(None));
            missing.diagnostics.push(
                missing
                    .diagnostics
                    .iter()
                    .find(|diagnostic| {
                        diagnostic.code.as_deref() == Some("unresolved-section-reference")
                    })
                    .unwrap()
                    .clone(),
            );
            assert_finding(&compare(&missing), "expected \"ONE\"", "in source order");
        }
    }
}

#[test]
fn a_section_reference_with_a_competing_paragraph_target_requires_a_diagnostic() {
    // CVS post_tg() puts the first target on Pp and the second on Sh HEAD.
    // Choosing the heading silently loses the conflict with the paragraph.
    let source = [
        HEADER,
        ".Sx W^X Test\n.Tg W^X_Test\n.Pp\nBODY-A.\n.Tg W^X_Test\n.Sh OTHER\nBODY-B.\n",
    ]
    .concat();
    let (expected, native, mut document) = parsed_structure(&source);
    let compare = |document: &Document| {
        let (observed, topology) = ir_profile(document);
        compare_structure(&expected, &observed, &native, &topology)
    };
    assert!(compare(&document).is_empty(), "{:?}", compare(&document));

    let id = document.sections[2].id.clone();
    let mant_ir::Block::Paragraph { children, .. } = &mut document.sections[1].blocks[0] else {
        panic!("expected the Sx label in an ordinary paragraph");
    };
    let label = std::mem::take(children);
    assert_eq!(inline_plain_text(&label), "W^X Test");
    children.push(Inline::Link {
        target: LinkTarget::Section { id },
        title: None,
        children: label,
    });
    document
        .diagnostics
        .retain(|diagnostic| diagnostic.code.as_deref() != Some("unresolved-section-reference"));
    assert_finding(
        &compare(&document),
        "expected unresolved diagnostic",
        "W^X Test",
    );
}

#[test]
fn section_link_obligations_distinguish_source_control_words_from_labels() {
    // Fixed CVS mdoc_sx_pre emits an empty HTML link for \fP and \s0. Tree
    // and term_word distinguish \& from the fixed blank \~ and its glyph.
    // The pristine HTML renderer has no usable href for the empty ID forms.
    for (label, visible, unresolved) in [
        (r"\fP", false, false),
        (r"\s0", false, false),
        (r"\&", false, false),
        (r"\p", false, false),
        ("\"\"", false, false),
        (r"\~", true, true),
        (r"\0", true, true),
        ("TOPIC", true, false),
    ] {
        let source = format!("{HEADER}.Sx {label}\n.br\nNEXT-A.\n.Sh TOPIC\nBODY-A.\n");
        let (expected, syntax, document) = parsed_structure(&source);
        let (observed, topology) = ir_profile(&document);
        assert_eq!(
            expected.section_outcomes.source_reference_counts(),
            (1, usize::from(!visible)),
            "{source}"
        );
        assert_eq!(expected.section_links, usize::from(visible), "{source}");
        assert_eq!(
            observed.section_links,
            usize::from(visible && !unresolved),
            "{source}"
        );
        assert_eq!(
            observed.unresolved_section_references,
            usize::from(unresolved),
            "{source}"
        );
        assert!(render_text(document.clone()).contains("NEXT-A."));
        let reading = reading(document.clone());
        let markdown = mant_codec::encode::render_addressable_markdown(&reading);
        if matches!(label, r"\~" | r"\0") {
            assert!(render_text(document.clone()).contains('\u{a0}'), "{source}");
        }
        assert_eq!(
            markdown.text().contains("](#"),
            visible && !unresolved,
            "{source}"
        );
        let findings = compare_structure(&expected, &observed, &syntax, &topology);
        assert!(findings.is_empty(), "{source}: {findings:?}");
    }
}

#[test]
fn mixed_section_link_obligations_reject_misrouted_links_and_diagnostics() {
    // QQ expands both visible labels and the font-only operand at 13:2.
    // Fixed CVS HTML targets TOPIC's Fl heading, emits an empty anchor for
    // the font change, and has no destination for MISSING.
    let source = [
        HEADER,
        ".de QQ\n.Sx TOPIC\n.Sx \\\\fP\n.Sx MISSING\n..\n.QQ\n.Sh Fl TOPIC\nBODY-A.\n",
    ]
    .concat();
    let (expected, native, mut document) = parsed_structure(&source);
    let compare = |document: &Document| {
        let (observed, topology) = ir_profile(document);
        compare_structure(&expected, &observed, &native, &topology)
    };
    assert_eq!(expected.section_outcomes.source_reference_counts(), (3, 1));
    assert_eq!(expected.section_links, 2);
    assert!(compare(&document).is_empty(), "{:?}", compare(&document));
    let mut wrong_owner = document.clone();
    mutate_section_links(&mut wrong_owner, |index| {
        (index == 0).then_some(Some("description"))
    });
    assert_finding(
        &compare(&wrong_owner),
        "expected \"TOPIC\"",
        "in source order",
    );
    let diagnostic = document
        .diagnostics
        .iter_mut()
        .find(|diagnostic| diagnostic.code.as_deref() == Some("unresolved-section-reference"))
        .unwrap();
    diagnostic.message = "cannot resolve section reference: \\fP".to_owned();
    assert_finding(
        &compare(&document),
        "expected unresolved diagnostic",
        "MISSING",
    );
}

#[test]
fn list_item_links_require_an_executed_native_part() {
    // CVS mdoc_it_pre skips the HEAD of bullet/dash/hyphen/item/enum lists.
    // Lint keeps the skipped-arguments error; the BODY and definition terms
    // still execute. The -column row puts the address in a cell BODY.
    for (options, skips_head) in [
        ("bullet", true),
        ("dash", true),
        ("hyphen", true),
        ("item", true),
        ("enum", true),
        ("tag -width Ds", false),
        ("hang -width Ds", false),
        ("column Ds", false),
    ] {
        for in_head in [false, true] {
            let item = if in_head {
                ".It Lk https://example.test LABEL\n".to_owned()
            } else {
                let tag = if skips_head { "" } else { " TAG" };
                format!(".It{tag}\n.Lk https://example.test LABEL\n")
            };
            let source = format!("{HEADER}.Bl -{options}\n{item}BODY-A.\n.El\n");
            let (expected, native, document) = parsed_structure(&source);
            let (observed, topology) = ir_profile(&document);
            let ignored = skips_head && in_head;
            assert_eq!(expected.external_links, usize::from(!ignored), "{source}");
            assert_eq!(observed.external_links, usize::from(!ignored), "{source}");
            let visible = render_text(document.clone());
            assert!(visible.contains("BODY-A."));
            assert_eq!(visible.contains("LABEL"), !ignored);
            if ignored {
                assert!(
                    document
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.message.contains("skipping all arguments"))
                );
            }
            let violations = compare_structure(&expected, &observed, &native, &topology);
            assert!(violations.is_empty(), "{source}: {violations:?}");
            if !ignored {
                let mut missing = document;
                super::link_tests::change_address(&mut missing, 0, None);
                let (observed, topology) = ir_profile(&missing);
                assert_finding(
                    &compare_structure(&expected, &observed, &native, &topology),
                    "external-links:",
                    "observed 0",
                );
            }
        }
    }
}

#[test]
fn manual_link_obligations_distinguish_glyphless_and_invalid_names() {
    // CVS mdoc_xr_pre/man_MR_pre emit parentheses for an explicit section.
    // Unusable empty names have a structured IR diagnostic despite the
    // visible (3). Explicit empty and roff zero-width names agree.
    for (macro_name, header, raw_name, section, usable, invalid, label) in [
        ("Xr", HEADER, "", "", 0, 0, "VISIBLE."),
        ("Xr", HEADER, " \"\"", "", 0, 0, "VISIBLE."),
        ("Xr", HEADER, " \\&", "", 0, 0, "VISIBLE."),
        ("Xr", HEADER, " \\0", "", 0, 0, "VISIBLE."),
        ("Xr", HEADER, " \\~", "", 0, 0, "VISIBLE."),
        ("Xr", HEADER, " \"\"", " 3", 0, 1, "(3)"),
        ("Xr", HEADER, " \\&", " 3", 0, 1, "(3)"),
        ("Xr", HEADER, " ls", "", 1, 0, "ls"),
        ("Xr", HEADER, " ls", " 3", 1, 0, "ls(3)"),
        (
            "MR",
            ".TH PROBE 1 \"November 18, 2026\"\n.SH DESCRIPTION\n",
            " \"\"",
            " 3",
            0,
            1,
            "(3)",
        ),
        (
            "MR",
            ".TH PROBE 1 \"November 18, 2026\"\n.SH DESCRIPTION\n",
            " \\&",
            " 3",
            0,
            1,
            "(3)",
        ),
    ] {
        let source = format!("{header}.{macro_name}{raw_name}{section}\nVISIBLE.\n");
        let (expected, native, document) = parsed_structure(&source);
        let (observed, topology) = ir_profile(&document);
        assert_eq!(expected.manual_links, usable, "{source}");
        assert_eq!(observed.manual_links, usable, "{source}");
        assert_eq!(expected.invalid_manual_links.len(), invalid, "{source}");
        assert_eq!(observed.invalid_manual_links.len(), invalid, "{source}");
        assert_eq!(observed.invalid_manual_diagnostics, invalid, "{source}");
        assert!(render_text(document.clone()).contains(label), "{source}");
        if matches!(raw_name.trim(), r"\0" | r"\~") {
            let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
                panic!("expected the native spacing followed by its paragraph: {source}");
            };
            assert!(
                inline_plain_text(children).starts_with('\u{a0}'),
                "{source}"
            );
        }
        assert!(
            compare_structure(&expected, &observed, &native, &topology).is_empty(),
            "{source}"
        );
        if invalid != 0 {
            let markdown = mant_codec::encode::render_markdown(&reading(document.clone()));
            assert!(markdown.contains("(3)"));
            assert!(!markdown.contains("man:"));
            let mut substituted = document.clone();
            super::link_tests::change_address(
                &mut substituted,
                0,
                Some(LinkTarget::Manual {
                    name: "ls".to_owned(),
                    manual_section: Some("3".to_owned()),
                }),
            );
            let (observed, topology) = ir_profile(&substituted);
            assert_finding(
                &compare_structure(&expected, &observed, &native, &topology),
                "invalid-manual-targets:",
                "observed 0",
            );
            let mut missing = document;
            missing.diagnostics.retain(|diagnostic| {
                diagnostic.code.as_deref() != Some("ir.invalid-manual-reference")
            });
            let (observed, topology) = ir_profile(&missing);
            assert_finding(
                &compare_structure(&expected, &observed, &native, &topology),
                "invalid-manual-reference-diagnostics:",
                "observed 0",
            );
        }
    }
}

#[test]
fn invalid_manual_labels_in_headings_survive_markdown_without_a_target() {
    // CVS HTML includes the empty-name anchor in both invalid examples.
    // The heading exporter permits usable manual destinations, as ls(3)
    // shows, while the invalid addresses remain ordinary visible labels.
    for (operand, valid) in [("\"\"", false), (r"\&", false), ("ls", true)] {
        let source = format!("{HEADER}.Ss Xr {operand} 3\nVISIBLE.\n");
        let (expected, native, document) = parsed_structure(&source);
        let (observed, topology) = ir_profile(&document);
        assert!(
            compare_structure(&expected, &observed, &native, &topology).is_empty(),
            "{source}"
        );
        let markdown = mant_codec::encode::render_markdown(&reading(document));
        assert_eq!(markdown.contains("man:"), valid, "{markdown}");
        assert!(markdown.contains("(3)"), "{markdown}");
        let imported = mant_loader::load_markdown_text(&markdown, None)
            .unwrap()
            .document
            .unwrap();
        let (observed, _) = ir_profile(&imported);
        assert_eq!(observed.manual_links, usize::from(valid), "{markdown}");
    }
}

fn reading(document: Document) -> ResolvedContent {
    ResolvedContent {
        label: "audit".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    }
}

fn render_text(document: Document) -> String {
    mant_render::render_query_text(&reading(document))
}

fn assert_finding(violations: &[String], property: &str, reason: &str) {
    assert!(
        violations
            .iter()
            .any(|item| item.contains(property) && item.contains(reason)),
        "wanted {property:?} and {reason:?}, got {violations:?}"
    );
}

fn mutate_section_links<'target, F>(document: &mut Document, mutation: F)
where
    F: FnMut(usize) -> Option<Option<&'target str>>,
{
    struct Mutation<M> {
        run: M,
        index: usize,
        changed: bool,
    }
    impl<'target, M: FnMut(usize) -> Option<Option<&'target str>>> VisitMut for Mutation<M> {
        fn visit_inline_mut(&mut self, inline: &mut Inline) {
            if let Inline::Link {
                target: LinkTarget::Section { id },
                children,
                ..
            } = inline
            {
                if let Some(edit) = (self.run)(self.index) {
                    if let Some(target) = edit {
                        *id = target.into();
                    } else {
                        *inline = Inline::Text {
                            value: inline_plain_text(children),
                        };
                    }
                    self.changed = true;
                }
                self.index += 1;
            }
            mant_ir::visit::walk_inline_mut(self, inline);
        }
    }
    let mut visit = Mutation {
        run: mutation,
        index: 0,
        changed: false,
    };
    visit.visit_document_mut(document);
    assert!(visit.changed);
}
