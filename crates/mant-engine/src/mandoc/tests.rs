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

#[test]
fn native_generated_anchors_share_one_normalized_unique_namespace() {
    let document = parse_manual_bytes(
        std::path::Path::new("anchors.1"),
        b".TH ANCHORS 1\n.SH ALPHA\nProse.\n.SH OPTIONS\n.TP\n.B --ALPHA\nFirst.\n.TP\n.B --ALPHA\nSecond.\n",
    )
    .expect("lower repeated uppercase definition tags");
    let anchors = anchor_ids(&document);

    assert!(anchors.iter().any(|id| id == "alpha-2"));
    assert!(anchors.iter().any(|id| id == "alpha-3"));
    assert_eq!(anchors.len(), anchors.iter().collect::<HashSet<_>>().len());
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.duplicate-identity")
        )
    }));
}

#[test]
fn explicit_mdoc_targets_are_zero_width_and_unique() {
    let document = parse_manual_bytes(
        std::path::Path::new("target-only.1"),
        b".Dd September 3, 2026\n.Dt TARGET-ONLY 1\n.Os\n.Sh DESCRIPTION\n\
.Tg explicit-target\n\
ordinary text\n\
.Tg repeated-target\n\
more text\n\
.Tg repeated-target\n\
last text\n\
.Tg\n",
    )
    .expect("lower explicit target-only requests");

    let anchors = anchor_ids(&document);
    assert_eq!(
        anchors
            .iter()
            .filter(|target| target.as_str() == "explicit-target")
            .count(),
        1
    );
    assert_eq!(
        anchors
            .iter()
            .filter(|target| target.as_str() == "repeated-target")
            .count(),
        1
    );
    assert!(anchors.iter().all(|target| !target.is_empty()));
    let visible = visible_document_text(&document);
    assert!(!visible.contains("explicit-target"));
    assert!(!visible.contains("repeated-target"));
    assert!(visible.contains("ordinary text"));
    assert!(visible.contains("last text"));
}

#[test]
fn automatic_targets_use_source_tokens_independently_of_spacing_mode() {
    let document = parse_manual_bytes(
        std::path::Path::new("spacing-target.7"),
        b".Dd September 4, 2026\n.Dt SPACING-TARGET 7\n.Os\n.Sh DESCRIPTION\n\
.Sm off\n\
.Sy (U + S) / R .\n\
.Sm on\n",
    )
    .expect("lower an automatic target while mdoc spacing is disabled");

    assert!(anchor_ids(&document).iter().any(|id| id == "u"));
    assert!(!anchor_ids(&document).iter().any(|id| id == "u-s-r"));
}

#[test]
fn argumentless_targets_retain_libmandoc_derived_destinations() {
    let document = parse_manual_bytes(
        std::path::Path::new("derived-target.8"),
        b".Dd September 4, 2026\n.Dt DERIVED-TARGET 8\n.Os\n.Sh DESCRIPTION\n\
.Bl -tag -width Ds\n\
.It Xo\n\
.Ic route\n\
.Op Fl dtv\n\
.Op Fl T Ar rtable\n\
.Tg\n\
.Cm nameserver\n\
.Xc\n\
First description.\n\
.It Xo\n\
.Ic route\n\
.Tg\n\
.Cm sourceaddr\n\
.Xc\n\
Second description.\n\
.El\n",
    )
    .expect("lower argument-less targets in extended list heads");

    let index = mant_ir::DocumentIndex::build(&document);
    assert!(index.fragment_target("nameserver").is_some());
    assert!(index.fragment_target("sourceaddr").is_some());
    assert!(
        document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("ir.ambiguous-fragment-alias")
        })
    );
}

#[test]
fn argumentless_target_requests_bind_each_following_source_owner() {
    let document = parse_manual_bytes(
        std::path::Path::new("derived-target-owners.7"),
        b".Dd September 4, 2026\n.Dt DERIVED-TARGET-OWNERS 7\n.Os\n.Sh DESCRIPTION\n\
.Tg\n\
.Ic first-command\n\
.Pp\n\
Paragraph before the second request.\n\
.Tg\n\
.Ic second-command\n\
.Tg explicit-third\n\
.Ic third-command\n",
    )
    .expect("lower consecutive target ownership forms");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in ["first-command", "second-command", "explicit-third"] {
        assert!(
            index.fragment_target(target).is_some(),
            "missing target request {target}"
        );
    }
    assert_eq!(
        index.fragment_target("third-command"),
        None,
        "an explicit .Tg spelling must replace the automatic owner spelling"
    );
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
fn preserves_targets_moved_to_paragraphs_and_displays() {
    let paragraph = parse_manual_bytes(
        std::path::Path::new("paragraph-target.3"),
        b".Dd September 3, 2026\n.Dt PARAGRAPH-TARGET 3\n.Os\n.Sh DESCRIPTION\n\
intro\n.Pp\n.Fn alpha\n",
    )
    .expect("lower an automatic function target moved to Pp");
    assert!(anchor_ids(&paragraph).iter().any(|id| id == "alpha"));
    assert!(visible_document_text(&paragraph).contains("alpha"));

    for (name, display) in [
        ("Bd", ".Bd -literal\nhello\n.Ed"),
        ("D1", ".D1 hello"),
        ("Dl", ".Dl hello"),
    ] {
        let source = format!(
            ".Dd September 3, 2026\n.Dt DISPLAY-TARGET 1\n.Os\n.Sh DESCRIPTION\n.Tg display-target\n{display}\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("{name}-target.1")),
            source.as_bytes(),
        )
        .unwrap_or_else(|error| panic!("lower {name} target: {error}"));
        assert_eq!(anchor_ids(&document), ["display-target"]);
        assert_eq!(visible_document_text(&document).trim(), "hello");
        assert!(matches!(
            document.sections[0].blocks.first(),
            Some(Block::Preformatted { children, .. })
                if matches!(children.first(), Some(Inline::Anchor { id, .. }) if id == "display-target")
        ));
    }
}

#[test]
fn preserves_targets_moved_to_list_items_and_containers() {
    let item = parse_manual_bytes(
        std::path::Path::new("list-item-target.1"),
        b".Dd September 3, 2026\n.Dt LIST-ITEM-TARGET 1\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.It\n.Tg bullet-target\n.Em bullet text\n.El\n",
    )
    .expect("lower a target moved to an ordinary list item");
    assert_eq!(anchor_ids(&item), ["bullet-target"]);
    assert!(visible_document_text(&item).contains("bullet text"));

    let container = parse_manual_bytes(
        std::path::Path::new("list-container-target.1"),
        b".Dd September 3, 2026\n.Dt LIST-CONTAINER-TARGET 1\n.Os\n.Sh DESCRIPTION\n\
.Tg list-target\n.Bl -bullet\n.It\nhello\n.El\n",
    )
    .expect("lower a target moved to a list container");
    assert_eq!(anchor_ids(&container), ["list-target"]);
    assert_eq!(visible_document_text(&container).trim(), "hello");
}

#[test]
fn preserves_explicit_targets_on_empty_mdoc_list_items() {
    let document = parse_manual_bytes(
        std::path::Path::new("empty-list-targets.7"),
        b".Dd September 4, 2026\n.Dt EMPTY-LIST-TARGETS 7\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.Tg empty-bullet-target\n.It\n.El\n\
.Bl -enum\n.Tg empty-enum-target\n.It\n.El\n\
.Bl -item\n.Tg empty-plain-target\n.It\n.El\n\
.Bl -column \"one\" \"two\"\n.Tg empty-column-target\n.It\n.El\n\
.Bl -tag\n.Tg empty-definition-target\n.It\n.El\n\
.Bl -bullet\n.It\nbody\n.Tg trailing-list-target\n.El\n\
.Bl -bullet\n.Tg target-without-item\n.El\n",
    )
    .expect("lower targets at empty and trailing mdoc list positions");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in [
        "empty-bullet-target",
        "empty-enum-target",
        "empty-plain-target",
        "empty-column-target",
        "empty-definition-target",
        "trailing-list-target",
        "target-without-item",
    ] {
        assert_eq!(
            index.fragment_target(target).map(mant_ir::NodeId::as_str),
            Some(target),
            "missing {target}"
        );
    }
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.duplicate-identity" | "ir.ambiguous-fragment-alias")
        )
    }));
    let visible = visible_document_text(&document);
    assert_eq!(visible.trim(), "body");
    assert!(!visible.contains("target"));
}

#[test]
fn explicit_section_targets_preserve_fragments_beside_normalized_ids() {
    let document = parse_manual_bytes(
        std::path::Path::new("section-targets.1"),
        b".Dd September 3, 2026\n.Dt SECTION-TARGETS 1\n.Os\n\
.Tg custom-section\n.Sh HEADING\ntext\n\
.Tg custom-subsection\n.Ss SUBHEADING\nmore text\n",
    )
    .expect("lower explicit section and subsection targets");

    assert_eq!(document.sections[0].id.as_str(), "heading");
    assert_eq!(
        document.sections[0]
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["custom-section"]
    );
    assert_eq!(document.sections[0].title, "HEADING");
    assert_eq!(document.sections[0].children[0].id.as_str(), "subheading");
    assert_eq!(
        document.sections[0].children[0]
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["custom-subsection"]
    );
    assert_eq!(document.sections[0].children[0].title, "SUBHEADING");
    assert!(anchor_ids(&document).is_empty());
}

