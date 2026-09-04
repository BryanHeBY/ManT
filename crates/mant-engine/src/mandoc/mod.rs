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
mod source_lines;
mod targets;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

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
    source_lines::SourceLineIndex,
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
    let target_plan = targets::NativeTargetPlan::build(&parsed.root);
    let explicit_targets = target_plan.explicit();
    let mut context = LoweringContext::new(parsed.metadata.name.as_deref(), source);
    context.reserve_section_ids(explicit_targets);
    let mut diagnostics = diagnostics::lower_diagnostics(&report.diagnostics);
    let mut sections = blocks::lower_sections(&parsed.root, &mut context);
    let mut root_blocks = blocks::lower_root_blocks(&parsed.root, &context);
    diagnostics.extend(context.take_diagnostics());
    navigation::normalize_generated_anchors(&mut root_blocks, &mut sections, explicit_targets);
    let mut retained_targets = navigation::native_anchor_ids(&root_blocks, &sections);
    retained_targets.extend(explicit_targets.iter().cloned());
    retained_targets.extend(crate::definitions::identify_definitions(
        &mut root_blocks,
        &mut sections,
        explicit_targets,
        parsed.metadata.name.as_deref(),
    ));
    diagnostics.extend(crate::projection::semantic_selector_diagnostics(
        &root_blocks,
        &sections,
        "manual",
    ));
    diagnostics.extend(crate::definitions::manual_discovery_diagnostics(&sections));
    navigation::resolve_navigation(
        &mut root_blocks,
        &mut sections,
        &retained_targets,
        &mut diagnostics,
    );
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
        fragment_aliases: Vec::new(),
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
    section_ids: HashMap<String, usize>,
    assigned_section_ids: HashSet<String>,
    explicit_targets: HashSet<String>,
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
            section_ids: HashMap::new(),
            assigned_section_ids: HashSet::new(),
            explicit_targets: HashSet::new(),
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

    fn reserve_section_ids(&mut self, ids: &HashSet<String>) {
        self.explicit_targets.clone_from(ids);
        self.assigned_section_ids.extend(ids.iter().cloned());
    }

    fn section_identity_for(
        &mut self,
        title: &str,
        node: &Node,
    ) -> (String, Vec<mant_ir::FragmentAlias>) {
        let id = self.section_id(title);
        let fragment_aliases = targets::section_target(node)
            .filter(|target| self.explicit_targets.contains(target))
            .map(|target| vec![target.into()])
            .unwrap_or_default();
        (id, fragment_aliases)
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
        let normalized = Parser::default()
            .parse_bytes(Path::new("mant-inline-eqn.7"), synthetic.as_bytes())
            .ok()
            .and_then(|report| first_equation(&report.document.root).map(visible_text))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| visible_text(source));
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

    /// Whether a source-level `.IP` marker uses roff's pre-increment form.
    ///
    /// libmandoc resolves number registers before exposing the owned AST, so
    /// `\n+[step]` and a literal value such as `1` otherwise become
    /// indistinguishable.  Retain only this narrow source fact: it proves an
    /// author-controlled sequence without teaching lowering a second roff
    /// parser or reinterpreting literal numeric option values.
    fn man_ip_uses_incrementing_register(&self, line: u32) -> bool {
        let Some(source_line) = self
            .source_lines
            .as_ref()
            .and_then(|source| source.line(line))
        else {
            return false;
        };
        let Some(request) = source_line
            .trim_start()
            .strip_prefix(['.', '\''])
            .map(str::trim_start)
        else {
            return false;
        };
        let Some(arguments) = request
            .strip_prefix("IP")
            .filter(|rest| rest.chars().next().is_none_or(char::is_whitespace))
        else {
            return false;
        };
        inline::roff_macro_arguments(arguments)
            .first()
            .is_some_and(|head| head.contains("\\n+"))
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
        let base = if slug.is_empty() {
            "section".to_owned()
        } else if crate::projection::is_reserved_selector(&slug) {
            format!("{slug}-section")
        } else {
            slug
        };
        let count = self.section_ids.entry(base.clone()).or_default();
        loop {
            *count += 1;
            let candidate = if *count == 1 {
                base.clone()
            } else {
                format!("{base}-{count}")
            };
            if self.assigned_section_ids.insert(candidate.clone()) {
                return candidate;
            }
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

    fn warn_definition_alias_boundary(&self, node: &Node) {
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.definition-alias-boundary".to_owned()),
            message: "unlabelled definition heads were kept separate because this macro does not prove that they share one description".to_owned(),
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
mod tests;
