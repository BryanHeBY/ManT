//! Lowers the engine-owned syntax projection into `ManT`'s stable document model.

mod blocks;
mod diagnostics;
mod error;
pub(crate) mod inline;
mod layout;
mod native_adapter;
mod navigation;
mod reference;
mod roff_escape;
mod source;
mod source_lines;
mod syntax;

use mant_ir::{
    Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, ParserInfo, SourceFormat,
    SourceSpan, validate_document,
};
use mantdoc::{
    DiagnosticProfile, Parser as NativeParser, ParserConfig, Source as NativeSource, SourceName,
};
use std::{cell::RefCell, collections::BTreeMap, path::Path};

use self::{
    roff_escape::visible_text,
    source::{load_manual_source, redirect_target, resolve_manual_redirects},
    source_lines::SourceLineIndex,
    syntax::{Document as MandocDocument, MacroSet, Node, NodeKind, ParseReport},
};
use crate::ManualPage;
use crate::text_safety::mask_terminal_control_bytes;

pub use error::{ManualError, ManualErrorKind};
pub use source::MAX_MANUAL_BYTES;

const MAX_INLINE_EQUATION_NORMALIZATIONS: usize = 256;

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
/// Returns [`ManualError`] when the parser rejects the input.
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
    let source_text = String::from_utf8_lossy(source.as_ref());
    let source_name = SourceName::new(path.to_string_lossy().as_ref())
        .map_err(|error| ManualError::native_parse(path, error.to_string()))?;
    let native_report = legacy_compatible_parser()
        .parse(NativeSource::new(&source_name, source.as_ref()))
        .map_err(|error| ManualError::native_parse(path, error.to_string()))?;
    let report = native_adapter::project(&native_report);
    let mut document = lower_mandoc_document_with_source(path, &report, Some(&source_text));
    document.parser = Some(ParserInfo {
        name: "mantdoc".to_owned(),
        version: mantdoc::MANTDOC_VERSION.to_owned(),
    });
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

fn legacy_compatible_parser() -> NativeParser {
    NativeParser::new(ParserConfig {
        diagnostic_profile: DiagnosticProfile::LibmandocRsV0_9,
        ..ParserConfig::default()
    })
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
        parsed.metadata.name.as_deref(),
    ));
    navigation::resolve_navigation(&mut sections, &retained_targets, &mut diagnostics);
    let mut document = Document {
        parser: None,
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
    source_lines: Option<SourceLineIndex<'a>>,
    equation_delimiters: Vec<EquationDelimiterChange>,
    normalized_equations: RefCell<BTreeMap<String, String>>,
    next_section_id: usize,
    diagnostics: RefCell<Vec<Diagnostic>>,
}

#[derive(Clone, Copy, Debug)]
struct EquationDelimiterChange {
    line: u32,
    delimiters: Option<(char, char)>,
}

#[derive(Clone, Copy, Debug)]
enum EquationDelimiterDirective {
    Enable(char, char),
    Disable,
}

impl EquationDelimiterDirective {
    const fn delimiters(self) -> Option<(char, char)> {
        match self {
            Self::Enable(opening, closing) => Some((opening, closing)),
            Self::Disable => None,
        }
    }
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
    fn new(default_name: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            default_name,
            source_lines: source.map(SourceLineIndex::new),
            equation_delimiters: source.map_or_else(Vec::new, equation_delimiter_changes),
            normalized_equations: RefCell::new(BTreeMap::new()),
            next_section_id: 1,
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    fn equation_delimiters_at(&self, line: u32) -> Option<(char, char)> {
        self.equation_delimiters
            .iter()
            .rev()
            .find(|change| change.line <= line)
            .and_then(|change| change.delimiters)
    }

    /// Normalize an eqn fragment through the same pinned parser used for
    /// display equations. tbl retains delimiter-wrapped cell text as an
    /// opaque string, so reparsing only that bounded fragment is the sole way
    /// to avoid a second, incomplete eqn grammar in the lowering layer.
    fn normalize_equation(&self, source: &str, line: u32) -> String {
        {
            let normalized = self.normalized_equations.borrow();
            if let Some(value) = normalized.get(source) {
                return value.clone();
            }
            if normalized.len() >= MAX_INLINE_EQUATION_NORMALIZATIONS {
                drop(normalized);
                self.warn_inline_equation_budget(line);
                return visible_text(source);
            }
        }
        let synthetic = format!(".TH MANT-EQN 7\n.EQ\n{source}\n.EN\n");
        let source_name = SourceName::new("mant-inline-eqn.7")
            .expect("fixed inline equation source name is valid");
        let normalized = NativeParser::default()
            .parse(NativeSource::new(&source_name, synthetic.as_bytes()))
            .ok()
            .and_then(|report| {
                let report = native_adapter::project(&report);
                first_equation(&report.document.root).map(visible_text)
            })
            .filter(|value| !value.trim().is_empty())
            .map_or_else(
                || visible_text(source),
                |value| retain_equation_delimiter_spacing(source, value),
            );
        self.normalized_equations
            .borrow_mut()
            .insert(source.to_owned(), normalized.clone());
        normalized
    }

    fn table_text_blocks(&self, line: u32, maximum: usize) -> Vec<TableTextBlock> {
        // Ordinary tbl rows must not scan forward for `T{` markers.  Besides
        // wasting work, that used to let a commented-out multiline-cell
        // marker claim later real rows as its embedded semantic children.
        if maximum == 0 {
            return Vec::new();
        }
        let Some(source_lines) = self.source_lines.as_ref() else {
            return Vec::new();
        };
        let mut blocks = Vec::new();
        let mut current = None::<(String, u32)>;
        for (line_number, line) in source_lines.lines_from(line) {
            let trimmed = line.trim_start();
            // `.\\"` comments are not tbl control lines, even when their
            // prose contains a disabled `T{` or `T}` marker.  Ignore them
            // both while looking for a block and inside an active block,
            // matching roff's non-printing comment semantics.
            if trimmed.starts_with(".\\\"") || trimmed.starts_with("'\\\"") {
                continue;
            }
            if let Some((content, start_line)) = current.as_mut() {
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
            } else if trimmed.trim_end().ends_with("T{") {
                current = Some((String::new(), line_number.saturating_add(1)));
            }
        }
        blocks
    }

    fn tab_separated_table_cells(&self, line: u32) -> Option<Vec<&'a str>> {
        let source_line = self.source_lines.as_ref()?.line(line)?;
        source_line
            .contains('\t')
            .then(|| source_line.split('\t').collect())
    }