#[test]
fn noncanonical_mdoc_targets_keep_exact_fragments_without_invalid_ids() {
    let document = parse_manual_bytes(
        std::path::Path::new("authored-fragments.7"),
        b".Dd September 4, 2026\n.Dt AUTHORED-FRAGMENTS 7\n.Os\n\
.Tg Mixed.Section\n.Sh HEADING\n\
.Tg --option\noption target\n",
    )
    .expect("lower exact authored fragments");

    let index = mant_ir::DocumentIndex::build(&document);
    assert_eq!(
        index
            .fragment_target("Mixed.Section")
            .map(mant_ir::NodeId::as_str),
        Some("heading")
    );
    assert_eq!(
        index
            .fragment_target("--option")
            .map(mant_ir::NodeId::as_str),
        Some("option")
    );
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.invalid-fragment-alias")
        )
    }));
}

#[test]
fn root_blocks_receive_the_same_navigation_passes_as_sections() {
    let mdoc = parse_manual_bytes(
        std::path::Path::new("root-section-reference.1"),
        b".Dd September 3, 2026\n.Dt ROOT-SECTION-REFERENCE 1\n.Os\n\
See\n.Sx NAME\n.Sh NAME\n.Nm root-section-reference\n.Nd root reference\n",
    )
    .expect("lower a root-level mdoc section reference");
    assert!(mdoc.blocks.iter().any(|block| {
        matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| {
            matches!(inline, Inline::Link {
                target: mant_ir::LinkTarget::Section { id }, ..
            } if id == "name")
        }))
    }));
    assert!(mdoc.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_deref() != Some("ir.dangling-section-link")
            && diagnostic.code.as_deref() != Some("unresolved-section-reference")
    }));

    let man = parse_manual_bytes(
        std::path::Path::new("root-manual-reference.1"),
        b".TH ROOT-MANUAL-REFERENCE 1\n.BR printf (3)\n.SH NAME\nroot-manual-reference \\- root reference\n",
    )
    .expect("lower a root-level traditional manual reference");
    assert!(man.blocks.iter().any(|block| {
        matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| {
            matches!(inline, Inline::Link {
                target: mant_ir::LinkTarget::Manual {
                    name,
                    manual_section: Some(section),
                }, ..
            } if name == "printf" && section == "3")
        }))
    }));
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
fn lowers_man_sections_fonts_definitions_and_literal_blocks() {
    let path = temporary_source(
        "man",
        ".TH MANT 1 \"July 2026\"\n\
         .SH NAME\n\
         mant \\- a viewer\n\
         .SH OPTIONS\n\
         .TP\n\
         \\fB\\-h\\fR\n\
         Show help.\n\
         .nf\n\
         mant --help\n\
         mant git\n\
         .fi\n",
    );

    let document = parse_manual_source(&path).expect("lower man source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>(),
        vec!["NAME", "OPTIONS"]
    );
    assert!(
        document.sections[1]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::DefinitionList { .. }))
    );
    assert!(document.sections[1].blocks.iter().any(|block| matches!(
        block,
        Block::DefinitionList { items, .. }
            if items.iter().any(|item| item.description.iter().any(
                |description| matches!(description, Block::Preformatted { .. })
            ))
    )));
}

#[test]
fn composite_environment_options_do_not_promote_shell_labels() {
    let path = temporary_source(
        "environment-options",
        ".TH DEMO 1\n\
         .SH \"ENVIRONMENT OPTIONS\"\n\
         .TP\n\
         Unix Bourne shell:\n\
         UNZIP=-qq; export UNZIP\n\
         .TP\n\
         \\-q\n\
         Be quiet.\n",
    );

    let document = parse_manual_source(&path).expect("lower environment option fixture");
    fs::remove_file(path).expect("remove fixture");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("definitions");
    };
    assert_eq!(
        items[0].identity.as_ref().expect("term").role,
        mant_ir::DefinitionRole::Term
    );
    assert_eq!(items[1].identity.as_ref().expect("option").names, ["-q"]);
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")
            && diagnostic.message.contains("Unix Bourne shell:")
    }));
}

#[test]
fn separates_definition_layout_arguments_from_visible_terms() {
    let path = temporary_source(
        "definition-head-roles",
        ".TH HEAD-ROLES 1\n\
         .SH EXAMPLES\n\
         .TP \\w'man\\ 'u\n\
         .BI man \\ ls\n\
         Display ls.\n\
         .TP 4\n\
         4\n\
         A numeric term remains visible.\n\
         .IP \"1\" 8n\n\
         An IP width remains layout-only.\n",
    );

    let document = parse_manual_source(&path).expect("lower definition head roles");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(
        items
            .iter()
            .flat_map(|item| item.terms.iter())
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["man ls", "4", "1"]
    );
    assert!(matches!(
        items[0].terms[0].as_slice(),
        [
            Inline::Anchor { id, .. },
            Inline::Anchor { .. },
            Inline::Strong { .. },
            Inline::Emphasis { .. }
        ]
            if items[0].identity.as_ref().is_some_and(|identity| &identity.id == id)
    ));
    assert!(
        items
            .iter()
            .flat_map(|item| item.terms.iter())
            .all(|term| !inline_text(term).contains("96u"))
    );
}

#[test]
fn preserves_consecutive_tp_aliases_ending_in_line_continuations() {
    let path = temporary_source(
        "continued-definition-aliases",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .BI \"\\-symbols=\" \"file\"\\c\n\
         .TP\n\
         .BI \"\\-s \" \"file\"\\c\n\
         \\&\n\
         Read symbols.\n",
    );

    let document = parse_manual_source(&path).expect("lower consecutive TP aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-symbols=file", "-s file"]
    );
    let Block::Paragraph { children, .. } = &items[0].description[0] else {
        panic!("expected alias description paragraph");
    };
    assert_eq!(inline_text(children), "Read symbols.");
}

#[test]
fn keeps_unrelated_consecutive_tp_definitions_separate() {
    let path = temporary_source(
        "distinct-consecutive-definitions",
        ".TH DISTINCT 1\n\
         .SH OPTIONS\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         Description only for b.\n",
    );

    let document = parse_manual_source(&path).expect("lower distinct tagged paragraphs");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "-a");
    assert!(items[0].description.is_empty());
    assert_eq!(inline_text(&items[1].terms[0]), "-b");
    let Block::Paragraph { children, .. } = &items[1].description[0] else {
        panic!("expected second tagged paragraph description");
    };
    assert_eq!(inline_text(children), "Description only for b.");
}

#[test]
fn paragraph_distance_zero_does_not_turn_tp_items_into_aliases() {
    let path = temporary_source(
        "distinct-zero-distance-definitions",
        ".TH DISTINCT 1\n\
         .SH OPTIONS\n\
         .PD 0\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         Description only for b.\n",
    );

    let document = parse_manual_source(&path).expect("lower compact tagged paragraphs");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "-a");
    assert!(items[0].description.is_empty());
    assert_eq!(inline_text(&items[1].terms[0]), "-b");
    let Block::Paragraph { children, .. } = &items[1].description[0] else {
        panic!("expected second tagged paragraph description");
    };
    assert_eq!(inline_text(children), "Description only for b.");
}

