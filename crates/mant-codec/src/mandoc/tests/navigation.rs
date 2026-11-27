//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

#[test]
fn list_paragraph_spacing_uses_native_items_instead_of_anchor_carriers() {
    // Exact 48-source tree/ASCII/UTF-8/HTML/lint runs precede these assertions.
    // mdoc_term.c::termp_bl_pre/post only call term_newln; It BLOCK pre
    // executes print_bvspace. A target-only IR carrier cannot create an It.
    // post_it() removes an entirely empty column It during normalization.
    for (style, arguments) in [
        ("tag", "-tag -width Ds"),
        ("plain", "-item"),
        ("column", "-column One Two"),
    ] {
        for compact in [false, true] {
            for paragraph in [false, true] {
                for variant in ["no-item", "anchor-only", "actual-item", "empty-item"] {
                    let payload = match variant {
                        "anchor-only" => ".Tg empty-target\n",
                        "actual-item" if style == "plain" => ".Tg item-target\n.It\nCONTENT\n",
                        "actual-item" => ".Tg item-target\n.It CONTENT\n",
                        "empty-item" => ".Tg item-target\n.It\n",
                        _ => "",
                    };
                    let source = format!(
                        ".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh NAME\n.Nm target\n.Nd list-boundary probe\n.Sh DESCRIPTION\nBEFORE\n.Bl {arguments}{}\n{payload}.El\n{}AFTER\n",
                        if compact { " -compact" } else { "" },
                        if paragraph { ".Pp\n" } else { "" },
                    );
                    let case = format!("{style}/{compact}/{paragraph}/{variant}");
                    let path = std::path::Path::new("native-item-spacing.1");
                    let mut report = Parser::default()
                        .parse_bytes(path, source.as_bytes())
                        .expect("parse native list spacing source");
                    let list = find_macro_mut(&mut report.document.root, "Bl")
                        .expect("normalized native Bl");
                    let has_native_items =
                        variant == "actual-item" || (variant == "empty-item" && style != "column");
                    assert_eq!(
                        find_macro_mut(list, "It").is_some(),
                        has_native_items,
                        "native It ownership: {case}"
                    );
                    let document = lower_mandoc_document(path, &report);
                    let blocks = &document.sections[1].blocks;
                    let layout = blocks
                        .iter()
                        .find_map(|block| match block {
                            Block::List { layout, .. }
                            | Block::DefinitionList { layout, .. }
                            | Block::Table { layout, .. } => Some(layout),
                            _ => None,
                        })
                        .expect("list or target-only list carrier");
                    assert_eq!(
                        layout.spacing_before_lines,
                        u16::from(has_native_items && !compact),
                        "native It pre distance: {case}"
                    );
                }
            }
        }
    }
}

#[test]
fn target_identity_is_independent_from_optional_raw_source_recovery() {
    let path = std::path::Path::new("target-source-parity.7");
    let source = b".Dd September 4, 2026\n.Dt TARGET-SOURCE-PARITY 7\n.Os\n\
.Tg Mixed.Section\n\
.Sh HEADING\n\
.Pp\n\
Paragraph before a target request.\n\
.Tg\n\
.Ic derived-command\n\
.Pp\n\
.Fn automatic_function\n";
    let with_source = parse_manual_bytes(path, source).expect("lower source-aware document");
    let report = Parser::default()
        .parse_bytes(path, source)
        .expect("parse owned native tree");
    let without_source = lower_mandoc_document(path, &report);

    let with_source_index = mant_ir::DocumentIndex::build(&with_source);
    let without_source_index = mant_ir::DocumentIndex::build(&without_source);
    for target in ["Mixed.Section", "derived-command", "automatic_function"] {
        assert_eq!(
            with_source_index
                .fragment_target(target)
                .map(mant_ir::NodeId::as_str),
            without_source_index
                .fragment_target(target)
                .map(mant_ir::NodeId::as_str),
            "target {target} changed when raw source recovery was unavailable"
        );
    }
}

