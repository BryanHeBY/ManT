use std::{collections::HashSet, fmt::Write as _, fs, process};

use mant_ir::{
    Block, DiagnosticLevel, Inline, ListKind, ResolvedContent, SemanticIndex, SourceFormat,
    ValueDomain,
    visit::{self, Visit},
};

use super::{
    LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Parser, lower_mandoc_document,
    parse_manual_bytes, parse_manual_source,
};

mod entry_forms;
mod flow_controls;
mod upstream_inline;
mod upstream_tables;

fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
    fs::write(&path, source).expect("write temporary roff fixture");
    path
}

fn anchor_ids(document: &mant_ir::Document) -> Vec<String> {
    struct AnchorCollector(Vec<String>);

    impl<'ir> Visit<'ir> for AnchorCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id, .. } = inline {
                self.0.push(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = AnchorCollector(Vec::new());
    collector.visit_document(document);
    collector.0
}

fn anchor_owner_lines(document: &mant_ir::Document) -> Vec<(String, u32)> {
    struct AnchorCollector(Vec<(String, u32)>);

    impl<'ir> Visit<'ir> for AnchorCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor {
                id,
                owner_source: Some(source),
                ..
            } = inline
            {
                self.0.push((id.to_string(), source.line));
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = AnchorCollector(Vec::new());
    collector.visit_document(document);
    collector.0
}

fn visible_document_text(document: &mant_ir::Document) -> String {
    struct TextCollector(String);

    impl<'ir> Visit<'ir> for TextCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value } | Inline::Code { value } => {
                    self.0.push_str(value);
                    self.0.push(' ');
                }
                Inline::LineBreak => self.0.push('\n'),
                Inline::Strong { .. }
                | Inline::Emphasis { .. }
                | Inline::Link { .. }
                | Inline::Anchor { .. } => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = TextCollector(String::new());
    collector.visit_document(document);
    collector.0
}

#[test]
fn native_section_ids_ignore_unrelated_section_insertions() {
    let mut original = LoweringContext::new(None, None);
    let original_name = original.section_id("NAME");
    let original_options = original.section_id("OPTIONS");

    let mut edited = LoweringContext::new(None, None);
    assert_eq!(edited.section_id("NOTES"), "notes");
    assert_eq!(edited.section_id("NAME"), original_name);
    assert_eq!(edited.section_id("OPTIONS"), original_options);
    assert_eq!(edited.section_id("OPTIONS"), "options-2");
}

#[test]
fn native_section_ids_disambiguate_final_slug_collisions() {
    let mut context = LoweringContext::new(None, None);
    assert_eq!(context.section_id("FOO"), "foo");
    assert_eq!(context.section_id("FOO"), "foo-2");
    assert_eq!(context.section_id("FOO 2"), "foo-2-2");
}

#[test]
fn reads_incrementing_registers_only_from_ip_markers() {
    let context = LoweringContext::new(
        None,
        Some(".IP \\n+[step] 4\n.IP 1 \\n+[width]\n.IPX \\n+[other]\n'IP \"\\n+[quoted]\" 4\n"),
    );

    assert!(context.man_ip_uses_incrementing_register(1));
    assert!(!context.man_ip_uses_incrementing_register(2));
    assert!(!context.man_ip_uses_incrementing_register(3));
    assert!(context.man_ip_uses_incrementing_register(4));
}

fn find_macro_mut<'a>(
    node: &'a mut libmandoc_rs::Node,
    name: &str,
) -> Option<&'a mut libmandoc_rs::Node> {
    if node.macro_name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_macro_mut(child, name))
}

fn replace_first_text(node: &mut libmandoc_rs::Node, value: &str) -> bool {
    if let Some(text) = node.text.as_mut() {
        *text = value.to_owned();
        return true;
    }
    node.children
        .iter_mut()
        .any(|child| replace_first_text(child, value))
}

#[test]
fn standalone_inputs_reject_redirect_only_so_pages() {
    let error = parse_manual_bytes(std::path::Path::new("stdin"), b".so man1/target.1\n")
        .expect_err("standalone input must not follow another file");
    assert!(error.to_string().contains("require MANPATH discovery"));
}