#[test]
fn restored_paragraph_distance_closes_a_compact_tp_alias_group() {
    let path = temporary_source(
        "compact-alias-group",
        ".TH ALIASES 1\n\
         .SH COMMANDS\n\
         .TP\n\
         bind first-form\n\
         .PD 0\n\
         .TP\n\
         bind second-form\n\
         .PD\n\
         Shared description.\n",
    );

    let document = parse_manual_source(&path).expect("lower compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "bind first-form");
    assert_eq!(inline_text(&items[0].terms[1]), "bind second-form");
    let Block::Paragraph { children, .. } = &items[0].description[0] else {
        panic!("expected shared description");
    };
    assert_eq!(inline_text(children), "Shared description.");
}

#[test]
fn paragraph_distance_in_the_first_tp_head_opens_a_compact_alias_group() {
    let path = temporary_source(
        "head-owned-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .PD 0\n\
         --first\n\
         .TP\n\
         --second\n\
         .PD\n\
         Shared description.\n",
    );

    let document = parse_manual_source(&path).expect("lower head-owned compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "--first");
    assert_eq!(inline_text(&items[0].terms[1]), "--second");
    let Block::Paragraph { children, .. } = &items[0].description[0] else {
        panic!("expected shared description");
    };
    assert_eq!(inline_text(children), "Shared description.");
}

#[test]
fn compact_alias_group_does_not_absorb_a_preceding_orphan() {
    let path = temporary_source(
        "bounded-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         orphan\n\
         .TP\n\
         .PD 0\n\
         --first\n\
         .TP\n\
         --second\n\
         .PD\n\
         Shared description.\n",
    );

    let document = parse_manual_source(&path).expect("lower bounded compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "orphan");
    assert!(items[0].description.is_empty());
    assert_eq!(items[1].terms.len(), 2);
    assert_eq!(inline_text(&items[1].terms[0]), "--first");
    assert_eq!(inline_text(&items[1].terms[1]), "--second");
}

#[test]
fn adjacent_compact_alias_groups_keep_their_exact_boundaries() {
    let path = temporary_source(
        "adjacent-compact-alias-groups",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .PD 0\n\
         --first\n\
         .TP\n\
         --second\n\
         .PD\n\
         First description.\n\
         .TP\n\
         .PD 0\n\
         --third\n\
         .TP\n\
         --fourth\n\
         .PD\n\
         Second description.\n",
    );

    let document = parse_manual_source(&path).expect("lower adjacent compact alias groups");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "--first");
    assert_eq!(inline_text(&items[0].terms[1]), "--second");
    assert_eq!(inline_text(&items[1].terms[0]), "--third");
    assert_eq!(inline_text(&items[1].terms[1]), "--fourth");
}

#[test]
fn unclosed_compact_run_stays_separate_and_resets_at_indent_scope() {
    let path = temporary_source(
        "unclosed-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .PD 0\n\
         --loose\n\
         .TP\n\
         --described\n\
         Own description.\n\
         .TP\n\
         .PD 0\n\
         outer\n\
         .RS\n\
         .TP\n\
         inner\n\
         .PD\n\
         Inner description.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower unclosed compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one outer definition list");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(inline_text(&items[0].terms[0]), "--loose");
    assert!(items[0].description.is_empty());
    assert_eq!(inline_text(&items[1].terms[0]), "--described");
    assert_eq!(inline_text(&items[2].terms[0]), "outer");
    let [
        Block::DefinitionList {
            items: inner_items, ..
        },
    ] = items[2].description.as_slice()
    else {
        panic!("expected one nested definition list");
    };
    assert_eq!(inner_items.len(), 1);
    assert_eq!(inline_text(&inner_items[0].terms[0]), "inner");
    assert!(!inner_items[0].description.is_empty());
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
fn preserves_man_synopsis_flow_and_alternating_fonts() {
    let path = temporary_source(
        "man-synopsis-flow",
        ".TH MAN 1\n\
         .SH SYNOPSIS\n\
         .B man\n\
         .RI [\\| \"man options\" \\|]\n\
         .RI [\\|[\\| section \\|]\n\
         .IR page \\ \\|.\\|.\\|.\\|]\\ \\.\\|.\\|.\\&\n\
         .br\n\
         .B man\n\
         .B \\-k\n\
         .RI [\\| \"apropos options\" \\|]\n\
         .I regexp\n\
         \\&.\\|.\\|.\\&\n\
         .br\n\
         .B man\n\
         .BR \\-w \\||\\| \\-W\n\
         .RI [\\| \"man options\" \\|]\n\
         .I page\n\
         \\&.\\|.\\|.\\&\n",
    );

    let document = parse_manual_source(&path).expect("lower man synopsis");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one synopsis paragraph");
    };
    assert_eq!(
        inline_text(children),
        "man [man options] [[section] page ...] ...\n\
         man -k [apropos options] regexp ...\n\
         man -w|-W [man options] page ..."
    );
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak))
            .count(),
        2
    );
    assert!(children.iter().any(
        |node| matches!(node, Inline::Emphasis { children } if inline_text(children) == "man options")
    ));
    assert!(
        children.iter().any(
            |node| matches!(node, Inline::Strong { children } if inline_text(children) == "-w")
        )
    );
    assert!(
        children.iter().any(
            |node| matches!(node, Inline::Strong { children } if inline_text(children) == "-W")
        )
    );
}

#[test]
fn preserves_man_sy_heads_with_body_content_and_inline_fonts() {
    let document = parse_manual_bytes(
        std::path::Path::new("sy-heads.1"),
        b".TH SY-HEADS 1 \"August 17, 2026\"\n\
.SH SYNOPSIS\n\
.SY getent\n\
.RI [ option ]\n\
.I database\n\
.YS\n\
.SH DESCRIPTION\n\
.SY #!\\f[I]interpreter\\f[]\n\
.RI [ optional-arg ]\n\
.YS\n",
    )
    .expect("lower SY heads");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one synopsis paragraph");
    };
    assert_eq!(inline_text(children), "getent [option] database");
    assert!(matches!(
        children.first(),
        Some(Inline::Strong { children }) if inline_text(children) == "getent"
    ));

    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("expected one description paragraph");
    };
    assert_eq!(inline_text(children), "#!interpreter [optional-arg]");
    assert!(matches!(
        children.first(),
        Some(Inline::Strong { children })
            if children.iter().any(|inline| matches!(
                inline,
                Inline::Emphasis { children } if inline_text(children) == "interpreter"
            ))
    ));
    assert!(
        document.diagnostics.is_empty(),
        "{:?}",
        document.diagnostics
    );
}

#[test]
fn keeps_man_synopsis_lines_together_inside_no_fill_examples() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-synopsis.2"),
        b".TH NO-FILL-SYNOPSIS 2\n\
.SH DESCRIPTION\n\
.EX\n\
.SY #!\\f[I]interpreter\\f[]\n\
.RI [ optional-arg ]\n\
.YS\n\
.EE\n",
    )
    .expect("lower synopsis inside example");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill synopsis must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "#!interpreter\n[optional-arg]");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
}

#[test]
fn preserves_explicit_blank_rows_inside_no_fill_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-blank-row.7"),
        b".TH NO-FILL-BLANK-ROW 7\n\
.SH EXAMPLE\n\
.EX\n\
first line\n\
\n\
second line\n\
.EE\n",
    )
    .expect("lower no-fill blank row");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
}

#[test]
fn preserves_targets_moved_to_paragraph_breaks_inside_no_fill_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-paragraph-target.8"),
        b".Dd September 4, 2026\n.Dt NO-FILL-PARAGRAPH-TARGET 8\n.Os\n\
.Sh EXAMPLES\n\
.Bd -unfilled\n\
.Li first line\n\
.Pp\n\
.Li prompt Sy hp(0,0)\n\
.Ed\n",
    )
    .expect("lower a target moved onto a no-fill paragraph break");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Anchor { id, .. } if id == "hp-0-0"))
    );
    assert!(inline_text(children).contains("prompt hp(0,0)"));
}

#[test]
fn preserves_zero_width_guard_rows_inside_no_fill_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-zero-width-row.7"),
        b".TH NO-FILL-ZERO-WIDTH-ROW 7\n\
.SH EXAMPLE\n\
.EX\n\
first line\n\
\\&\n\
second line\n\
.EE\n",
    )
    .expect("lower no-fill zero-width row");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
}

#[test]
fn preserves_lines_inside_font_blocks_nested_in_literal_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("literal-font-block.7"),
        b".Dd August 20, 2026\n\
.Dt LITERAL-FONT-BLOCK 7\n\
.Os\n\
.Sh EXAMPLE\n\
.Bd -literal\n\
.Bf Sy\n\
first line\n\
second line\n\
.Ef\n\
.Ed\n",
    )
    .expect("lower font block inside literal display");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "literal display must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
}

#[test]
fn preserves_literal_display_lines_inside_literal_font_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("literal-display-inside-font-block.7"),
        b".Dd August 21, 2026\n\
.Dt LITERAL-DISPLAY-INSIDE-FONT-BLOCK 7\n\
.Os\n\
.Sh EXAMPLE\n\
.Bf Li\n\
.Bd -literal\n\
first line\n\
second line\n\
.Ed\n\
.Ef\n",
    )
    .expect("lower literal display inside literal font block");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "fonted literal display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
}

#[test]
fn preserves_lines_inside_nested_literal_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("nested-literal-display.7"),
        b".Dd August 21, 2026\n\
.Dt NESTED-LITERAL-DISPLAY 7\n\
.Os\n\
.Sh EXAMPLE\n\
.Bd -literal\n\
first line\n\
.Bd -literal\n\
second line\n\
third line\n\
.Ed\n",
    )
    .expect("lower nested literal display");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "nested literal displays must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\nsecond line\nthird line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
}

