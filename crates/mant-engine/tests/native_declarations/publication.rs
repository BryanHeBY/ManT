use super::{
    definitions,
    operands::{explain, load},
};
use mant_ir::{
    Block, Document, EntryInlineRoot, EntryKind, EntryOwner, Inline, LinkTarget, ParameterKind,
};
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

const MAN: &str = ".TH TEST 1 \"2026-11-17\"\n.SH DESCRIPTION\n";
const MDOC: &str = ".Dd November 17, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd role probe\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n";
const OP: &str = r#".OP \-\-color "printf(1)$MARK""#;
const MARKER: &str = r" \%<>";
const OPTION: EntryKind = EntryKind::Parameter {
    parameter_kind: ParameterKind::Option,
};

fn man_head(head: &str) -> String {
    format!("{MAN}.TP\n{head}\nUNIQUE_BODY\n")
}

fn mdoc_head(head: &str) -> String {
    format!("{MDOC}{head}\nUNIQUE_BODY\n.El\n")
}

fn forms(owner: EntryOwner<'_>) -> Vec<String> {
    owner
        .forms()
        .unwrap()
        .iter()
        .map(mant_ir::inline_plain_text)
        .collect()
}

fn manual_links(nodes: &[Inline]) -> usize {
    use mant_ir::visit::{self, Visit};
    #[derive(Default)]
    struct Links(usize);
    impl<'ir> Visit<'ir> for Links {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target:
                    LinkTarget::Manual {
                        name,
                        manual_section,
                    },
                ..
            } = inline
                && name == "printf"
                && manual_section.as_deref() == Some("1")
            {
                self.0 += 1;
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut links = Links::default();
    for node in nodes {
        links.visit_inline(node);
    }
    links.0
}

fn assert_names(query: &mant_ir::ResolvedContent, names: &[&str], source: &str) {
    for name in names {
        let explanation = explain(query, name);
        let direct: Vec<_> = explanation
            .evidence
            .iter()
            .filter(|hit| hit.class == EvidenceClass::DirectEntry)
            .collect();
        let [hit] = direct.as_slice() else {
            panic!("expected one direct definition of {name}: {source}");
        };
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. })),
            "{name}: {source}"
        );
        let excerpt =
            mant_query::select_excerpt(query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        assert!(mant_render::render_excerpt_text(&excerpt).contains("UNIQUE_BODY"));
    }
    assert_eq!(
        explain(query, "--fake").counts.direct_entry.total,
        0,
        "{source}"
    );
}

fn assert_final_fl_binding(owner: EntryOwner<'_>, source: &str) {
    let facts = owner.facts().unwrap();
    let binding = facts
        .name_bindings
        .iter()
        .find(|binding| facts.names[binding.name] == "-all")
        .unwrap();
    let [name] = binding.occurrences.as_slice() else {
        panic!("{source}")
    };
    let nodes = owner
        .inline_root(&EntryInlineRoot::Term { index: 0 })
        .unwrap();
    // Both occurrences spell -all. Its binding must resolve to the final
    // Fl, after the Ar's same-spelled text.
    let last = nodes
        .iter()
        .rposition(|node| mant_ir::inline_plain_text(std::slice::from_ref(node)).contains("-all"))
        .unwrap();
    assert_eq!(name.parts[0].path[0], last, "{source}");
}