#[test]
fn unclosed_compact_run_does_not_cross_a_section_boundary() {
    let path = temporary_source(
        "section-bounded-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH FIRST\n\
         .TP\n\
         .PD 0\n\
         first\n\
         .SH SECOND\n\
         .TP\n\
         second\n\
         .PD\n\
         Second description.\n",
    );

    let document = parse_manual_source(&path).expect("lower section-bounded compact run");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [first, second] = document.sections.as_slice() else {
        panic!("expected two sections");
    };
    let [
        Block::DefinitionList {
            items: first_items, ..
        },
    ] = first.blocks.as_slice()
    else {
        panic!("expected first definition list");
    };
    let [
        Block::DefinitionList {
            items: second_items,
            ..
        },
    ] = second.blocks.as_slice()
    else {
        panic!("expected second definition list");
    };
    assert_eq!(first_items.len(), 1);
    assert_eq!(inline_text(&first_items[0].terms[0]), "first");
    assert!(first_items[0].description.is_empty());
    assert_eq!(second_items.len(), 1);
    assert_eq!(inline_text(&second_items[0].terms[0]), "second");
    assert!(!second_items[0].description.is_empty());
}

#[test]
fn adjacent_no_fill_regions_scale_without_changing_their_topology() {
    const REGION_COUNT: usize = 2_048;
    let mut source = String::from(".TH NO-FILL-SCALE 7\n.SH EXAMPLE\n");
    for index in 0..REGION_COUNT {
        writeln!(source, ".nf\nline {index}\n.fi").expect("append no-fill region");
    }

    let document = parse_manual_bytes(std::path::Path::new("no-fill-scale.7"), source.as_bytes())
        .expect("lower adjacent no-fill regions");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "adjacent regions must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        REGION_COUNT - 1
    );
    assert!(inline_text(children).starts_with("line 0\nline 1\n"));
    assert!(
        inline_text(children).ends_with(&format!("line {}", REGION_COUNT - 1)),
        "last no-fill region must remain visible"
    );
}

#[test]
fn lets_explicit_fonts_override_an_alternating_macro_default() {
    let path = temporary_source(
        "alternating-font-reset",
        ".TH MAN 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .BI \\-r\\  prompt \\fR,\\ \\fB\\-\\-prompt= prompt\n\
         Set the pager prompt.\n",
    );

    let document = parse_manual_source(&path).expect("lower alternating font reset");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    let term = items[0]
        .terms
        .first()
        .expect("first definition term")
        .iter()
        .filter(|inline| !matches!(inline, Inline::Anchor { .. }))
        .collect::<Vec<_>>();

    assert_eq!(term.len(), 5);
    assert!(matches!(term[0], Inline::Strong { children } if inline_text(children) == "-r "));
    assert!(matches!(term[1], Inline::Emphasis { children } if inline_text(children) == "prompt"));
    assert!(matches!(term[2], Inline::Text { value } if value == ", "));
    assert!(matches!(term[3], Inline::Strong { children } if inline_text(children) == "--prompt="));
    assert!(matches!(term[4], Inline::Emphasis { children } if inline_text(children) == "prompt"));
}

#[test]
fn suppresses_pod_font_requests_around_verbatim_blocks() {
    let path = temporary_source(
        "pod-verbatim-fonts",
        ".de Vb\n\
         .ft CW\n\
         .nf\n\
         ..\n\
         .de Ve\n\
         .ft R\n\
         .fi\n\
         ..\n\
         .TH POD 1\n\
         .SH EXAMPLES\n\
         .Vb 2\n\
         \\&struct A { int a; };\n\
         \\&struct B : A {};\n\
         .Ve\n",
    );

    let document = parse_manual_source(&path).expect("lower Pod::Man verbatim source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.sections[0].blocks.len(), 1);
    let Block::Preformatted { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one preformatted block");
    };
    assert_eq!(
        inline_text(children),
        "struct A { int a; };\nstruct B : A {};"
    );
}

#[test]
fn tq_continuation_starts_at_the_immediately_preceding_head() {
    let path = temporary_source(
        "bounded-tq-aliases",
        ".TH TQ-BOUNDARY 7\n\
         .SH OPTIONS\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         .TQ\n\
         --beta\n\
         Description only for beta.\n",
    );

    let document = parse_manual_source(&path).expect("lower bounded TQ definitions");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "-a");
    assert!(items[0].description.is_empty());
    assert_eq!(
        items[1]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-b", "--beta"]
    );
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.definition-alias-boundary")
    }));
}