#[test]
fn collapses_a_no_fill_blank_line_run_to_one_visual_separator() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-blank-run.7"),
        b".TH NO-FILL-BLANK-RUN 7\n\
.SH EXAMPLE\n\
.EX\n\
first line\n\
\n\
\n\
second line\n\
.EE\n",
    )
    .expect("lower no-fill blank run");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
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
fn distinguishes_filled_source_wrapping_from_indented_output_lines() {
    let path = temporary_source(
        "filled-line-boundaries",
        concat!(
            ".TH TOOL 1\n",
            ".SH SYNOPSIS\n",
            "tool [first]\n",
            "    [second]\n",
            "    [third]\n",
            ".PP\n",
            "Ordinary source wrapping\n",
            "remains one filled paragraph.\n",
        ),
    );

    let document = parse_manual_source(&path).expect("lower filled line boundaries");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [
        Block::Paragraph {
            children: synopsis, ..
        },
        Block::Paragraph {
            children: prose, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected synopsis and prose paragraphs");
    };
    assert_eq!(
        inline_text(synopsis),
        "tool [first]\n    [second]\n    [third]"
    );
    assert_eq!(
        synopsis
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
    assert_eq!(
        inline_text(prose),
        "Ordinary source wrapping remains one filled paragraph."
    );
}

#[test]
fn honours_roff_no_space_line_continuations() {
    let document = parse_manual_bytes(
        std::path::Path::new("line-continuation.1"),
        b".TH LINE-CONTINUATION 1\n\
.SH DESCRIPTION\n\
extsize=\\c\n\
nnnn; multi-\\c\n\
block; (\\c\n\
.BR read (2)\n\
.EX\n\
literal-\\c\n\
continuation\n\
.EE\n",
    )
    .expect("lower no-space line continuations");

    let [
        Block::Paragraph {
            children: prose, ..
        },
        Block::Preformatted {
            children: literal, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected one filled and one no-fill block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(prose), "extsize=nnnn; multi-block; (read(2)");
    assert_eq!(inline_text(literal), "literal-continuation");
}

#[test]
fn keeps_explicit_horizontal_separation_at_a_tight_line_join() {
    let document = parse_manual_bytes(
        std::path::Path::new("motion-continuation.1"),
        b".TH MOTION-CONTINUATION 1\n\
.SH DESCRIPTION\n\
\\h'-04' 1.\\h'+01'\\c\n\
The next line.\n",
    )
    .expect("lower a horizontally spaced continued line");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:?}", document.sections[0].blocks);
    };
    assert_eq!(inline_text(children), " 1. The next line.");
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
fn lowers_indented_aliases_without_roff_layout_arguments() {
    let path = temporary_source(
        "indented-aliases",
        ".TH CONTROL 1\n\
         .SH OPTIONS\n\
         .PD 0\n\
         .IP \"\\fB-a\\fR\" 4\n\
         .IP \"\\fB--all\\fR\" 4\n\
         Show all entries.\n\
         .PD\n\
         .in 168u\n",
    );

    let document = parse_manual_source(&path).expect("lower indented aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-a", "--all"]
    );
    assert_eq!(items[0].description.len(), 1);
    let Block::Paragraph { children, .. } = &items[0].description[0] else {
        panic!("expected alias description paragraph");
    };
    assert_eq!(inline_text(children), "Show all entries.");
}

#[test]
fn headless_ip_macros_continue_the_preceding_definition() {
    let path = temporary_source(
        "headless-ip-continuations",
        ".TH CONTINUATIONS 1\n\
         .SH DESCRIPTION\n\
         .IP foo\n\
         First paragraph.\n\
         .IP\n\
         Second paragraph.\n\
         .IP\n\
         Third paragraph.\n\
         .IP bar\n\
         Bar body.\n",
    );

    let document = parse_manual_source(&path).expect("lower headless IP continuations");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "foo");
    assert_eq!(items[0].description.len(), 3);
    assert_eq!(
        items[0]
            .description
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["First paragraph.", "Second paragraph.", "Third paragraph."]
    );
    assert_eq!(inline_text(&items[1].terms[0]), "bar");
}

#[test]
fn tq_terms_share_one_semantic_option_identity() {
    let path = temporary_source(
        "tq-aliases",
        ".TH TQ-ALIASES 7\n\
         .SH OPTIONS\n\
         .TP\n\
         .B \\-\\-alpha\n\
         .TQ\n\
         .B \\-a\n\
         .TQ\n\
         .B \\-\\-ALPHA\n\
         Enable alpha mode.\n",
    );

    let document = parse_manual_source(&path).expect("lower TQ aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["--alpha", "-a", "--ALPHA"]
    );
    assert_eq!(
        items[0].identity.as_ref().expect("option identity").names,
        ["--alpha", "-a", "--ALPHA"]
    );
}

#[test]
fn ip_does_not_absorb_unproven_definition_heads() {
    let path = temporary_source(
        "bounded-ip-aliases",
        ".TH IP-BOUNDARY 7\n\
         .SH OPTIONS\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         .TP\n\
         -c\n\
         .IP -d\n\
         Description only for d.\n",
    );

    let document = parse_manual_source(&path).expect("lower bounded IP definitions");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["-a", "-b", "-c", "-d"]
    );
    assert!(items[..3].iter().all(|item| item.description.is_empty()));
    assert!(!items[3].description.is_empty());
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.definition-alias-boundary")
    }));
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
fn preserves_man_paragraph_distance_between_indented_paragraphs() {
    let path = temporary_source(
        "paragraph-distance",
        ".TH SPACING 1\n\
         .SH OPTIONS\n\
         .IP \"\\fB-a\\fR\" 4\n\
         First.\n\
         .IP \"\\fB-b\\fR\" 4\n\
         Second.\n\
         .PD 0\n\
         .IP \"\\fB-c\\fR\" 4\n\
         Third.\n\
         .IP \"\\fB-d\\fR\" 4\n\
         Fourth.\n\
         .PD\n\
         .IP \"\\fB-e\\fR\" 4\n\
         Fifth.\n",
    );

    let document = parse_manual_source(&path).expect("lower paragraph distance");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, compact, .. }] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected one definition list");
    };
    assert!(!compact);
    assert_eq!(items.len(), 5);
    assert_eq!(
        items
            .iter()
            .map(|item| item.spacing_before_lines)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(0), Some(0), Some(1)]
    );
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
fn does_not_duplicate_explicit_space_before_a_transparent_indent() {
    let path = temporary_source(
        "explicit-space-before-indent",
        ".TH SPACING 1\n\
         .SH CONTENT\n\
         Before.\n\
         .sp\n\
         .RS 4\n\
         After.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower explicit indented spacing");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [
        Block::Paragraph { .. },
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph { layout, .. },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected prose, one explicit gap, and indented prose");
    };
    assert_eq!(layout.indent_columns, 4);
    assert_eq!(
        layout.spacing_before_lines, 0,
        "the explicit gap must not be repeated as wrapper boundary spacing",
    );
}

#[test]
fn relative_indent_does_not_invent_paragraph_distance() {
    let path = temporary_source(
        "relative-indent-spacing",
        ".TH SPACING 7\n\
         .SH DESCRIPTION\n\
         .PP\n\
         first term\n\
         .RS 4\n\
         First description.\n\
         .RE\n\
         .PP\n\
         second term\n\
         .RS 4\n\
         Second description.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower relative-indent spacing");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [
        Block::Paragraph {
            layout: first_term, ..
        },
        Block::Paragraph {
            layout: first_description,
            ..
        },
        Block::Paragraph {
            layout: second_term,
            ..
        },
        Block::Paragraph {
            layout: second_description,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected two terms followed by their indented descriptions");
    };
    assert_eq!(
        (first_term.indent_columns, first_term.spacing_before_lines),
        (0, 0)
    );
    assert_eq!(
        (
            first_description.indent_columns,
            first_description.spacing_before_lines,
        ),
        (4, 0),
        "RS changes indentation without adding paragraph distance",
    );
    assert_eq!(
        (second_term.indent_columns, second_term.spacing_before_lines),
        (0, 1),
        "the following PP still owns the distance between entries",
    );
    assert_eq!(
        (
            second_description.indent_columns,
            second_description.spacing_before_lines,
        ),
        (4, 0),
    );
}

#[test]
fn relative_indent_preserves_child_owned_paragraph_distance() {
    let path = temporary_source(
        "relative-indent-child-spacing",
        ".TH SPACING 7\n\
         .SH DESCRIPTION\n\
         Before.\n\
         .RS 4\n\
         .PP\n\
         Explicit nested paragraph.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower nested paragraph spacing");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::Paragraph { .. }, Block::Paragraph { layout, .. }] =
        document.sections[0].blocks.as_slice()
    else {
        panic!("expected outer prose and one explicitly separated nested paragraph");
    };
    assert_eq!(layout.indent_columns, 4);
    assert_eq!(
        layout.spacing_before_lines, 1,
        "PP inside RS must retain its own paragraph distance",
    );
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
fn lowers_mdoc_semantic_inline_nodes_and_nested_sections() {
    let path = temporary_source(
        "mdoc",
        ".Dd July 19, 2026\n\
         .Dt MANT 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         Use\n\
         .Nm mant\n\
         with\n\
         .Xr man 1\n\
         Read\n\
         .Lk https://example.test/docs \"the documentation\"\n\
         or contact\n\
         .Mt docs@example.test\n\
         .Ss Details\n\
         .Fl h\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.source.format, SourceFormat::Mdoc);
    assert_eq!(document.sections[0].children[0].title, "Details");
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected description paragraph");
    };
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(
        children.iter().any(
            |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Manual { name, .. }, .. } if name == "man")
        )
    );
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.test/docs")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, .. } if address == "docs@example.test")
    ));
}