    /// Return explicitly requested blank rows between two visible no-fill
    /// source lines.
    ///
    /// Some structural AST forms omit blank physical input rows.  That is
    /// normally the right tree representation, but no-fill displays make the
    /// rows observable.  Source spans let lowering restore raw blank runs and
    /// `.sp` requests, without mistaking comments or hidden state changes for
    /// vertical content.  Consecutive empty input lines are one visual
    /// separator in roff no-fill output, so they must not accumulate.
    pub(super) fn no_fill_blank_rows_between(
        &self,
        previous_line: Option<u32>,
        current_line: Option<u32>,
    ) -> u16 {
        let Some((previous, current)) = previous_line.zip(current_line) else {
            return 0;
        };
        if current <= previous.saturating_add(1) {
            return 0;
        }
        let Some(source_lines) = self.source_lines.as_ref() else {
            return 0;
        };
        source_lines
            .lines_between(previous, current)
            .map(no_fill_vertical_rows)
            .max()
            .unwrap_or(0)
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

    fn warn_inline_equation_budget(&self, line: u32) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("manual.inline-equation-budget"))
        {
            return;
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.inline-equation-budget".to_owned()),
            message: format!(
                "more than {MAX_INLINE_EQUATION_NORMALIZATIONS} distinct inline table equations; later source spellings were retained without normalization"
            ),
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

fn first_equation(node: &Node) -> Option<&str> {
    node.equation
        .as_deref()
        .or_else(|| node.children.iter().find_map(first_equation))
}

/// Preserve observable blank space immediately inside literal equation
/// delimiters.  `mantdoc` normalizes equation tokens, whereas libmandoc's
/// owned-AST projection retains this particular layout detail.  The lowerer
/// still has the bounded source fragment for a tbl cell, so restoring it here
/// keeps the public document contract stable without a second parser.
fn retain_equation_delimiter_spacing(source: &str, mut normalized: String) -> String {
    let source = source.trim();
    let Some(opening) = source.chars().next() else {
        return normalized;
    };
    let closing = match opening {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return normalized,
    };
    if !source.ends_with(closing)
        || !normalized.starts_with(opening)
        || !normalized.ends_with(closing)
    {
        return normalized;
    }

    let source_inner = &source[opening.len_utf8()..source.len() - closing.len_utf8()];
    if source_inner.starts_with(char::is_whitespace)
        && !normalized[opening.len_utf8()..].starts_with(char::is_whitespace)
    {
        normalized.insert(opening.len_utf8(), ' ');
    }
    if source_inner.ends_with(char::is_whitespace)
        && !normalized[..normalized.len() - closing.len_utf8()].ends_with(char::is_whitespace)
    {
        normalized.insert(normalized.len() - closing.len_utf8(), ' ');
    }
    normalized
}

/// Track active inline eqn delimiters at each source line.
///
/// eqn configures delimiters inside an `.EQ`/`.EN` block; they take effect on
/// following prose and tbl cells. Keeping the change points makes lookup
/// logarithm-free and deterministic without replaying the whole source for
/// every table cell.
fn equation_delimiter_changes(source: &str) -> Vec<EquationDelimiterChange> {
    let mut changes = Vec::new();
    let mut in_equation = false;
    let mut pending = None;
    for (index, source_line) in source.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = source_line.trim();
        if trimmed.starts_with(".\\\"") || trimmed.starts_with("'\\\"") {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix(".EQ")
            .or_else(|| trimmed.strip_prefix("'EQ"))
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            in_equation = true;
            pending = parse_equation_delimiters(rest.trim()).or(pending);
            continue;
        }
        if in_equation {
            if trimmed == ".EN" || trimmed == "'EN" {
                if let Some(delimiters) = pending.take() {
                    changes.push(EquationDelimiterChange {
                        line: line.saturating_add(1),
                        delimiters: delimiters.delimiters(),
                    });
                }
                in_equation = false;
            } else if let Some(delimiters) = parse_equation_delimiters(trimmed) {
                pending = Some(delimiters);
            }
        }
    }
    changes
}

fn parse_equation_delimiters(value: &str) -> Option<EquationDelimiterDirective> {
    let value = value.strip_prefix("delim")?.trim_start();
    if value == "off" {
        return Some(EquationDelimiterDirective::Disable);
    }
    let mut delimiters = value.chars();
    let opening = delimiters.next()?;
    let closing = delimiters.next()?;
    Some(EquationDelimiterDirective::Enable(opening, closing))
}

/// Return the largest explicit vertical separation requested by one source
/// line.  A raw blank line is a single separator; `no_fill_blank_rows_between`
/// deliberately takes the maximum across adjacent source lines because groff
/// collapses a run of blank input lines in a no-fill display.
fn no_fill_vertical_rows(line: &str) -> u16 {
    let trimmed = line.trim();
    if trimmed.is_empty() || roff_zero_width_blank_line(trimmed) {
        return 1;
    }
    let Some(request) = line.trim_start().strip_prefix(['.', '\'']) else {
        return 0;
    };
    let (name, arguments) = request
        .split_once(char::is_whitespace)
        .unwrap_or((request, ""));
    if name != "sp" {
        return 0;
    }
    let Some(argument) = arguments.split_whitespace().next() else {
        return 1;
    };
    argument.trim_end_matches('v').parse::<u16>().unwrap_or(1)
}

/// Whether a no-fill input row contains only roff's zero-width guard escape.
///
/// POD-generated manuals use `\&` instead of an empty physical input line.
/// libmandoc correctly lowers that escape to no glyph, but the row itself is
/// still observable inside a verbatim display. Keep this deliberately narrow:
/// font switches and other state-only input do not independently request a
/// visual row, while `\c` explicitly suppresses the line boundary.
fn roff_zero_width_blank_line(line: &str) -> bool {
    let mut remainder = line;
    let mut found = false;
    while let Some(rest) = remainder.strip_prefix(r"\&") {
        found = true;
        remainder = rest.trim();
    }
    found && remainder.is_empty()
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
fn first_part_children(node: &Node, kind: NodeKind) -> &[Node] {
    node.children
        .iter()
        .find(|child| child.kind == kind)
        .map_or(&[], |child| child.children.as_slice())
}

/// Iterate every libmandoc structural part of one kind in source order.
fn part_child_groups(node: &Node, kind: NodeKind) -> impl Iterator<Item = &[Node]> {
    node.children
        .iter()
        .filter(move |child| child.kind == kind)
        .map(|child| child.children.as_slice())
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, fs, process};

    use mant_ir::{Block, DiagnosticLevel, Inline, SourceFormat};

    use super::{MAX_INLINE_EQUATION_NORMALIZATIONS, parse_manual_bytes, parse_manual_source};

    fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
        fs::write(&path, source).expect("write temporary roff fixture");
        path
    }

