//! Lowers the owned libmandoc syntax tree into Mant's stable document model.

mod blocks;
mod diagnostics;
mod inline;

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
        diagnostics: diagnostics::parse_diagnostics(&parsed.diagnostics),
        sections: blocks::lower_sections(&parsed.root, &mut context),
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
}