#[test]
fn retains_unlabelled_mdoc_link_targets_before_trailing_punctuation() {
    let document = parse_manual_bytes(
        std::path::Path::new("external-link.9"),
        b".Dd August 19, 2026\n.Dt EXTERNAL-LINK 9\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/books .\n",
    )
    .expect("lower an unlabelled mdoc external link");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one external-link paragraph");
    };
    assert_eq!(inline_text(children), "https://example.test/books.");
    assert!(matches!(
        children.as_slice(),
        [
            Inline::Link {
                target: mant_ir::LinkTarget::External { uri },
                children: link_children,
                ..
            },
            Inline::Text { value },
        ] if uri == "https://example.test/books"
            && inline_text(link_children) == "https://example.test/books"
            && value == "."
    ));
}

#[test]
fn expands_mdoc_bsd_lifecycle_and_release_forms() {
    let source = b".Dd August 19, 2026\n.Dt BSD-LIFECYCLE 7\n.Os\n.Sh DESCRIPTION\n.Bx\n.Bx -alpha\n.Bx -beta\n.Bx -devel .\n.Bx 4.3 .\n.Bx 4.3 Net/2 .\n.Bx 386 0.1 .\n";
    let document = parse_manual_bytes(std::path::Path::new("bsd-lifecycle.7"), source)
        .expect("lower mdoc BSD lifecycle forms");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one BSD lifecycle paragraph");
    };
    assert_eq!(
        inline_text(children),
        "BSD BSD (currently in alpha test) BSD (currently in beta test) BSD (currently under development). 4.3BSD. 4.3BSD Net/2. 386BSD 0.1."
    );
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
fn propagates_nested_no_space_and_preserves_prefix_content() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-space.7"),
        b".Dd August 19, 2026\n.Dt NO-SPACE 7\n.Os\n.Sh DESCRIPTION\n\
.Em Bell Labs Ns -derived\n\
.Ar job Ns s :\n\
.Sm off\n\
.Pf [\\-]ddd Cm \\&. No ddd\n\
.Sm on\n",
    )
    .expect("lower nested no-space macros");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one no-space paragraph");
    };
    assert_eq!(inline_text(children), "Bell Labs-derived jobs: [-]ddd.ddd");
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
.Fa const char *path\n\
.Fa int flags\n\
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
        "<angle> [bracket] “double” (parenthesized) “quoted” ‘single’ {braced} \
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
fn preserves_mdoc_command_names_in_each_synopsis_form() {
    let document = parse_manual_bytes(
        std::path::Path::new("fido2-cred.1"),
        b".Dd August 19, 2026\n.Dt FIDO2-CRED 1\n.Os\n.Sh NAME\n.Nm fido2-cred\n.Nd make a credential\n.Sh SYNOPSIS\n.Nm\n.Fl M\n.Op Fl i Ar input_file\n.Nm fido2-cred\n.Fl V\n.Nm helper\n.Op Fl q\n",
    )
    .expect("lower mdoc synopsis names");
    let synopsis = &document.sections[1];
    let rendered = synopsis
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        [
            "fido2-cred -M [-i input_file]",
            "fido2-cred -V",
            "helper [-q]",
        ]
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
fn preserves_printable_roff_content_outside_formal_sections() {
    let document = parse_manual_bytes(
        std::path::Path::new("manweb.1"),
        b".TH MANWEB 1\n .SH NAME\nmanweb - browse generated documentation\n.SH SYNOPSIS\n.B manweb\n",
    )
    .expect("lower root prose");
    let [Block::Paragraph { children, .. }] = document.blocks.as_slice() else {
        panic!("expected one root paragraph, got {:?}", document.blocks);
    };

    assert_eq!(
        inline_text(children),
        " .SH NAME manweb - browse generated documentation"
    );
    assert_eq!(document.sections[0].title, "SYNOPSIS");
}

#[test]
fn discards_temporary_indent_arguments_without_hiding_the_next_line() {
    let document = parse_manual_bytes(
        std::path::Path::new("temporary-indent.8"),
        b".TH TEMPORARY-INDENT 8\n.SH EXAMPLES\n.ti +8n\nexample% command\n.ti\nexample% other\n",
    )
    .expect("lower temporary indentation requests");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one examples paragraph");
    };
    assert_eq!(inline_text(children), "example% command example% other");
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
fn recognizes_legacy_sphinx_manual_links_in_roff_inputs() {
    let path = temporary_source(
        "sphinx-manual-links",
        ".TH BTRFS 8\n\
         .SH COMMANDS\n\
         See btrfs\\-subvolume(8) \\%<> and btrfs(5) \\%<> for details.\n\
         .EX\n\
         btrfs-subvolume(8) \\%<>\n\
         .EE\n",
    );

    let document = parse_manual_source(&path).expect("lower legacy Sphinx references");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let section = &document.sections[0];
    let paragraph = section
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(children),
            _ => None,
        })
        .expect("commands paragraph");
    assert_eq!(
        inline_text(paragraph),
        "See btrfs-subvolume(8) and btrfs(5) for details."
    );
    let references = paragraph
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link {
                target:
                    mant_ir::LinkTarget::Manual {
                        name,
                        manual_section: Some(manual_section),
                    },
                ..
            } => Some((name.as_str(), manual_section.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(references, [("btrfs-subvolume", "8"), ("btrfs", "5")]);

    let literal = section
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Preformatted { children, .. } => Some(children),
            _ => None,
        })
        .expect("literal display");
    assert_eq!(inline_text(literal), "btrfs-subvolume(8) <>");
    assert!(!literal.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { .. },
            ..
        }
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
fn searches_across_man_link_labels_and_visible_targets() {
    let source = b".TH LINK-SEARCH 1\n\
.SH REPORTING BUGS\n\
Mail comments, suggestions and bug reports to\n\
.MT docs@example.test\n\
Sean\n\
.ME .\n";

    for pattern in ["bug reports to Sean", "docs@example.test"] {
        let query = crate::query_roff_bytes(source).expect("query link fixture");
        let result = crate::project_query_view(
            query,
            &mant_protocol::QueryView::Search {
                pattern: pattern.to_owned(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .expect("search link fixture");
        let crate::QueryViewResult::Search(search) = result else {
            panic!("expected search result");
        };
        assert_eq!(search.total, 1, "pattern={pattern:?}");
    }
}

#[test]
fn resolves_mdoc_section_references_and_explicit_targets() {
    let path = temporary_source(
        "mdoc-navigation",
        ".Dd July 19, 2026\n\
         .Dt NAVIGATION 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         Continue with\n\
         .Sx DETAILS\n\
         .Tg explicit-option\n\
         .Fl x\n\
         .Sh DETAILS\n\
         Target content.\n",
    );

    let document = parse_manual_source(&path).expect("lower navigation mdoc source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.sections[0].id, "description");
    assert_eq!(document.sections[1].id, "details");
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected navigation paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            children,
            ..
        } if id == "details" && inline_text(children) == "DETAILS"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Anchor { id, .. } if id == "explicit-option"
    )));
}

#[test]
fn explicit_targets_reserve_the_native_section_namespace() {
    let path = temporary_source(
        "mdoc-target-section-collision",
        ".Dd August 30, 2026\n\
         .Dt TARGET-COLLISION 1\n\
         .Os\n\
         .Sh FOO\n\
         .Tg bar\n\
         First.\n\
         .Sh BAR\n\
         Second.\n",
    );

    let document = parse_manual_source(&path).expect("lower reserved explicit target");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.sections[0].id, "foo");
    assert_eq!(document.sections[1].id, "bar-2");
    assert!(document.sections[0].blocks.iter().any(|block| matches!(
        block,
        Block::Paragraph { children, .. }
            if children.iter().any(|inline| matches!(
                inline,
                Inline::Anchor { id, .. } if id == "bar"
            ))
    )));
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code.as_deref() != Some("ir.identity-role-collision") })
    );
}