#[test]
fn preserves_man_paragraph_and_heading_distance_as_one_layout_model() {
    let path = temporary_source(
        "vertical-layout",
        ".TH SPACING 1\n\
         .SH FIRST\n\
         First paragraph.\n\
         .PP\n\
         Second paragraph.\n\
         .SS CHILD\n\
         Child body.\n\
         .PD 0\n\
         .SS COMPACT\n\
         Compact child.\n\
         .SH NEXT\n\
         Next body.\n\
         .PD\n\
         .SH FINAL\n\
         Final body.\n",
    );

    let document = parse_manual_source(&path).expect("lower vertical layout");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [first, next, final_section] = document.sections.as_slice() else {
        panic!("expected three top-level sections");
    };
    assert_eq!(first.spacing_before_lines, 0);
    let [Block::Paragraph { .. }, Block::Paragraph { layout, .. }] = first.blocks.as_slice() else {
        panic!("expected two semantic paragraphs");
    };
    assert_eq!(layout.spacing_before_lines, 1);

    let [child, compact] = first.children.as_slice() else {
        panic!("expected two subsections");
    };
    assert_eq!(child.spacing_before_lines, 1);
    assert_eq!(compact.spacing_before_lines, 0);
    assert_eq!(next.spacing_before_lines, 0);
    assert_eq!(final_section.spacing_before_lines, 1);
}

#[test]
fn preserves_mdoc_paragraph_and_heading_distance() {
    let path = temporary_source(
        "mdoc-vertical-layout",
        ".Dd July 19, 2026\n\
         .Dt SPACING 1\n\
         .Os\n\
         .Sh FIRST\n\
         First paragraph.\n\
         .Pp\n\
         Second paragraph.\n\
         .Ss CHILD\n\
         Child body.\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc vertical layout");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [first] = document.sections.as_slice() else {
        panic!("expected one top-level section");
    };
    assert_eq!(first.spacing_before_lines, 1);
    assert!(matches!(
        first.blocks.get(1),
        Some(Block::VerticalSpace { lines: 1, .. })
    ));
    assert_eq!(first.children[0].spacing_before_lines, 1);
}

#[test]
fn preserves_complete_mdoc_include_directives() {
    let document = parse_manual_bytes(
        std::path::Path::new("include.3"),
        b".Dd August 19, 2026\n.Dt INCLUDE 3\n.Os\n.Sh SYNOPSIS\n.In fido.h\n",
    )
    .expect("lower mdoc include");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one include paragraph");
    };
    assert_eq!(inline_text(children), "#include <fido.h>");
    assert!(matches!(
        children.as_slice(),
        [Inline::Code { value }] if value == "#include <fido.h>"
    ));
}

#[test]
fn lowers_documented_mdoc_delimiters_and_common_roff_characters() {
    let path = temporary_source(
        "mdoc-delimiters",
        ".Dd July 19, 2026\n\
         .Dt DELIMITERS 7\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         .Op optional\n\
         .Bq bracket\n\
         .Dq double\n\
         .Sq single\n\
         .Pq parenthesized\n\
         .Brq braced\n\
         .Aq angled\n\
         .Oo multi Ar value\n\
         .Oc\n\
         .Sh CHARACTERS\n\
         \\(en \\(em \\(aq \\(dq \\(co \\(rg \\(tm \\(bu \\(ha \\(ti \\(rs\n",
    );

    let document = parse_manual_source(&path).expect("lower delimiter and character source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let description = document.sections[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "[optional]",
        "[bracket]",
        "“double”",
        "‘single’",
        "(parenthesized)",
        "{braced}",
        "<angled>",
        "[multi value]",
    ] {
        assert!(
            description.contains(expected),
            "missing {expected:?} in {description:?}"
        );
    }

    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("expected one special-character paragraph");
    };
    assert_eq!(inline_text(children), "– — ' \" © ® ™ • ^ ~ \\");
}

#[test]
fn retains_punctuation_after_implicit_mdoc_enclosures() {
    let document = parse_manual_bytes(
        std::path::Path::new("implicit-enclosure-punctuation.7"),
        b".Dd August 19, 2026\n.Dt IMPLICIT-ENCLOSURE-PUNCTUATION 7\n.Os\n\
.Sh DESCRIPTION\nWhen disabled\n.Pq all features remain readable ;\ncontinue safely.\n",
    )
    .expect("lower punctuation after an implicit enclosure");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one paragraph");
    };
    assert_eq!(
        inline_text(children),
        "When disabled (all features remain readable); continue safely."
    );
}