#[test]
fn source_link_publication_preserves_names_and_parameter_exclusions() {
    // CVS man_term.c::pre_OP/pre_alternate and mdoc_term.c::termp_fl_pre/
    // termp_fn_pre execute the same operands across these forms. The source
    // escape is a ManT navigation convention, independently of CVS tags.
    let cases = [
        (man_head(OP), OPTION, &["--color"][..]),
        (
            man_head(r#".OP \-\-color "printf(1)$MARK \fB--fake""#),
            OPTION,
            &["--color"],
        ),
        (
            man_head(r#".OP \-\-color "é printf(1)$MARK \fB--fake printf(1)$MARK""#),
            OPTION,
            &["--color"],
        ),
        (
            man_head(r#".BR \-\-color " printf(1)$MARK""#),
            OPTION,
            &["--color"],
        ),
        (
            man_head(r#".BI \-\-color " printf(1)$MARK""#),
            OPTION,
            &["--color"],
        ),
        (
            man_head(r#".BR \-\-color " " printf "(1)$MARK""#),
            OPTION,
            &["--color"],
        ),
        (
            mdoc_head(r#".It Fl color Ar "printf(1)$MARK \fB--fake" , Fl all"#),
            OPTION,
            &["-color", "-all"],
        ),
        (
            mdoc_head(r#".It Fl color Ar "é printf(1)$MARK \fB-all printf(1)$MARK" , Fl all"#),
            OPTION,
            &["-color", "-all"],
        ),
        (
            mdoc_head(r#".It Dv NAME Fa "printf(1)$MARK""#),
            EntryKind::Term,
            &["NAME"],
        ),
        (
            mdoc_head(r#".It Fn checked "printf(1)$MARK""#),
            EntryKind::Term,
            &["checked"],
        ),
        (
            mdoc_head(r#".It Dv FIRST Fa "printf(1)$MARK" , Dv SECOND"#),
            EntryKind::Term,
            &["FIRST", "SECOND"],
        ),
    ];
    for (template, kind, names) in cases {
        let mut before_publication = None;
        for marker in ["", " <>", MARKER] {
            let source = template.replace("$MARK", marker);
            let query = load(&source);
            let document = query.document.as_ref().unwrap();
            let items = definitions(document);
            let [item] = items.as_slice() else {
                panic!("{source}")
            };
            let owner = EntryOwner::Definition(item);
            assert_eq!(owner.facts().unwrap().kind, kind, "{source}");
            assert_eq!(owner.validated_names().unwrap(), names, "{source}");
            assert_names(&query, names, &source);
            let body = &item.description;
            let current = forms(owner);
            if marker.is_empty() {
                before_publication = Some((current, body.clone()));
            } else if marker == MARKER {
                assert_eq!(
                    before_publication.as_ref(),
                    Some(&(current, body.clone())),
                    "{source}"
                );
                assert_eq!(
                    manual_links(&item.terms[0]),
                    template.matches("$MARK").count(),
                    "{source}"
                );
                assert!(
                    mant_ir::semantics_complete(&document.diagnostics),
                    "{source}: {:?}",
                    document.diagnostics
                );
            } else {
                assert!(current[0].contains("<>"), "{source}");
            }
            if source.contains(r"\fB-all") {
                assert_final_fl_binding(owner, &source);
            }
        }
    }
}

#[test]
fn reference_like_text_cannot_promote_an_unqualified_key() {
    for (head, names) in [
        (OP.replace("printf(1)", "ordinary"), &["--color"][..]),
        (r#".OP "\-\-fake printf(1)$MARK" WHEN"#.into(), &[][..]),
    ] {
        for marker in ["", " <>", MARKER] {
            let source = man_head(&head.replace("$MARK", marker));
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let owner = EntryOwner::Definition(items[0]);
            assert_eq!(owner.validated_names().unwrap(), names, "{source}");
            assert_names(&query, names, &source);
            if head.contains("ordinary") && !marker.is_empty() {
                assert!(forms(owner)[0].contains("ordinary <>"), "{source}");
            }
        }
    }
}

#[test]
fn accepted_key_text_and_hard_rows_precede_reference_publication() {
    // CVS man_term.c::pre_OP feeds both operands through term.c::term_word:
    // the next glyph replaces the zero-advance Z, and \p creates a hard
    // row before <>. The latter cannot publish a continuous link label.
    for (key, names, links, form) in [
        (r"\-\-fake\p", &["--fake"][..], 0, "[--fake printf(1)\n<>]"),
        (r"\-\-\zZfake", &["--fake"][..], 1, "[--fake printf(1)]"),
        (r"\-\-", &[][..], 1, "[-- printf(1)]"),
        (
            r"\-\-fake\pSUFFIX",
            &["--fakeSUFFIX"][..],
            0,
            "[--fakeSUFFIX printf(1)\n<>]",
        ),
    ] {
        let source = man_head(&format!(r#".OP "{key}" "printf(1){MARKER}""#));
        let query = load(&source);
        let items = definitions(query.document.as_ref().unwrap());
        let owner = EntryOwner::Definition(items[0]);
        assert_eq!(owner.validated_names().unwrap(), names, "{source}");
        assert_eq!(forms(owner), [form], "{source}");
        assert_eq!(manual_links(&items[0].terms[0]), links, "{source}");
        assert!(mant_render::render_query_text(&query).contains("UNIQUE_BODY"));
    }
}

#[test]
fn marker_edits_move_only_the_affected_heads_in_tq() {
    for (heads, names) in [
        ([OP, r".B \-R", r".B \-S"], ["--color", "-R", "-S"]),
        ([r".B \-R", OP, r".B \-S"], ["-R", "--color", "-S"]),
        ([r".B \-R", r".B \-S", OP], ["-R", "-S", "--color"]),
    ] {
        for marker in ["", MARKER] {
            let heads = heads.join("\n.TQ\n").replace("$MARK", marker);
            let source = man_head(&heads);
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let [item] = items.as_slice() else {
                panic!("{source}")
            };
            let owner = EntryOwner::Definition(item);
            assert_eq!(owner.validated_names().unwrap(), names, "{source}");
            assert_names(&query, &names, &source);
            for (index, binding) in owner.facts().unwrap().name_bindings.iter().enumerate() {
                assert!(
                    binding
                        .occurrences
                        .iter()
                        .flat_map(|name| &name.parts)
                        .all(|part| part.root == EntryInlineRoot::Term { index }),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn same_coordinate_tq_expansions_publish_separate_names_and_owners() {
    // The pinned tree locates both TP/TQ pairs on the same .PAIRS line.
    // The OP value is formatted once per expansion in man_term.c::pre_OP.
    let op = OP.replace("$MARK", MARKER);
    let source = format!(
        "{MAN}.de PAIR\n.TP\n.BI \\-L \"dir,\\fB--fake\"\n.TQ\n{op}\n\\\\$1\n..\n.de PAIRS\n.PAIR FIRST_BODY\n.PAIR SECOND_BODY\n..\n.PAIRS\n"
    );
    let query = load(&source);
    let items = definitions(query.document.as_ref().unwrap());
    let [first, second] = items.as_slice() else {
        panic!("{source}")
    };
    assert_eq!(first.source, second.source);
    for item in items {
        let owner = EntryOwner::Definition(item);
        assert_eq!(owner.validated_names().unwrap(), ["-L", "--color"]);
    }
    let explanation = explain(&query, "--color");
    let hits: Vec<_> = explanation
        .evidence
        .iter()
        .filter(|hit| {
            hit.class == EvidenceClass::DirectEntry
                && hit
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        })
        .collect();
    let [a, b] = hits.as_slice() else {
        panic!("{explanation:?}")
    };
    assert_ne!(a.outline.path(), b.outline.path());
    for (hit, own, other) in [
        (a, "FIRST_BODY", "SECOND_BODY"),
        (b, "SECOND_BODY", "FIRST_BODY"),
    ] {
        let excerpt =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        let text = mant_render::render_excerpt_text(&excerpt);
        assert!(text.contains(own) && !text.contains(other), "{text}");
    }
    assert_eq!(explain(&query, "--fake").counts.direct_entry.total, 0);
}

fn group_bounds(document: &Document) -> Vec<(usize, usize)> {
    use mant_ir::visit::{self, Visit};
    #[derive(Default)]
    struct Groups(Vec<(usize, usize)>);
    impl<'ir> Visit<'ir> for Groups {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::DefinitionList {
                declaration_groups, ..
            } = block
            {
                self.0.extend(
                    declaration_groups
                        .iter()
                        .map(|group| (group.start_item, group.end_item)),
                );
            }
            visit::walk_block(self, block);
        }
    }
    let mut groups = Groups::default();
    groups.visit_document(document);
    groups.0
}

#[test]
fn reference_publication_leaves_the_declaration_group_provider_in_place() {
    for intervening in ["", ".PP\nSEPARATE\n", "FIRST_BODY\n"] {
        for marker in ["", MARKER] {
            let op = OP.replace("$MARK", marker);
            let source = format!("{MAN}.TP\n{op}\n{intervening}.TP\n.B \\-R\nUNIQUE_BODY\n");
            let query = load(&source);
            let document = query.document.as_ref().unwrap();
            let expected_bounds = if intervening.is_empty() {
                &[(0, 2)][..]
            } else {
                &[]
            };
            assert_eq!(group_bounds(document), expected_bounds, "{source}");
            let items = definitions(document);
            assert_eq!(items.len(), 2, "{source}");
            assert_eq!(
                items[0].entry.as_ref().unwrap().names,
                ["--color"],
                "{source}"
            );
            assert_eq!(items[1].entry.as_ref().unwrap().names, ["-R"], "{source}");
            assert_eq!(
                explain(&query, "--color").counts.direct_entry.total,
                1,
                "{source}"
            );
            assert_names(&query, &["-R"], &source);
            if intervening.is_empty() {
                assert!(items[0].description.is_empty(), "{source}");
            }
        }
    }
}
