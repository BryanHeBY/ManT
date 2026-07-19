//! Lowers the owned libmandoc syntax tree into Mant's stable document model.

mod blocks;
mod diagnostics;
mod inline;
mod navigation;

use std::path::Path;

use mant_ast::{
    DocumentMeta, DocumentSchema, DocumentSource, Engine, MantDocument, Producer, SourceFormat,
    SourceSpan,
};
use mant_mandoc_sys::{MacroSet, Node, ParsedDocument};

pub use mant_mandoc_sys::ParseError;

/// Parse and normalize one located man or mdoc source file.
///
/// # Errors
///
/// Returns [`ParseError`] when the source cannot be opened or parsed.
pub fn parse_manual_source(path: &Path) -> Result<MantDocument, ParseError> {
    let parsed = mant_mandoc_sys::parse_file(path, true)?;
    Ok(lower_mandoc_document(path, &parsed))
}

/// Convert a completed low-level parse into the stable document contract.
#[must_use]
pub fn lower_mandoc_document(path: &Path, parsed: &ParsedDocument) -> MantDocument {
    let mut context = LoweringContext::new(parsed.metadata.name.as_deref());
    let mut diagnostics = diagnostics::parse_diagnostics(&parsed.diagnostics);
    let mut sections = blocks::lower_sections(&parsed.root, &mut context);
    let explicit_targets = navigation::explicit_targets(&parsed.root);
    navigation::resolve_navigation(&mut sections, &explicit_targets, &mut diagnostics);
    MantDocument {
        schema: DocumentSchema::V1,
        producer: Producer {
            name: "mant".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            engine: Some(Engine {
                name: "libmandoc".to_owned(),
                version: mant_mandoc_sys::MANDOC_VERSION.to_owned(),
            }),
        },
        source: DocumentSource {
            format: match parsed.macro_set {
                MacroSet::Mdoc => SourceFormat::Mdoc,
                MacroSet::Man | MacroSet::None => SourceFormat::Man,
            },
            path: Some(path.to_string_lossy().into_owned()),
            renderer: None,
        },
        meta: DocumentMeta {
            title: parsed.metadata.title.clone(),
            section: parsed.metadata.section.clone(),
            date: parsed.metadata.date.clone(),
            volume: parsed.metadata.volume.clone(),
            os: parsed.metadata.os.clone(),
            arch: parsed.metadata.arch.clone(),
            names: parsed.metadata.name.iter().cloned().collect(),
            alias_target: parsed.metadata.alias_target.clone(),
        },
        diagnostics,
        sections,
    }
}

struct LoweringContext<'a> {
    default_name: Option<&'a str>,
    next_section_id: usize,
}

impl<'a> LoweringContext<'a> {
    const fn new(default_name: Option<&'a str>) -> Self {
        Self {
            default_name,
            next_section_id: 1,
        }
    }

    fn section_id(&mut self, title: &str) -> String {
        let sequence = self.next_section_id;
        self.next_section_id += 1;
        let slug: String = title
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
            format!("section-{sequence}")
        } else {
            format!("{slug}-{sequence}")
        }
    }
}

fn source_span(node: &Node) -> Option<SourceSpan> {
    (node.line > 0).then_some(SourceSpan {
        line: node.line,
        column: node.column.max(1),
        end_line: None,
        end_column: None,
    })
}

fn part_children(node: &Node, kind: mant_mandoc_sys::NodeKind) -> &[Node] {
    node.children
        .iter()
        .find(|child| child.kind == kind)
        .map_or(&[], |child| child.children.as_slice())
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use mant_ast::{Block, DiagnosticLevel, Inline, SourceFormat};

    use super::parse_manual_source;

    fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
        fs::write(&path, source).expect("write temporary roff fixture");
        path
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
                |inline| matches!(inline, Inline::ManualReference { name, .. } if name == "man")
            )
        );
        assert!(children.iter().any(
            |inline| matches!(inline, Inline::ExternalLink { uri, .. } if uri == "https://example.test/docs")
        ));
        assert!(children.iter().any(
            |inline| matches!(inline, Inline::EmailLink { address, .. } if address == "docs@example.test")
        ));
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

        assert_eq!(document.sections[0].id, "description-1");
        assert_eq!(document.sections[1].id, "details-2");
        let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
            panic!("expected navigation paragraph");
        };
        assert!(children.iter().any(|inline| matches!(
            inline,
            Inline::SectionReference { target, children }
                if target == "details-2" && inline_text(children) == "DETAILS"
        )));
        assert!(children.iter().any(|inline| matches!(
            inline,
            Inline::Anchor { id } if id == "explicit-option"
        )));
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
        assert!(
            children
                .iter()
                .all(|inline| !matches!(inline, Inline::SectionReference { .. }))
        );
        assert!(document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        }));
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
                kind: mant_ast::ListKind::Ordered,
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
    fn lowers_the_pinned_large_mdoc_fixture_without_empty_sections() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/mandoc-1.14.6/mandoc.1");

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
             .SH EQUATION\n.EQ\nx sup 2\n.EN\n",
        );

        let document = parse_manual_source(&path).expect("lower table and equation");
        fs::remove_file(path).expect("remove temporary roff fixture");

        assert!(matches!(
            document.sections[0].blocks[0],
            Block::Table { ref rows, .. } if rows.len() == 1 && rows[0].cells.len() == 2
        ));
        assert!(matches!(
            document.sections[1].blocks[0],
            Block::Equation { ref value, .. } if value.contains('x')
        ));
    }

    fn inline_text(children: &[Inline]) -> String {
        children
            .iter()
            .map(|child| match child {
                Inline::Text { value } | Inline::Code { value } => value.clone(),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::ExternalLink { children, .. }
                | Inline::EmailLink { children, .. }
                | Inline::ManualReference { children, .. }
                | Inline::SectionReference { children, .. } => inline_text(children),
                Inline::Anchor { .. } => String::new(),
                Inline::LineBreak => "\n".to_owned(),
            })
            .collect()
    }
}