#[test]
fn lowers_the_pinned_named_character_catalog_without_silent_deletion() {
    let document = parse_manual_bytes(
        std::path::Path::new("named-characters.7"),
        b".TH NAMED-CHARACTERS 7\n\
.SH TEST\n\
at=\\(at ga=\\(ga oq=\\(oq arrow=\\(-> larrow=\\(<- mu=\\(mu\n\
de=\\(de pl=\\(pl dg=\\(dg ua=\\(ua da=\\(da lB=\\(lB rB=\\(rB\n\
unknown=\\[future-glyph]\n",
    )
    .expect("lower named characters");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one character paragraph");
    };
    assert_eq!(
        inline_text(children),
        "at=@ ga=` oq=' arrow=→ larrow=← mu=× de=° pl=+ dg=† ua=↑ da=↓ lB=[ rB=] unknown=\\[future-glyph]"
    );
}

#[test]
fn round_trips_raw_and_bracketed_unicode_manual_text() {
    let source = ".TH UNICODE 7\n\
.SH TEST\n\
Raw UTF-8: Mašláňová café — naïve.\n\
Escaped: Ma\\[u0161]l\\[u00E1] and \\[u2014] dash.\n";
    let document = parse_manual_bytes(std::path::Path::new("unicode.7"), source.as_bytes())
        .expect("lower raw and escaped Unicode");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one Unicode paragraph");
    };
    let rendered = inline_text(children);
    assert!(rendered.contains("Raw UTF-8: Mašláňová café — naïve."));
    assert!(rendered.contains("Escaped: Mašlá and — dash."));
    assert!(!rendered.contains(r"\[u"));
}

#[test]
fn preserves_explicit_mdoc_function_and_enclosure_structure() {
    let document = parse_manual_bytes(
        std::path::Path::new("explicit-mdoc.1"),
        b".Dd August 17, 2026\n\
.Dt EXPLICIT-MDOC 1\n\
.Os\n\
.Sh NAME\n\
.Nm explicit-mdoc\n\
.Nd exercise explicit blocks\n\
.Sh FUNCTION\n\
.Ft int\n\
.Fo audit_open\n\
.Fa \"const char *path\"\n\
.Fa \"int flags\"\n\
.Fc\n\
.Sh ENCLOSURES\n\
.Ao\nangle\n.Ac\n\
.Bo\nbracket\n.Bc\n\
.Do\ndouble\n.Dc\n\
.Po\nparenthesized\n.Pc\n\
.Qo\nquoted\n.Qc\n\
.So\nsingle\n.Sc\n\
.Bro\nbraced\n.Brc\n\
.Oo\noptional\n.Oc\n\
.Eo <<\ngeneric\n.Ec >>\n\
.Es [[ ]]\n\
.En custom\n",
    )
    .expect("lower explicit mdoc blocks");

    let function = &document.sections[1];
    let [
        Block::Paragraph {
            children: return_type,
            ..
        },
        Block::Paragraph {
            children: declaration,
            ..
        },
    ] = function.blocks.as_slice()
    else {
        panic!("expected return type and function declaration paragraphs");
    };
    assert_eq!(inline_text(return_type), "int");
    assert_eq!(
        inline_text(declaration),
        "audit_open(const char *path, int flags)"
    );
    assert!(declaration.iter().any(|inline| matches!(
        inline,
        Inline::Strong { children } if inline_text(children) == "audit_open"
    )));
    assert!(anchor_ids(&document).iter().any(|id| id == "audit-open"));

    let [Block::Paragraph { children, .. }] = document.sections[2].blocks.as_slice() else {
        panic!("expected one enclosure paragraph");
    };
    assert_eq!(
        inline_text(children),
        "<angle> [bracket] “double” (parenthesized) \"quoted\" ‘single’ {braced} \
         [optional] <<generic>> [[custom]]"
    );
    assert_eq!(document.diagnostics.len(), 2);
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.starts_with("obsolete macro:")),
        "{:?}",
        document.diagnostics
    );
}