#[test]
fn automatic_function_target_survives_a_section_identity_collision() {
    let path = temporary_source(
        "mdoc-function-section-collision",
        ".Dd September 4, 2026\n\
         .Dt TARGET-COLLISION 3\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         .Ss acl_delete_def_file_at\n\
         .Fn acl_delete_def_file_at \"const char *path\"\n",
    );

    let document = parse_manual_source(&path).expect("lower automatic function target");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(
        document.sections[0].children[0].id,
        "acl-delete-def-file-at"
    );
    assert!(
        anchor_ids(&document)
            .iter()
            .any(|id| id == "acl-delete-def-file-at-2")
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
fn lowers_normalized_ordered_lists_and_literal_displays() {
    let path = temporary_source(
        "normalized",
        ".Dd July 19, 2026\n.Dt NORMALIZED 1\n.Os\n.Sh CONTENT\n\
         .Bl -enum -compact\n.It\nfirst\n.It\nsecond\n.El\n\
         .Bd -literal -offset 6n\nline one\nline two\n.Ed\n",
    );

    let document = parse_manual_source(&path).expect("lower normalized mdoc");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::List {
            kind: mant_ir::ListKind::Ordered,
            compact: true,
            ..
        }
    ));
    assert!(matches!(
        document.sections[0].blocks[1],
        Block::Preformatted { layout, .. } if layout.indent_columns == 6
    ));
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
            .all(|entry| entry.aliases.iter().all(|alias| alias != "2."))
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
fn only_tag_definition_lists_recover_ordered_procedures() {
    for style in ["tag", "diag", "hang", "inset", "ohang"] {
        let source = format!(
            ".Dd September 4, 2026\n.Dt MDOC-{style} 4\n.Os\n.Sh EXAMPLES\n\
.Bl -{style} -offset 4n -width \"1.\"\n\
.It 1.\nFirst step.\n\
.It 2.\nSecond step.\n\
.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("mdoc-definition-style.4"),
            source.as_bytes(),
        )
        .expect("lower mdoc definition style");
        if style == "tag" {
            let Block::List {
                kind: ListKind::Ordered,
                items,
                layout,
                ..
            } = &document.sections[0].blocks[0]
            else {
                panic!("-{style} must recover one ordered list")
            };
            assert_eq!(layout.indent_columns, 0);
            assert!(items.iter().all(|item| {
                matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 4)
            }));
        } else {
            assert!(
                matches!(document.sections[0].blocks[0], Block::DefinitionList { .. }),
                "-{style} must retain definition semantics"
            );
        }
    }

    let native = parse_manual_bytes(
        std::path::Path::new("mdoc-native-enum.4"),
        b".Dd September 4, 2026\n.Dt MDOC-ENUM 4\n.Os\n.Sh EXAMPLES\n\
.Bl -enum -offset 4n\n\
.It\nFirst step.\n\
.It\nSecond step.\n\
.El\n",
    )
    .expect("lower native mdoc enum");
    let Block::List { items, .. } = &native.sections[0].blocks[0] else {
        panic!("native enum must remain an ordered list")
    };
    assert!(items.iter().all(|item| {
        matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 4)
    }));
}

#[test]
fn distinguishes_man_ip_enumeration_from_numeric_option_values() {
    let document = parse_manual_bytes(
        std::path::Path::new("ip-enumeration.1"),
        b".TH IP-ENUMERATION 1\n.SH OPTIONS\n\
.TP\n.B -fchanges\nThis flag makes these changes:\n.RS 4\n\
.IP 1. 4\nfirst change\n.IP 2. 4\nsecond change\n.IP 3. 4\nthird change\n.RE\n\
.TP\n.B -fcounter\nThese are generated steps:\n.RS 4\n\
.nr step 0 1\n.IP \\n+[step]\nfirst step\n.IP \\n+[step]\nsecond step\n.RE\n\
.TP\n.B -flevel=level\nThe level can be one of:\n.RS 4\n\
.IP 0 4\ndisabled\n.IP 1 4\nenabled\n.RE\n",
    )
    .expect("lower man IP lists and values");

    let options = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(options.len(), 3);
    for option in &options[..2] {
        assert!(option.description.iter().any(|block| {
            let Block::List {
                kind: ListKind::Ordered,
                start: Some(1),
                items,
                ..
            } = block
            else {
                return false;
            };
            items.len() >= 2
                && items.iter().all(|item| {
                    matches!(
                        item.blocks.first(),
                        Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 0
                    )
                })
        }));
    }
    assert!(
        options[2]
            .description
            .iter()
            .any(|block| matches!(block, Block::DefinitionList { items, .. } if items.len() == 2))
    );

    let index = SemanticIndex::build(&document);
    let entries = index.section("options");
    assert_eq!(entries.len(), 3);
    assert!(
        entries[..2]
            .iter()
            .all(|entry| entry.children.is_empty() && entry.value_domain.is_none())
    );
    assert_eq!(
        entries[2].value_domain,
        Some(ValueDomain::Choices { exhaustive: false })
    );
    assert_eq!(
        entries[2]
            .children
            .iter()
            .flat_map(|entry| &entry.aliases)
            .collect::<Vec<_>>(),
        ["0", "1"]
    );
}

#[test]
fn keeps_relative_indent_references_inside_man_ip_enumerations() {
    let document = parse_manual_bytes(
        std::path::Path::new("ip-reference-enumeration.1"),
        b".TH IP-REFERENCE-ENUMERATION 1\n.SH NOTES\n\
.IP \" 1.\" 4\nFirst reference\n.RS 4\nfile:///first\n.RE\n\
.IP \" 2.\" 4\nSecond reference\n.RS 4\nfile:///second\n.RE\n\
.IP \" 3.\" 4\nThird reference\n.RS 4\nfile:///third\n.RE\n",
    )
    .expect("lower numbered IP references");

    let notes = &document.sections[0];
    let [
        Block::List {
            kind: ListKind::Ordered,
            start: Some(1),
            items,
            ..
        },
    ] = notes.blocks.as_slice()
    else {
        panic!("numbered references must form one ordered list");
    };
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|item| {
        item.blocks.len() == 2
            && item.blocks.iter().all(
                |block| matches!(block, Block::Paragraph { layout, .. } if layout.indent_columns == 0),
            )
    }));
    assert!(SemanticIndex::build(&document).section("notes").is_empty());
}

#[test]
fn recognizes_one_source_proven_ip_ordinal_without_semantic_entry() {
    let document = parse_manual_bytes(
        std::path::Path::new("ip-reference-candidate.1"),
        b".TH IP-REFERENCE-CANDIDATE 1\n.SH NOTES\n\
.IP \" 9.\" 4\nOnly reference\n.RS 4\nfile:///only\n.RE\n",
    )
    .expect("lower one numbered IP reference");

    let Block::List {
        kind: ListKind::Ordered,
        start: Some(9),
        compact: false,
        items,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("one source-proven ordinal is an ordered list");
    };
    assert!(matches!(
        items[0].blocks.as_slice(),
        [
            Block::Paragraph { layout: body, .. },
            Block::Paragraph {
                layout: continuation,
                ..
            }
        ] if body.indent_columns == 0 && continuation.indent_columns == 0
    ));
    assert!(SemanticIndex::build(&document).section("notes").is_empty());
}

#[test]
fn recognizes_tp_enumerations_nested_below_a_definition() {
    let document = parse_manual_bytes(
        std::path::Path::new("tp-enumeration.1"),
        b".TH TP-ENUMERATION 1\n.SH OPTIONS\n\
.TP\n.B extdebug\nThis setting has the following effects:\n.RS 4\n\
.TP\n.B 1.\nFirst effect.\n\
.TP\n.B 2.\nSecond effect.\n\
.TP\n.B 3.\nThird effect.\n.RE\n",
    )
    .expect("lower nested TP enumeration");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("outer setting remains a definition");
    };
    assert!(items[0].description.iter().any(|block| {
        matches!(
            block,
            Block::List {
                kind: ListKind::Ordered,
                start: Some(1),
                items,
                ..
            } if items.len() == 3
        )
    }));
    let index = SemanticIndex::build(&document);
    let entries = index.section("options");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].children.is_empty());
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
fn mdoc_definition_layout_uses_the_normalized_list_width() {
    let path = temporary_source(
        "mdoc-definition-widths",
        ".Dd July 23, 2026\n.Dt WIDTHS 1\n.Os\n.Sh ITEMS\n\
         .Bl -tag -width 20n\n.It tenletters\nwide description\n.El\n\
         .Bl -tag -width 3n\n.It short\nnarrow description\n.El\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc definition widths");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let lists = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2);
    assert!(lists[0][0].inline_term);
    assert!(!lists[1][0].inline_term);
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
fn lowers_tbl_and_eqn_payloads_into_structured_blocks() {
    let path = temporary_source(
        "table-equation",
        ".TH PAYLOAD 1\n.SH TABLE\n.TS\ntab(|);\nl r.\nleft|right\n.TE\n\
         .SH EQUATION\n.EQ\nx + {width over 2}\n.EN\n",
    );

    let document = parse_manual_source(&path).expect("lower table and equation");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::Table { ref rows, .. } if rows.len() == 1 && rows[0].cells.len() == 2
    ));
    assert!(matches!(
        document.sections[1].blocks[0],
        Block::Equation { ref value, .. } if value == "x + width / 2"
    ));
}

#[test]
fn large_tbl_rows_scale_without_changing_their_topology() {
    const ROW_COUNT: usize = 2_048;
    let mut source = String::from(".TH TABLE-SCALE 7\n.SH TABLE\n.TS\nl l.\n");
    for index in 0..ROW_COUNT {
        writeln!(source, "left {index}\tright {index}").expect("append table row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(std::path::Path::new("table-scale.7"), source.as_bytes())
        .expect("lower large table");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("large tbl input must remain one table");
    };
    assert_eq!(rows.len(), ROW_COUNT);
    assert!(matches!(
        rows.first().and_then(|row| row.cells.first()),
        Some(mant_ir::TableCell { blocks, .. })
            if matches!(blocks.as_slice(), [Block::Paragraph { children, .. }]
                if inline_text(children) == "left 0")
    ));
    assert!(matches!(
        rows.last().and_then(|row| row.cells.get(1)),
        Some(mant_ir::TableCell { blocks, .. })
            if matches!(blocks.as_slice(), [Block::Paragraph { children, .. }]
                if inline_text(children) == format!("right {}", ROW_COUNT - 1))
    ));
}

#[test]
fn keeps_inline_equations_in_macro_arguments_and_filled_prose() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-equation.7"),
        b".TH EQNPROBE2 7\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.TP\n.BR Dp\\~ \"$dx sub 1 ~ ldots ~ dx sub n$\"\nDraw a polygon with,\nfor $i = 1 , ldots , n + 1$,\nits vertex.\n",
    )
    .expect("lower inline equations");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "expected one definition list: {:?}",
            document.sections[0].blocks
        );
    };
    let [item] = items.as_slice() else {
        panic!("expected one equation definition");
    };
    assert_eq!(inline_text(&item.terms[0]), "Dp dx _ 1 ... dx _ n");
    let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
        panic!("expected one filled description: {:?}", item.description);
    };
    assert_eq!(
        inline_text(children),
        "Draw a polygon with, for i = 1 , ... , n + 1, its vertex."
    );
    assert!(
        children
            .iter()
            .any(|child| matches!(child, Inline::Code { value } if value == "i = 1 , ... , n + 1"))
    );
}

