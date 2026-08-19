//! Lowers the owned libmandoc syntax tree into `ManT`'s stable document model.

mod blocks;
mod diagnostics;
mod error;
pub(crate) mod inline;
mod layout;
mod navigation;
mod reference;
mod roff_escape;
mod source;

use std::{cell::RefCell, path::Path};

use libmandoc_rs::{
    Compression, Document as MandocDocument, IncludePolicy, MacroSet, Node, ParseOptions,
    ParseReport, Parser,
};
use mant_ir::{
    Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, ParserInfo, SourceFormat,
    SourceSpan, validate_document,
};

use self::{
    roff_escape::visible_text,
    source::{load_manual_source, redirect_target, resolve_manual_redirects},
};
use crate::ManualPage;
use crate::text_safety::mask_terminal_control_bytes;

pub use error::{ManualError, ManualErrorKind};
pub use source::MAX_MANUAL_BYTES;

/// Parse and normalize one standalone man or mdoc source file.
///
/// This safe convenience entry point does not expand `.so` redirects because
/// no caller-approved manual hierarchy accompanies a bare path. `ManT`'s indexed
/// query path uses [`parse_manual_page`] instead.
///
/// # Errors
///
/// Returns [`ManualError`] when the source cannot be opened, decoded, or parsed.
pub fn parse_manual_source(path: &Path) -> Result<Document, ManualError> {
    let loaded = load_manual_source(path)?;
    reject_standalone_redirect(path, &loaded.source)?;
    parse_plain_manual(path, &loaded.source, None)
}

/// Parse one already bounded, uncompressed standalone roff input.
///
/// This is the standard-input counterpart of [`parse_manual_source`]. It does
/// not expand `.so` redirects and never reads another file.
///
/// # Errors
///
/// Returns [`ManualError`] when libmandoc rejects the input.
pub fn parse_manual_bytes(path: &Path, source: &[u8]) -> Result<Document, ManualError> {
    reject_standalone_redirect(path, source)?;
    parse_plain_manual(path, source, None)
}

fn reject_standalone_redirect(path: &Path, source: &[u8]) -> Result<(), ManualError> {
    if redirect_target(path, source)?.is_some() {
        return Err(ManualError::redirect(
            path,
            "standalone .so redirects require MANPATH discovery and cannot be followed by --input",
        ));
    }
    Ok(())
}

/// Parse an indexed manual, resolving `.so` redirects against its discovered
/// manual hierarchy without falling back to the process working directory.
///
/// # Errors
///
/// Returns [`ManualError`] when the source cannot be opened, decoded, or parsed.
pub fn parse_manual_page(page: &ManualPage) -> Result<Document, ManualError> {
    let resolved = resolve_manual_redirects(page)?;
    parse_plain_manual(
        &page.path,
        &resolved.source,
        resolved.alias_target.as_deref(),
    )
}

fn parse_plain_manual(
    path: &Path,
    source: &[u8],
    alias_target: Option<&str>,
) -> Result<Document, ManualError> {
    let (source, masked_controls) = mask_terminal_control_bytes(source);
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes(path, source.as_ref())
    .map_err(ManualError::from)?;
    let source_text = String::from_utf8_lossy(source.as_ref());
    let mut document = lower_mandoc_document_with_source(path, &report, Some(&source_text));
    if masked_controls > 0 {
        document.diagnostics.insert(
            0,
            Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("manual.control-characters".to_owned()),
                message: format!("masked {masked_controls} terminal-unsafe control character(s)"),
                source: None,
            },
        );
    }
    if let Some(alias_target) = alias_target {
        document.meta.alias_target = Some(alias_target.to_owned());
    }
    Ok(document)
}

/// Convert a completed low-level parse into the stable document contract.
#[must_use]
pub fn lower_mandoc_document(path: &Path, report: &ParseReport) -> Document {
    lower_mandoc_document_with_source(path, report, None)
}