#[test]
fn preserves_the_complete_libbsd_library_identity() {
    let document = parse_manual_bytes(
        std::path::Path::new("libbsd.3bsd"),
        b".Dd August 19, 2026\n.Dt LIBBSD 3bsd\n.Os\n.Sh LIBRARY\n.Lb libbsd\n",
    )
    .expect("lower libbsd library declaration");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one library paragraph");
    };

    assert_eq!(
        inline_text(children),
        "Utility functions from BSD systems (libbsd, -lbsd)"
    );
}

#[test]
fn joins_the_final_mdoc_bibliography_authors() {
    let document = parse_manual_bytes(
        std::path::Path::new("bibliography.3"),
        b".Dd August 19, 2026\n.Dt BIBLIOGRAPHY 3\n.Os\n.Sh SEE ALSO\n\
.Rs\n.%A Bentley, J.L.\n.%A McIlroy, M.D.\n.%T Engineering a Sort Function\n.Re\n",
    )
    .expect("lower mdoc bibliography");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one bibliography paragraph");
    };

    assert_eq!(
        inline_text(children),
        "Bentley, J.L. and McIlroy, M.D. Engineering a Sort Function."
    );
}

#[test]
fn preserves_mdoc_name_and_function_punctuation_by_context() {
    let document = parse_manual_bytes(
        std::path::Path::new("function-punctuation.3"),
        b".Dd August 19, 2026\n.Dt FUNCTION-PUNCTUATION 3\n.Os\n\
.Sh NAME\n.Nm function-punctuation\n.Nd test generated punctuation\n\
.Sh SYNOPSIS\n.Fn compact_call \"int value\"\n\
.Fo explicit_call\n.Fa \"int value\" \"const char *label\"\n.Fc\n\
.Sh DESCRIPTION\nThe\n.Fn prose_call \"int value\"\nfunction.\n",
    )
    .expect("lower mdoc generated punctuation");

    let [Block::Paragraph { children: name, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one NAME paragraph");
    };
    assert_eq!(
        inline_text(name),
        "function-punctuation — test generated punctuation"
    );

    let synopsis = document.sections[1]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        synopsis,
        [
            "compact_call(int value);",
            "explicit_call(int value, const char *label);"
        ]
    );

    let [
        Block::Paragraph {
            children: description,
            ..
        },
    ] = document.sections[2].blocks.as_slice()
    else {
        panic!("expected one DESCRIPTION paragraph");
    };
    assert_eq!(
        inline_text(description),
        "The prose_call(int value) function."
    );
}

#[test]
fn preserves_mdoc_synopsis_declaration_units() {
    let document = parse_manual_bytes(
        std::path::Path::new("synopsis-declarations.3"),
        b".Dd August 19, 2026\n.Dt SYNOPSIS-DECLARATIONS 3\n.Os\n\
.Sh SYNOPSIS\n.In synprobe.h\n.Ft const struct stat *\n\
.Fn synprobe_first \"struct thing *a\"\n.Ft void\n\
.Fo synprobe_second\n.Fa \"struct thing *a\"\n.Fa \"int n\"\n.Fc\n\
.Fn synprobe_third \"int n\"\n",
    )
    .expect("lower mdoc synopsis declarations");

    let rendered = document.sections[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis declaration paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        [
            "#include <synprobe.h>",
            "const struct stat * synprobe_first(struct thing *a);",
            "void synprobe_second(struct thing *a, int n);",
            "synprobe_third(int n);",
        ]
    );
}

#[test]
fn diagnoses_future_structural_macros_before_discarding_visible_parts() {
    let mut report = Parser::default()
        .parse_bytes(
            "future-structure.1",
            b".Dd August 17, 2026\n.Dt FUTURE 1\n.Os\n.Sh SYNOPSIS\n\
.Fo future_call\n.Fa argument\n.Fc\n",
        )
        .expect("parse structural fixture");
    let block = find_macro_mut(&mut report.document.root, "Fo").expect("Fo block");
    block.macro_name = Some("FutureBlock".to_owned());
    let mut second_body = block
        .children
        .iter()
        .find(|child| child.kind == libmandoc_rs::NodeKind::Body)
        .cloned()
        .expect("function body");
    assert!(replace_first_text(&mut second_body, "second_argument"));
    block.children.push(second_body);

    let document = lower_mandoc_document(std::path::Path::new("future-structure.1"), &report);

    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.unhandled-structural-parts")
            && diagnostic.message.contains("FutureBlock")
    }));
    let rendered = document.sections[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected fallback paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, ["argument", "second_argument"]);
}