#[test]
fn normalizes_inline_equations_retained_as_tbl_cell_text() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation.3"),
        b".TH TABLE-EQN 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl l.\n%0%\tfor values in % [ 0 , ~pi over 2 ]%\n.TE\n",
    )
    .expect("lower table equations");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    let [left, right] = rows[0].cells.as_slice() else {
        panic!("expected two cells");
    };
    let [Block::Paragraph { children: left, .. }] = left.blocks.as_slice() else {
        panic!("expected left paragraph");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = right.blocks.as_slice()
    else {
        panic!("expected right paragraph");
    };
    assert!(matches!(left.as_slice(), [Inline::Code { value }] if value == "0"));
    assert_eq!(inline_text(right), "for values in [ 0 , π / 2 ]");
    assert!(
        right
            .iter()
            .any(|child| matches!(child, Inline::Code { .. }))
    );
}

#[test]
fn bounds_distinct_tbl_equation_normalization_work() {
    let mut source =
        String::from(".TH TABLE-EQN-BUDGET 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl.\n");
    for index in 0..=MAX_INLINE_EQUATION_NORMALIZATIONS {
        writeln!(source, "%x{index}%").expect("write fixture row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation-budget.3"),
        source.as_bytes(),
    )
    .expect("lower a bounded number of table equations");

    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
        })
    );
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    assert_eq!(rows.len(), MAX_INLINE_EQUATION_NORMALIZATIONS + 1);
}

#[test]
fn preserves_tbl_rows_across_interleaved_comments_and_text_blocks() {
    let source = b".TH COMMENTED-TABLE 1\n.SH TABLE\n.TS\nl l.\na\t1\n.\\\" disabled text block T{\n.\\\" ignored\n.\\\" T}\nb\t2\nc\t3\nT{\n.BR d (1)\nT}\t4\ne\t5\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("commented-table.1"), source)
        .expect("lower commented table");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(rows.len(), 5);
    let first_cells = rows
        .iter()
        .map(|row| match row.cells[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            cells => panic!("expected one paragraph per table cell: {cells:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(first_cells, ["a", "b", "c", "d(1)", "e"]);
}

#[test]
fn keeps_multiline_cells_aligned_after_an_empty_text_block() {
    let source = b".TH EMPTY-TABLE-CELL 7\n.SH TABLE\n.TS\ntab(@);\nl l l.\n\
T{\nT}@T{\nCore\nT}@T{\nProduction-grade, first-class\nT}\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("empty-table-cell.7"), source)
        .expect("lower a row beginning with an empty text block");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one table");
    };
    let [row] = rows.as_slice() else {
        panic!("expected one table row");
    };
    let values = row
        .cells
        .iter()
        .map(|cell| match cell.blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            [] => String::new(),
            blocks => panic!("unexpected table cell blocks: {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(values, ["", "Core", "Production-grade, first-class"]);
}

#[test]
fn keeps_tbl_vertical_span_markers_out_of_visible_cells() {
    let document = parse_manual_bytes(
        std::path::Path::new("vertical-table-span.1"),
        b".TH VERTICAL-TABLE-SPAN 1\n.SH ATTRIBUTES\n.TS\nl l l.\nInterface\tAttribute\tValue\nT{\n.BR demo (1)\nT}\tThread safety\tMT-Safe\n\\^\tAsync-signal safety\tAS-Unsafe\n\\^\tAsync-cancel safety\tAC-Unsafe\n.TE\n",
    )
    .expect("lower vertical table span");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1].cells[0].row_span, 3);
    assert!(rows[2].cells[0].blocks.is_empty());
    assert!(rows[3].cells[0].blocks.is_empty());
}

#[test]
fn preserves_tbl_rows_nested_in_unfilled_mdoc_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("unfilled-table.7"),
        b".Dd August 19, 2026\n.Dt UNFILLED-TABLE 7\n.Os\n.Sh DESCRIPTION\n\
.Bd -unfilled -offset indent\n.TS\ntab(@);\nl l.\nleft@right\nnext@value\n.TE\n.Ed\n",
    )
    .expect("lower table nested in an unfilled display");

    let table = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("nested table must remain structured");
    assert_eq!(table.len(), 2);
    assert_eq!(table[0].cells.len(), 2);
    assert!(
        document.sections[0].blocks.iter().all(
            |block| !matches!(block, Block::Preformatted { children, .. } if children.is_empty())
        ),
        "the surrounding display must not leave an empty placeholder"
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
fn restores_mdoc_names_inside_tbl_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-block.3"),
        b".Dd August 19, 2026\n.Dt TABLE-TEXT-BLOCK 3\n.Os\n\
.Sh NAME\n.Nm table-text-block\n.Nd test tbl text blocks\n\
.Sh ATTRIBUTES\n.TS\nallbox;\nl l.\nInterface\tValue\n\
T{\n.Nm\nT}\tMT-Safe\n.TE\n",
    )
    .expect("lower tbl text blocks");

    let Block::Table { rows, .. } = &document.sections[1].blocks[0] else {
        panic!("expected attributes table");
    };
    let [Block::Paragraph { children, .. }] = rows[1].cells[0].blocks.as_slice() else {
        panic!("expected recovered name cell");
    };
    assert_eq!(inline_text(children), "table-text-block");
    assert!(matches!(children.as_slice(), [Inline::Strong { .. }]));
}

#[test]
fn keeps_semantic_links_inside_tbl_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-link.1"),
        b".TH TABLE-TEXT-LINK 1\n\
.nr do-fallback 0\n\
.if !\\n(.f .nr do-fallback 1\n\
.if \\n[do-fallback] \\{\\\n\
.  de MR\n\
.    ie \\\\n(.$=1 \\\n\
.      I \\%\\\\$1\n\
.    el \\\n\
.      IR \\%\\\\$1 (\\\\$2)\\\\$3\n\
.  .\n\
.\\}\n\
.rr do-fallback\n\
.SH DESCRIPTION\n\
.TS\ntab($);\nl l.\ngrn$T{\nrenders\n.MR gremlin 1\ndiagrams;\nT}\n\
gperl$T{\npopulates\n.I groff\nregisters using\n.MR perl 1 ;\nT}\n.TE\n",
    )
    .expect("lower semantic tbl text block");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("semantic table content must not escape into a separate paragraph");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
        panic!("expected semantic table cell paragraph");
    };
    assert_eq!(inline_text(children), "renders gremlin(1) diagrams;");
    assert!(children.iter().any(|child| matches!(
        child,
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { name, manual_section },
            ..
        } if name == "gremlin" && manual_section.as_deref() == Some("1")
    )));
    let [Block::Paragraph { children, .. }] = rows[1].cells[1].blocks.as_slice() else {
        panic!("expected styled semantic table cell paragraph");
    };
    assert_eq!(
        inline_text(children),
        "populates groff registers using perl(1);"
    );
    assert!(
        children
            .iter()
            .any(|child| matches!(child, Inline::Emphasis { .. }))
    );
}

#[test]
fn restores_alternating_font_arguments_inside_tbl_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-alternation.7"),
        b".TH TABLE-TEXT-ALTERNATION 7\n.SH DESCRIPTION\n.TS\nl l.\nT{\n\
.BI \\[aq] s1 \\[aq] s2 \\[aq]\nT}\tT{\n\
.I s1\nproduces the same formatted output as\n.IR s2 .\nT}\n.TE\n",
    )
    .expect("lower alternating man macros inside a tbl text block");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a structured table");
    };
    let [left, right] = rows[0].cells.as_slice() else {
        panic!("expected both reconstructed table cells");
    };
    let [Block::Paragraph { children: left, .. }] = left.blocks.as_slice() else {
        panic!("expected a reconstructed left table-cell paragraph");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = right.blocks.as_slice()
    else {
        panic!("expected a reconstructed right table-cell paragraph");
    };
    assert_eq!(inline_text(left), "'s1's2'");
    assert_eq!(
        inline_text(right),
        "s1 produces the same formatted output as s2."
    );
    assert!(
        right
            .iter()
            .any(|inline| matches!(inline, Inline::Emphasis { .. }))
    );
}