fn lower_mandoc_document_with_source(
    path: &Path,
    report: &ParseReport,
    source: Option<&str>,
) -> Document {
    let parsed: &MandocDocument = &report.document;
    let mut context = LoweringContext::new(parsed.metadata.name.as_deref(), source);
    let mut diagnostics = diagnostics::lower_diagnostics(&report.diagnostics);
    let mut sections = blocks::lower_sections(&parsed.root, &mut context);
    let mut root_blocks = blocks::lower_root_blocks(&parsed.root, &context);
    diagnostics.extend(context.take_diagnostics());
    let explicit_targets = navigation::explicit_targets(&parsed.root);
    let mut retained_targets = explicit_targets.clone();
    retained_targets.extend(crate::definitions::identify_definitions(
        &mut root_blocks,
        &mut sections,
        &explicit_targets,
    ));
    navigation::resolve_navigation(&mut sections, &retained_targets, &mut diagnostics);
    let mut document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc".to_owned(),
            version: libmandoc_rs::LIBMANDOC_VERSION.to_owned(),
        }),
        source: DocumentSource {
            format: match parsed.macro_set {
                MacroSet::Mdoc => SourceFormat::Mdoc,
                MacroSet::Man | MacroSet::None => SourceFormat::Man,
            },
            path: Some(path.to_string_lossy().into_owned()),
        },
        meta: DocumentMeta {
            title: normalize_metadata(parsed.metadata.title.as_deref()),
            manual_section: normalize_metadata(parsed.metadata.section.as_deref()),
            date: normalize_metadata(parsed.metadata.date.as_deref()),
            volume: normalize_metadata(parsed.metadata.volume.as_deref()),
            os: normalize_metadata(parsed.metadata.os.as_deref()),
            arch: normalize_metadata(parsed.metadata.arch.as_deref()),
            names: normalize_metadata(parsed.metadata.name.as_deref())
                .into_iter()
                .collect(),
            alias_target: parsed.metadata.alias_target.clone(),
        },
        diagnostics,
        blocks: root_blocks,
        sections,
    };
    document.diagnostics.extend(validate_document(&document));
    document
}

/// Metadata strings come from roff macro arguments rather than visible text
/// nodes, so libmandoc can legitimately retain zero-width escapes such as
/// `\&`. Normalize them through the same inline decoder used for document
/// content before exposing the renderer-neutral contract.
fn normalize_metadata(value: Option<&str>) -> Option<String> {
    value.map(visible_text)
}

struct LoweringContext<'a> {
    default_name: Option<&'a str>,
    source: Option<&'a str>,
    next_section_id: usize,
    diagnostics: RefCell<Vec<Diagnostic>>,
}

#[derive(Debug)]
struct TableTextBlock {
    source: String,
    start_line: u32,
    end_line: u32,
}

impl TableTextBlock {
    const fn contains_line(&self, line: u32) -> bool {
        line >= self.start_line && line <= self.end_line
    }
}

impl<'a> LoweringContext<'a> {
    const fn new(default_name: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            default_name,
            source,
            next_section_id: 1,
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    fn table_text_blocks(&self, line: u32, maximum: usize) -> Vec<TableTextBlock> {
        let Some(start) = usize::try_from(line)
            .ok()
            .and_then(|line| line.checked_sub(1))
        else {
            return Vec::new();
        };
        let Some(source) = self.source else {
            return Vec::new();
        };
        let mut blocks = Vec::new();
        let mut current = None::<(String, u32)>;
        for (index, line) in source.lines().enumerate().skip(start) {
            let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if let Some((content, start_line)) = current.as_mut() {
                let trimmed = line.trim_start();
                if let Some(remainder) = trimmed.strip_prefix("T}") {
                    blocks.push(TableTextBlock {
                        source: std::mem::take(content),
                        start_line: *start_line,
                        end_line: line_number.saturating_sub(1),
                    });
                    current = None;
                    if blocks.len() == maximum {
                        break;
                    }
                    // tbl serializes adjacent multiline cells as `T}\tT{`.
                    // Closing the first cell must not hide the next opening
                    // marker carried by the same physical source line.
                    if remainder.trim_end().ends_with("T{") {
                        current = Some((String::new(), line_number.saturating_add(1)));
                    }
                } else {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(line);
                }
            } else if line.trim_end().ends_with("T{") {
                current = Some((String::new(), line_number.saturating_add(1)));
            }
        }
        blocks
    }

    fn tab_separated_table_cells(&self, line: u32) -> Option<Vec<&'a str>> {
        let line = usize::try_from(line).ok()?.checked_sub(1)?;
        let source_line = self.source?.lines().nth(line)?;
        source_line
            .contains('\t')
            .then(|| source_line.split('\t').collect())
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

    fn warn_unhandled_structural_parts(&self, node: &Node) {
        let macro_name = node.macro_name.as_deref().unwrap_or("unknown");
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-structural-parts".to_owned()),
            message: format!(
                "structural macro '{macro_name}' contains parts without a complete lowering policy"
            ),
            source: source_span(node),
        });
    }

    fn warn_unhandled_table_text_block(&self, node: &Node) {
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-table-text-block".to_owned()),
            message: "tbl text block contains semantic roff that could not be retained".to_owned(),
            source: source_span(node),
        });
    }

    fn warn_unhandled_table_text_block_line(&self, line: u32) {
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-table-text-block".to_owned()),
            message: "tbl text block contains semantic roff that could not be retained".to_owned(),
            source: Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            }),
        });
    }

    fn warn_unexpanded_table_cell(&self, line: u32) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("manual.unexpanded-table-cell"))
        {
            return;
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.unexpanded-table-cell".to_owned()),
            message: "one or more tbl cells contain formatter strings that could not be expanded; their source spellings were preserved".to_owned(),
            source: Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            }),
        });
    }

    fn take_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.take()
    }
}