#[test]
fn recognizes_explicitly_styled_traditional_man_references_in_any_section() {
    let path = temporary_source(
        "man-see-also",
        ".TH TOOL 1\n\
         .SH DESCRIPTION\n\
         The styled reference \\fBprintf\\fP(3) is usable here.\n\
         .SH SEE ALSO\n\
         .BR printf (3),\n\
         .BR man (1)\n",
    );

    let document = parse_manual_source(&path).expect("lower man references");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let see_also = document
        .sections
        .iter()
        .find(|section| section.title == "SEE ALSO")
        .expect("SEE ALSO");
    let Block::Paragraph { children, .. } = &see_also.blocks[0] else {
        panic!("references are a paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
            if name == "printf" && manual_section == "3"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
            if name == "man" && manual_section == "1"
    )));

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("description is a paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
            if name == "printf" && manual_section == "3"
    )));
}

#[test]
fn lowers_modern_groff_manual_uri_and_mail_macros() {
    let path = temporary_source(
        "man-modern-links",
        ".TH TOOL 1\n\
         .SH DESCRIPTION\n\
         .MR git-add 1 ,\n\
         .PP\n\
         Read\n\
         .UR https://example.test/docs\n\
         Documentation\n\
         .UE\n\
         now.\n\
         .PP\n\
         Mail comments, suggestions and bug reports to\n\
         .MT docs@example.test\n\
         Sean\n\
         .ME .\n",
    );

    let document = parse_manual_source(&path).expect("lower modern man links");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let section = &document.sections[0];
    let mut manual = false;
    let mut web = false;
    let mut mail = false;
    for children in section.blocks.iter().filter_map(|block| match block {
        Block::Paragraph { children, .. } => Some(children),
        _ => None,
    }) {
        for inline in children {
            match inline {
                Inline::Link {
                    target:
                        mant_ir::LinkTarget::Manual {
                            name,
                            manual_section: Some(manual_section),
                        },
                    ..
                } if name == "git-add" && manual_section == "1" => manual = true,
                Inline::Link {
                    target: mant_ir::LinkTarget::External { uri },
                    ..
                } if uri == "https://example.test/docs" => {
                    web = true;
                }
                Inline::Link {
                    target: mant_ir::LinkTarget::Email { address },
                    ..
                } if address == "docs@example.test" => {
                    mail = true;
                }
                _ => {}
            }
        }
    }

    assert!(manual && web && mail);
    assert!(section.blocks.iter().any(|block| match block {
        Block::Paragraph { children, .. } => inline_text(children).contains("git-add(1),"),
        _ => false,
    }));
    let linked_paragraphs = section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. }
                if children.iter().any(|inline| {
                    matches!(
                        inline,
                        Inline::Link {
                            target: mant_ir::LinkTarget::External { .. },
                            ..
                        } | Inline::Link {
                            target: mant_ir::LinkTarget::Email { .. },
                            ..
                        }
                    )
                }) =>
            {
                Some(inline_text(children))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        linked_paragraphs,
        [
            "Read Documentation ⟨https://example.test/docs⟩ now.",
            "Mail comments, suggestions and bug reports to Sean ⟨docs@example.test⟩."
        ]
    );
}

#[test]
fn resolves_a_unique_parenthetically_qualified_mdoc_section_reference() {
    let path = temporary_source(
        "mdoc-qualified-navigation",
        ".Dd July 19, 2026\n\
         .Dt NAVIGATION 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         See\n\
         .Sx White Space Splitting\n\
         .Sh \"White Space Splitting (Field Splitting)\"\n\
         Target content.\n",
    );

    let document = parse_manual_source(&path).expect("lower qualified navigation source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected navigation paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            children,
            ..
        } if id == "white-space-splitting-field-splitting"
            && inline_text(children) == "White Space Splitting"
    )));
    assert!(
        document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        })
    );
}

#[test]
fn degrades_unresolved_mdoc_section_references_to_text() {
    let path = temporary_source(
        "mdoc-missing-section",
        ".Dd July 19, 2026\n.Dt NAVIGATION 1\n.Os\n.Sh DESCRIPTION\n.Sx MISSING\n",
    );

    let document = parse_manual_source(&path).expect("lower unresolved navigation source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected reference paragraph");
    };
    assert_eq!(inline_text(children), "MISSING");
    assert!(children.iter().all(|inline| !matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Section { .. },
            ..
        }
    )));
    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        })
    );
}