#[test]
fn restores_nested_mdoc_requests_inside_tbl_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-mdoc-requests.8"),
        b".Dd August 19, 2026\n.Dt TABLE-MDOC-REQUESTS 8\n.Os\n.Sh DESCRIPTION\n\
.TS\ntab(@);\nl l.\nT{\n.Cm sip Ar addr Ns Op / Ns Ar mask\nT}@T{\n\
bitwise and of the address with\n.Ar mask\nequals\n.Ar addr .\n.Ar addr\n\
can be an IPv4 or IPv6 address.\nT}\n.TE\n",
    )
    .expect("lower nested mdoc requests in table text blocks");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a structured table");
    };
    let [left, right] = rows[0].cells.as_slice() else {
        panic!("expected two reconstructed table cells");
    };
    let [Block::Paragraph { children: left, .. }] = left.blocks.as_slice() else {
        panic!("expected reconstructed selector cell");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = right.blocks.as_slice()
    else {
        panic!("expected reconstructed description cell");
    };
    assert_eq!(inline_text(left), "sip addr[/mask]");
    assert_eq!(
        inline_text(right),
        "bitwise and of the address with mask equals addr. addr can be an IPv4 or IPv6 address."
    );
    assert!(
        left.iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(
        right
            .iter()
            .any(|inline| matches!(inline, Inline::Emphasis { .. }))
    );
}

#[test]
fn keeps_command_names_in_extended_mdoc_synopsis_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("extended-synopsis.8"),
        b".Dd August 19, 2026\n.Dt EXTENDED-SYNOPSIS 8\n.Os\n.Sh NAME\n\
.Nm zinject\n.Nd inject faults\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n\
.It Xo\n.Nm zinject\n.Xc\nList injections.\n\
.It Xo\n.Nm zinject\n.Fl b Ar bookmark\n.Xc\nInject a bookmark.\n.El\n",
    )
    .expect("lower extended mdoc synopsis terms");

    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("expected synopsis definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "zinject");
    assert_eq!(inline_text(&items[1].terms[0]), "zinject -b bookmark");
    assert!(matches!(
        items[0].terms[0].as_slice(),
        [Inline::Anchor { id, .. }, Inline::Strong { .. }]
            if items[0].identity.as_ref().is_some_and(|identity| &identity.id == id)
    ));
    assert!(
        items[1].terms[0]
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
}

#[test]
fn decodes_named_characters_inside_equations() {
    let document = parse_manual_bytes(
        std::path::Path::new("equation-characters.1"),
        b".TH EQUATION-CHARACTERS 1\n.SH EQUATION\n.EQ\n\\[*p] \\[mi] x\n.EN\n",
    )
    .expect("lower equation characters");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::Equation { ref value, .. } if value == "\u{03c0} \u{2212} x"
    ));
}

#[test]
fn lowers_every_mdoc_column_list_cell() {
    let document = parse_manual_bytes(
        std::path::Path::new("columns.3"),
        b".Dd August 19, 2026\n.Dt COLUMNS 3\n.Os\n.Sh DESCRIPTION\n\
.Bl -column name type description\n.It Dv CLSET_TIMEOUT Ta \"struct timeval *\" Ta \"set total timeout\"\n.El\n",
    )
    .expect("lower mdoc column list");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected column list to lower as a table");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 3);
    let rendered = rows[0]
        .cells
        .iter()
        .map(|cell| match cell.blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            blocks => panic!("expected one paragraph per cell, got {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered,
        ["CLSET_TIMEOUT", "struct timeval *", "set total timeout"]
    );
}

#[test]
fn preserves_nested_mdoc_spacing_state_in_definition_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("nested-spacing.1"),
        b".Dd August 19, 2026\n.Dt NESTED-SPACING 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n.It Fl L Xo\n.Sm off\n.Ar local_socket : host : hostport\n.Sm on\n.Xc\nForward a socket.\n.El\n",
    )
    .expect("lower nested mdoc spacing controls");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(
        inline_text(&items[0].terms[0]),
        "-L local_socket:host:hostport"
    );
}

#[test]
fn groups_mdoc_option_forms_that_share_one_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("shared-option-forms.1"),
        b".Dd August 27, 2026\n.Dt SHARED-OPTION-FORMS 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl L Xo\n.Sm off\n.Oo Ar bind_address : Oc\n.Ar port : host : hostport\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Oo Ar bind_address : Oc\n.Ar port : remote_socket\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Ar local_socket : host : hostport\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Ar local_socket : remote_socket\n.Sm on\n.Xc\n\
Forward a local socket.\n.El\n",
    )
    .expect("lower option forms with a shared description");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        [
            "-L [bind_address:]port:host:hostport",
            "-L [bind_address:]port:remote_socket",
            "-L local_socket:host:hostport",
            "-L local_socket:remote_socket",
        ]
    );
    assert_eq!(items[0].identity.as_ref().unwrap().names, ["-L"]);
    assert!(document.sections[0].blocks.iter().any(|block| {
        matches!(block, Block::DefinitionList { items, .. }
        if items[0].description.iter().any(|description| {
            matches!(description, Block::Paragraph { children, .. }
                if inline_text(children).contains("Forward a local socket"))
        }))
    }));
}

#[test]
fn groups_distinct_mdoc_options_that_share_one_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("shared-option-description.1"),
        b".Dd August 29, 2026\n.Dt SHARED-OPTION-DESCRIPTION 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl I Ar encoding\n\
.It Fl O Ar encoding\n\
Convert filenames from the specified encoding.\n\
.El\n",
    )
    .expect("lower distinct options with a shared description");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-I encoding", "-O encoding"]
    );
    assert_eq!(items[0].identity.as_ref().unwrap().names, ["-I", "-O"]);
    assert!(items[0].description.iter().any(|description| {
        matches!(description, Block::Paragraph { children, .. }
            if inline_text(children) == "Convert filenames from the specified encoding.")
    }));
}

#[test]
fn preserves_a_single_mdoc_option_argument_and_its_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("option-with-argument.1"),
        b".Dd August 29, 2026\n.Dt OPTION-WITH-ARGUMENT 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl Z Ar mode\n\
Select the archive mode without losing this description.\n\
.El\n",
    )
    .expect("lower an mdoc option with one argument");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(inline_text(&items[0].terms[0]), "-Z mode");
    assert_eq!(items[0].identity.as_ref().unwrap().names, ["-Z"]);
    assert!(items[0].description.iter().any(|description| {
        matches!(description, Block::Paragraph { children, .. }
            if inline_text(children)
                == "Select the archive mode without losing this description.")
    }));
}

#[test]
fn carries_mdoc_spacing_state_into_display_lines() {
    let document = parse_manual_bytes(
        std::path::Path::new("display-spacing.8"),
        b".Dd August 24, 2026\n.Dt DISPLAY-SPACING 8\n.Os\n.Sh FORMAT\n\
.Sm off\n.D1 Ar name : uid : gid\n.Sm on\n",
    )
    .expect("lower display-scoped mdoc spacing controls");

    let Block::Preformatted { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one display line");
    };
    assert_eq!(inline_text(children), "name:uid:gid");
}

#[test]
fn carries_mdoc_spacing_state_across_list_item_boundaries() {
    let document = parse_manual_bytes(
        std::path::Path::new("list-spacing.8"),
        b".Dd August 19, 2026\n.Dt LIST-SPACING 8\n.Os\n.Sh COMMANDS\n\
.Bl -tag -width Ds\n.Sm off\n.It Ic O Ar device\n.Sm on\n.It Ic done\nFinished.\n.El\n",
    )
    .expect("lower list-scoped mdoc spacing controls");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a command definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "Odevice");
    assert_eq!(inline_text(&items[1].terms[0]), "done");
}

#[test]
fn carries_mdoc_spacing_state_out_of_nested_synopsis_enclosures() {
    let document = parse_manual_bytes(
        std::path::Path::new("nested-synopsis-spacing.8"),
        b".Dd August 19, 2026\n.Dt NESTED-SYNOPSIS-SPACING 8\n.Os\n.Sh SYNOPSIS\n\
.Nm demo\n.Sm off\n.Oo Fl m\\~\n.Ar memory\n.Sm on\n.Oc\n\
.Op Fl o Ar variable Ns Cm = Ns Ar value\n.Ar name\n",
    )
    .expect("lower nested synopsis spacing transitions");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected synopsis paragraph");
    };
    assert_eq!(
        inline_text(children),
        "demo [-m memory] [-o variable=value] name"
    );
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

#[test]
fn separates_alternative_terms_in_an_extended_mdoc_definition_head() {
    let document = parse_manual_bytes(
        std::path::Path::new("extended-term-alternatives.8"),
        b".Dd August 19, 2026\n.Dt EXTENDED-TERM-ALTERNATIVES 8\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n.It Xo\n.Sm off\n.Ar ipaddr\n.Op / Ar masklen\n.Pp\n\
.Ar ipaddr\n.Op / Ar prefixlen\n.Sm on\n.Xc\nAccept this peer.\n.El\n",
    )
    .expect("lower alternative extended definition terms");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "ipaddr[/masklen]");
    assert_eq!(inline_text(&items[0].terms[1]), "ipaddr[/prefixlen]");
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