#[test]
fn semantic_identity_uses_the_same_composite_glyph_projection_as_visible_text() {
    let path = std::path::Path::new("semantic-composite-glyph.1");
    let source = b".TH SEMANTIC-COMPOSITE 1\n\
.SH OPTIONS\n\
.TP\n\
.B A\\z\\o'BC'D\n\
Description.\n";
    let document = parse_manual_bytes(path, source).expect("lower term with overstrike glyph");
    let index = mant_ir::DocumentIndex::build(&document);

    assert!(
        index.contains("term-ad"),
        "semantic projection: {document:#?}"
    );
    assert!(!index.contains("term-acd"));
}

#[test]
fn native_section_ids_ignore_unrelated_section_insertions() {
    let original = native_section_ids(&["NAME", "OPTIONS"]);
    let edited = native_section_ids(&["NOTES", "NAME", "OPTIONS", "OPTIONS"]);
    assert_eq!(&edited[1..3], &original);
    assert_eq!(edited, ["notes", "name", "options", "options-2"]);
}

#[test]
fn native_section_ids_disambiguate_final_slug_collisions() {
    assert_eq!(
        native_section_ids(&["FOO", "FOO", "FOO 2"]),
        ["foo", "foo-2", "foo-2-2"]
    );
}

fn native_section_ids(titles: &[&str]) -> Vec<String> {
    let mut ids = crate::producer_identity::HeadingIds::default();
    let claims = titles
        .iter()
        .map(|title| {
            let base = crate::mandoc::section_identity::section_id_base(title);
            let full = ids.claim_full(&base);
            (base, full)
        })
        .collect::<Vec<_>>();
    claims
        .into_iter()
        .map(|(base, full)| ids.complete(&base, full).0)
        .collect()
}

#[test]
fn authored_macro_target_survives_repaint_without_adopting_a_sibling() {
    #[derive(Default)]
    struct Position {
        ink: String,
        hits: Vec<usize>,
    }
    impl<'ir> Visit<'ir> for Position {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Anchor {
                    id,
                    fragment_aliases,
                    ..
                } if id.as_str() == "body-target" => {
                    assert_eq!(fragment_aliases.len(), 1);
                    assert_eq!(fragment_aliases[0].as_str(), "Body.Target");
                    self.hits.push(self.ink.len());
                }
                Inline::Text { value } | Inline::Code { value } => self
                    .ink
                    .extend(value.chars().filter(|ch| !ch.is_whitespace())),
                _ => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    // Pinned CVS 20261013T025419Z: mdoc_validate.c::post_tg assigns
    // the target to Ev; terminal ASCII and UTF-8 agree on the survivors.
    // The sibling No repaint does not acquire Ev's target even though it
    // occupies Ev's former columns. An entirely empty Ev remains zero-width.
    for (line, expected, target) in [
        (r#".Ev "\%<>" K"#, "JA<>K", Some(2)),
        (r#".Ev "\%<>" K No "\h'|6n'S""#, "JAS>K", Some(3)),
        (r#".Ev "\%<>" K No "\h'|6n'ST""#, "JASTK", Some(4)),
        (r#".Ev "<>" K No "\h'|5n'ST""#, "JASTK", Some(4)),
        (
            r#".Ev "\X'tty: link https://a.test'\%<>\X'tty: link'" K No "\h'|7n'ST""#,
            "JASTK",
            Some(4),
        ),
        (r#".Ev "\%<>" No "\h'|6n'ST""#, "JAST", None),
    ] {
        let source = format!(
            ".Dd November 26, 2026\n.Dt INSPECTION 1\n.Os\n.Sh NAME\n.Nm inspection\n.Nd annotations\n.Sh DESCRIPTION\n.No JA\n.Tg Body.Target\n{line}\n.Sh ENDTEST\nDone.\n"
        );
        let document = parse_manual_bytes(std::path::Path::new("inspection.1"), source.as_bytes())
            .expect("valid mdoc page");
        let mut position = Position::default();
        for block in &document.sections[1].blocks {
            position.visit_block(block);
        }
        assert_eq!(position.ink, expected, "{line}");
        assert_eq!(position.hits.len(), 1, "{line}");
        if let Some(expected) = target {
            assert_eq!(position.hits[0], expected, "{line}");
        }
    }
}