#[test]
fn turns_captured_parser_findings_into_structured_diagnostics() {
    let path = temporary_source(
        "unsupported",
        ".Dd July 19, 2026\n.Dt BAD 1\n.Os\n.Sh NAME\n.Nm bad\n.ab\n",
    );

    let document = parse_manual_source(&path).expect("best-effort parse");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Unsupported)
    );
}

#[test]
fn masks_terminal_controls_before_native_parsing() {
    let path = temporary_source("controls", ".TH SAFE 1\n.SH NAME\nsafe \x1b[2J text\n");

    let document = parse_manual_source(&path).expect("parse sanitized manual");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code.as_deref() == Some("manual.control-characters") })
    );
}

#[test]
fn recovers_complete_numbered_sequences_from_mdoc_tag_lists() {
    let document = parse_manual_bytes(
        std::path::Path::new("mdoc-tag-enumeration.4"),
        b".Dd September 4, 2026\n.Dt MDOC-TAG-ENUMERATION 4\n.Os\n.Sh EXAMPLES\n\
.Bl -tag -width \"1.\"\n\
.It 1.\nFirst step.\n\
.Tg second-step\n\
.It 2.\nSecond step.\n\
.It 3.\nThird step.\n\
.El\n\
.Bl -tag -width \"1.\"\n\
.It 1.\nA real singleton definition.\n\
.El\n\
.Bl -tag -width \"1.\"\n\
.It 1.\nFirst non-sequence term.\n\
.It 3.\nThird non-sequence term.\n\
.El\n",
    )
    .expect("lower mdoc tag lists with numeric terms");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::List {
            kind: ListKind::Ordered,
            start: Some(1),
            ref items,
            ..
        } if items.len() == 3
    ));
    let Block::List { items, .. } = &document.sections[0].blocks[0] else {
        unreachable!("numbered tag list was asserted above")
    };
    assert!(items.iter().all(|item| {
        matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 4)
    }));
    assert!(matches!(
        document.sections[0].blocks[1],
        Block::DefinitionList { ref items, .. } if items.len() == 1
    ));
    assert!(matches!(
        document.sections[0].blocks[2],
        Block::DefinitionList { ref items, .. } if items.len() == 2
    ));
    assert!(
        SemanticIndex::build(&document)
            .section("examples")
            .iter()
            .all(|entry| entry.names.iter().all(|alias| alias != "2."))
    );
    assert!(
        mant_ir::DocumentIndex::build(&document)
            .fragment_target("second-step")
            .is_some()
    );
    let rendered = crate::render_query_text(&ResolvedContent {
        label: "mdoc-tag-enumeration".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    });
    assert!(rendered.contains("1.     First step.\n2.     Second step.\n3.     Third step."));
    assert!(!rendered.contains("1.         First step."));
}

#[test]
fn keeps_man_ordinal_boundaries_explicit_without_reclassifying_numeric_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("ordinal-boundaries.1"),
        b".TH ORDINAL-BOUNDARIES 1\n.SH BREAKS\n\
.IP 1. 4\none\n.IP 3. 4\nthree\n.IP 1) 4\nparen\n.IP 2. 4\nperiod\n\
.SH MIXED\n.IP 1. 4\none\n.TP\n.B 2.\ntwo\n\
.SH VALUES\n.TP\n.B 1\none\n.TP\n.B 2.2\ndecimal\n.TP\n.B v1.\nversion\n.TP\n.B 1.2.\nrelease\n",
    )
    .expect("lower ordinal boundaries");

    let starts = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::List {
                kind: ListKind::Ordered,
                start,
                ..
            } => *start,
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts, [1, 3, 1, 2]);
    assert!(matches!(
        document.sections[1].blocks.as_slice(),
        [Block::List {
            kind: ListKind::Ordered,
            start: Some(1),
            items,
            ..
        }] if items.len() == 2
    ));
    assert!(matches!(
        document.sections[2].blocks.as_slice(),
        [Block::DefinitionList { items, .. }] if items.len() == 4
    ));
}

