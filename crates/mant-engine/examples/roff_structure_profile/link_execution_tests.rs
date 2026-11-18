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

fn mutate_section_links<F>(document: &mut Document, mutation: F)
where
    F: FnMut(usize) -> Option<Option<&'static str>>,
{
    struct Mutation<M> {
        run: M,
        index: usize,
        changed: bool,
    }
    impl<M: FnMut(usize) -> Option<Option<&'static str>>> VisitMut for Mutation<M> {
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