fn source_span(node: &Node) -> Option<SourceSpan> {
    (node.line > 0).then_some(SourceSpan {
        byte_range: None,
        line: node.line,
        column: node.column.max(1),
        end_line: None,
        end_column: None,
    })
}

/// Return the first libmandoc structural part of one kind.
///
/// Most semantic macros own at most one head, body, and tail. Callers whose
/// grammar permits repeated parts must use [`part_child_groups`] instead so
/// the multiplicity remains explicit at the lowering boundary.
fn first_part_children(node: &Node, kind: libmandoc_rs::NodeKind) -> &[Node] {
    node.children
        .iter()
        .find(|child| child.kind == kind)
        .map_or(&[], |child| child.children.as_slice())
}

/// Iterate every libmandoc structural part of one kind in source order.
fn part_child_groups(node: &Node, kind: libmandoc_rs::NodeKind) -> impl Iterator<Item = &[Node]> {
    node.children
        .iter()
        .filter(move |child| child.kind == kind)
        .map(|child| child.children.as_slice())
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use mant_ir::{Block, DiagnosticLevel, Inline, SourceFormat};

    use super::{Parser, lower_mandoc_document, parse_manual_bytes, parse_manual_source};

    fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
        fs::write(&path, source).expect("write temporary roff fixture");
        path
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
            [Inline::Strong { .. }, Inline::Emphasis { .. }]
        ));
        assert!(
            items
                .iter()
                .flat_map(|item| item.terms.iter())
                .all(|term| !inline_text(term).contains("96u"))
        );
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
        assert!(children.iter().any(
            |node| matches!(node, Inline::Strong { children } if inline_text(children) == "-w")
        ));
        assert!(children.iter().any(
            |node| matches!(node, Inline::Strong { children } if inline_text(children) == "-W")
        ));
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
        assert!(
            matches!(term[1], Inline::Emphasis { children } if inline_text(children) == "prompt")
        );
        assert!(matches!(term[2], Inline::Text { value } if value == ", "));
        assert!(
            matches!(term[3], Inline::Strong { children } if inline_text(children) == "--prompt=")
        );
        assert!(
            matches!(term[4], Inline::Emphasis { children } if inline_text(children) == "prompt")
        );
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
        let [Block::Paragraph { .. }, Block::Paragraph { layout, .. }] = first.blocks.as_slice()
        else {
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
        assert!(matches!(
            declaration.first(),
            Some(Inline::Strong { children }) if inline_text(children) == "audit_open"
        ));

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
.Fo explicit_call\n.Fa \"int value\"\n.Fc\n\
.Sh DESCRIPTION\nThe\n.Fn prose_call \"int value\"\nfunction.\n",
        )
        .expect("lower mdoc generated punctuation");

        let [Block::Paragraph { children: name, .. }] = document.sections[0].blocks.as_slice()
        else {
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
            ["compact_call(int value);", "explicit_call(int value);"]
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

        assert_eq!(document.sections[0].id, "description-1");
        assert_eq!(document.sections[1].id, "details-2");
        let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
            panic!("expected navigation paragraph");
        };
        assert!(children.iter().any(|inline| matches!(
            inline,
            Inline::Link {
                target: mant_ir::LinkTarget::Section { id },
                children,
                ..
            } if id == "details-2" && inline_text(children) == "DETAILS"
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
        assert!(children.iter().all(|inline| !matches!(
            inline,
            Inline::Link {
                target: mant_ir::LinkTarget::Section { .. },
                ..
            }
        )));
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
    fn masks_terminal_controls_before_native_parsing() {
        let path = temporary_source("controls", ".TH SAFE 1\n.SH NAME\nsafe \x1b[2J text\n");

        let document = parse_manual_source(&path).expect("parse sanitized manual");
        fs::remove_file(path).expect("remove temporary roff fixture");

        assert!(
            document.diagnostics.iter().any(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.control-characters")
            })
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
            document.sections[0]
                .blocks
                .iter()
                .all(|block| !matches!(block, Block::Preformatted { children, .. } if children.is_empty())),
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
            [Inline::Strong { .. }]
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
}