#[test]
fn keeps_each_adjacent_rs_scope_in_the_current_ordinal_item() {
    let document = parse_manual_bytes(
        std::path::Path::new("ordinal-continuations.1"),
        b".TH ORDINAL-CONTINUATIONS 1\n.SH NOTES\n\
.IP 1. 4\none\n.RS 4\nfirst continuation\n.RE\n.RS 4\nsecond continuation\n.RE\n\
.PP\nseparate paragraph\n.IP 2. 4\ntwo\n",
    )
    .expect("lower adjacent relative-indent continuations");

    assert!(matches!(
        document.sections[0].blocks.as_slice(),
        [
            Block::List {
                kind: ListKind::Ordered,
                start: Some(1),
                items: first,
                ..
            },
            Block::Paragraph { .. },
            Block::List {
                kind: ListKind::Ordered,
                start: Some(2),
                items: second,
                ..
            }
        ] if first.len() == 1 && first[0].blocks.len() == 3 && second.len() == 1
    ));
}

#[test]
fn lowers_normalized_mdoc_font_and_author_layout() {
    let path = temporary_source(
        "normalized-mdoc-modes",
        ".Dd July 19, 2026\n\
         .Dt NORMALIZED-MODES 1\n\
         .Os\n\
         .Sh AUTHORS\n\
         .An -split\n\
         .An Alice Example\n\
         .An Bob Example\n\
         .An -nosplit\n\
         .An Carol Example\n\
         .An Dave Example\n\
         .Sh DESCRIPTION\n\
         .Bf -literal\n\
         literal text\n\
         .Ef\n",
    );

    let document = parse_manual_source(&path).expect("lower normalized mdoc modes");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let authors = &document.sections[0];
    let Block::Paragraph { children, .. } = &authors.blocks[0] else {
        panic!("authors are one paragraph");
    };
    assert_eq!(
        inline_text(children),
        "Alice Example\nBob Example Carol Example Dave Example"
    );

    let description = &document.sections[1];
    let Block::Paragraph { children, .. } = &description.blocks[0] else {
        panic!("font block is a paragraph");
    };
    assert!(matches!(
        children.as_slice(),
        [Inline::Code { value }] if value == "literal text"
    ));
}

#[test]
fn lowers_the_pinned_large_mdoc_fixture_without_empty_sections() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../libmandoc-rs/vendor/mandoc-1.14.6/mandoc.1");
    if !source.exists() {
        // The repository supplies this separately licensed cross-crate
        // fixture; the published mant-engine package is self-contained.
        return;
    }

    let document = parse_manual_source(&source).expect("lower vendored mandoc manual");

    assert!(document.sections.len() > 5);
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.title == "DESCRIPTION")
    );
    assert!(
        document
            .sections
            .iter()
            .all(|section| !section.blocks.is_empty() || !section.children.is_empty())
    );
}

#[test]
fn keeps_unexpanded_tabular_cells_visible_with_a_diagnostic() {
    let document = parse_manual_bytes(
        std::path::Path::new("unexpanded-table-cell.7"),
        b".TH UNEXPANDED-TABLE-CELL 7\n.SH DESCRIPTION\n.TS\nl l.\n1\t\\*[unknown-label]\n.TE\n",
    )
    .expect("lower unresolved formatter string in a table cell");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a structured table");
    };
    assert_eq!(rows[0].cells.len(), 2);
    let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
        panic!("expected one recovered table-cell paragraph");
    };
    assert_eq!(inline_text(children), r"\*[unknown-label]");
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.level == DiagnosticLevel::Unsupported
            && diagnostic.code.as_deref() == Some("manual.unexpanded-table-cell")
    }));
}

#[test]
fn preserves_the_boundary_that_enters_a_compact_mdoc_term() {
    let document = parse_manual_bytes(
        std::path::Path::new("spacing-transition.5"),
        b".Dd August 19, 2026\n.Dt SPACING-TRANSITION 5\n.Os\n.Sh KEYWORDS\n\
.Bl -tag -width Ds\n.It Xo\n.Cm @newuser\n.Sm off\n.Ar name : uid : gid\n.Sm on\n.Xc\nCreate a user.\n.El\n",
    )
    .expect("lower an mdoc spacing transition inside a term");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a keyword definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "@newuser name:uid:gid");
}

fn inline_text(children: &[Inline]) -> String {
    children
        .iter()
        .map(|child| match child {
            Inline::Text { value } | Inline::Code { value } => value.clone(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => inline_text(children),
            Inline::Anchor { .. } => String::new(),
            Inline::LineBreak => "\n".to_owned(),
        })
        .collect()
}

mod entries;
mod layout;
mod navigation;
mod tables;