    #[test]
    fn standalone_inputs_reject_redirect_only_so_pages() {
        let error = parse_manual_bytes(std::path::Path::new("stdin"), b".so man1/target.1\n")
            .expect_err("standalone input must not follow another file");
        assert!(error.to_string().contains("require MANPATH discovery"));
    }

    #[test]
    fn reports_the_native_parser_name_and_version() {
        let document = parse_manual_bytes(
            std::path::Path::new("version.1"),
            b".TH VERSION 1\n.SH NAME\nversion\n",
        )
        .expect("parse native manual");
        let parser = document.parser.expect("native parser provenance");
        assert_eq!(parser.name, "mantdoc");
        assert_eq!(parser.version, mantdoc::MANTDOC_VERSION);
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

        let document =
            parse_manual_bytes(std::path::Path::new("no-fill-scale.7"), source.as_bytes())
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
            ["-a", "--alpha", "--ALPHA"]
        );
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-a", "--alpha", "--ALPHA"]
        );
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
.Fo explicit_call\n.Fa \"int value\" \"const char *label\"\n.Fc\n\
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
            } if id == "white-space-splitting-field-splitting-2"
                && inline_text(children) == "White Space Splitting"
        )));
        assert!(document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        }));
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
    fn lowers_the_checked_in_mdoc_fixture_without_empty_sections() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/roff/minimal-mdoc.1");
        if !source.exists() {
            // The workspace fixture is intentionally not packaged with the
            // independent mant-engine crate.
            return;
        }

        let document = parse_manual_source(&source).expect("lower checked-in mdoc fixture");

        assert!(document.sections.len() > 1);
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
        assert!(children.iter().any(
            |child| matches!(child, Inline::Code { value } if value == "i = 1 , ... , n + 1")
        ));
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

        assert!(document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
        }));
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
}
