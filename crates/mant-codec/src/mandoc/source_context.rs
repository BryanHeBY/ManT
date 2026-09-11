//! Source facts and operation-local services; no hidden formatter registers.
use super::{
    BTreeMap, Diagnostic, EquationDelimiterChange, HashMap, HashSet, MacroSet, Node, RefCell,
    SourceLineIndex, equation_delimiter_changes, formatter, inline,
};

pub(super) struct LoweringContext<'a> {
    pub(super) macro_set: MacroSet,
    pub(super) native_heads: RefCell<crate::definitions::NativeHeadEvidence>,
    // Immutable source services. Formatter execution is passed separately;
    // the RefCells below are memoization and diagnostic collection only.
    pub(super) default_name: Option<&'a str>,
    pub(super) source_lines: Option<SourceLineIndex<'a>>,
    table_escape_changes: Vec<TableEscapeChange>,
    pub(super) equation_delimiters: Vec<EquationDelimiterChange>,
    pub(super) normalized_equations: RefCell<BTreeMap<String, String>>,
    pub(super) section_ids: HashMap<String, usize>,
    pub(super) assigned_section_ids: HashSet<String>,
    pub(super) explicit_targets: HashSet<String>,
    pub(super) diagnostics: RefCell<Vec<Diagnostic>>,
}

#[derive(Debug)]
pub(super) struct TableTextBlock {
    pub(super) source: String,
    pub(super) start_line: u32,
    pub(super) end_line: u32,
}

#[derive(Clone, Copy, Debug)]
struct TableEscapeChange {
    line: u32,
    escape: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
enum TableEscapeRequest {
    Set(u8),
    Disable,
}

impl TableTextBlock {
    pub(super) const fn contains_line(&self, line: u32) -> bool {
        line >= self.start_line && line <= self.end_line
    }
}

/// Apply the lexical comment rule that roff executes before handing text to
/// tbl.
///
/// This intentionally is not a second roff interpreter.  Source-backed tbl
/// recovery only needs the one lexical transformation that both CVS mandoc
/// and groff perform before parsing a table cell: `\\\"` discards the rest
/// of the physical line and `\\#` discards it while continuing with the next
/// line.  Keeping it at the source-recovery boundary prevents comment prose
/// from becoming a synthetic table cell when libmandoc has correctly omitted
/// that cell from its owned table snapshot.
fn table_execution_source(source: &str, initial_escape: Option<u8>) -> String {
    let mut output = String::with_capacity(source.len());
    let mut escape = initial_escape;
    for physical_line in source.split_inclusive('\n') {
        let (line, had_newline) = physical_line.strip_suffix("\r\n").map_or_else(
            || {
                physical_line
                    .strip_suffix('\n')
                    .map_or((physical_line, false), |line| (line, true))
            },
            |line| (line, true),
        );
        let (visible, continues) = roff_line_without_comment(line, escape);
        output.push_str(visible);
        if had_newline && !continues {
            output.push('\n');
        }
        if let Some(request) = table_escape_request(visible) {
            escape = request.apply();
        }
    }
    output
}

/// Return the printable prefix of one roff input line and whether `\\#`
/// suppresses its trailing newline.
///
/// CVS mandoc's `roff_parse_comment()` scans a physical line before
/// `tbl_read()`: an escaped escape skips both bytes, while `\\\"` and `\\#`
/// start a comment.  This mirrors only that lexical contract; callers supply
/// the already-executed `.ec`/`.eo` state for their source coordinate.
fn roff_line_without_comment(line: &str, escape: Option<u8>) -> (&str, bool) {
    let Some(escape) = escape else {
        return (line, false);
    };
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != escape {
            index += 1;
            continue;
        }
        match bytes.get(index.saturating_add(1)).copied() {
            Some(next) if next == escape => index = index.saturating_add(2),
            Some(b'"' | b'#') => {
                let mut end = index;
                // Match mandoc's deliberate whitespace rule: remove only
                // literal spaces immediately preceding a comment, unless
                // that space was itself escaped.
                while end > 0 && bytes[end - 1] == b' ' && (end == 1 || bytes[end - 2] != escape) {
                    end -= 1;
                }
                return (&line[..end], bytes[index + 1] == b'#');
            }
            _ => index += 1,
        }
    }
    (line, false)
}

/// Return an escape-state transition executed by a plain roff control line.
///
/// CVS mandoc implements only `.ec` and `.eo`; `ecs`/`ecr` remain unsupported
/// there, so this bounded source service deliberately follows that parser
/// rather than growing a broader, incompatible roff interpreter.
fn table_escape_request(line: &str) -> Option<TableEscapeRequest> {
    let trimmed = line.trim_start();
    let request = trimmed
        .strip_prefix('.')
        .or_else(|| trimmed.strip_prefix('\''))?;
    let name_end = request.find(char::is_whitespace).unwrap_or(request.len());
    let name = &request[..name_end];
    let argument = request[name_end..].trim_start();
    match name {
        "ec" => Some(TableEscapeRequest::Set(
            argument.as_bytes().first().copied().unwrap_or(b'\\'),
        )),
        "eo" => Some(TableEscapeRequest::Disable),
        _ => None,
    }
}

fn table_escape_changes(source: &str) -> Vec<TableEscapeChange> {
    let mut changes = Vec::new();
    let mut escape = Some(b'\\');
    for (index, line) in source.lines().enumerate() {
        let (visible, _) = roff_line_without_comment(line, escape);
        if let Some(request) = table_escape_request(visible) {
            escape = request.apply();
            changes.push(TableEscapeChange {
                line: u32::try_from(index.saturating_add(2)).unwrap_or(u32::MAX),
                escape,
            });
        }
    }
    changes
}

impl TableEscapeRequest {
    const fn apply(self) -> Option<u8> {
        match self {
            Self::Set(escape) => Some(escape),
            Self::Disable => None,
        }
    }
}

impl<'a> LoweringContext<'a> {
    pub(super) fn new(default_name: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            macro_set: MacroSet::None,
            native_heads: RefCell::default(),
            default_name,
            source_lines: source.map(SourceLineIndex::new),
            table_escape_changes: source.map_or_else(Vec::new, table_escape_changes),
            equation_delimiters: source.map_or_else(Vec::new, equation_delimiter_changes),
            normalized_equations: RefCell::new(BTreeMap::new()),
            section_ids: HashMap::new(),
            assigned_section_ids: HashSet::new(),
            explicit_targets: HashSet::new(),
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn lower_inline_with_spacing(
        &self,
        nodes: &[Node],
        spacing: bool,
        formatter: &mut formatter::FormatterState,
    ) -> Vec<mant_ir::Inline> {
        if self.macro_set != MacroSet::Mdoc {
            return inline::lower_inline_nodes_with_spacing(nodes, self.default_name, spacing);
        }
        let mut builder = inline::InlineBuilder::with_spacing(spacing);
        builder.font = formatter.font;
        inline::append_inline_nodes(&mut builder, nodes, self.default_name);
        formatter.font = builder.font;
        formatter.spacing = builder.spacing_enabled();
        builder.finish()
    }

    pub(super) fn lower_text(
        &self,
        source: &str,
        formatter: &mut formatter::FormatterState,
    ) -> Vec<mant_ir::Inline> {
        if self.macro_set != MacroSet::Mdoc {
            return inline::parse_roff_text(source);
        }
        inline::parse_roff_text_with_state(source, &mut formatter.font, true)
    }

    pub(super) fn table_execution_source(&self, line: u32, source: &str) -> String {
        table_execution_source(source, self.table_escape_at(line))
    }

    pub(super) fn table_text_blocks(&self, line: u32, maximum: usize) -> Vec<TableTextBlock> {
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
            // Interpret tbl's `T{` / `T}` sentinels after roff has removed
            // inline comments.  A comment can follow a real sentinel, while
            // a comment-only request can mention a disabled sentinel without
            // claiming a later text block.
            let (visible_line, _) =
                roff_line_without_comment(line, self.table_escape_at(line_number));
            let trimmed = visible_line.trim_start();
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

    pub(super) fn tab_separated_table_cells(&self, line: u32) -> Option<Vec<&'a str>> {
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
    pub(super) fn man_ip_uses_incrementing_register(&self, line: u32) -> bool {
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

    fn table_escape_at(&self, line: u32) -> Option<u8> {
        self.table_escape_changes
            .iter()
            .rev()
            .find(|change| change.line <= line)
            .map_or(Some(b'\\'), |change| change.escape)
    }
}

#[cfg(test)]
mod tests {
    use super::{LoweringContext, table_execution_source};

    #[test]
    fn table_execution_source_matches_roff_inline_comment_and_continuation_rules() {
        assert_eq!(
            table_execution_source("visible \\\" ignored\nnext", Some(b'\\')),
            "visible\nnext"
        );
        assert_eq!(
            table_execution_source("joined \\# ignored\nnext", Some(b'\\')),
            "joinednext"
        );
        assert_eq!(
            table_execution_source(r#"literal \\" remains"#, Some(b'\\')),
            r#"literal \\" remains"#
        );
        assert_eq!(
            table_execution_source(r#"escaped\ \" comment"#, Some(b'\\')),
            r"escaped\ "
        );
    }

    #[test]
    fn table_execution_source_uses_the_document_escape_state_at_the_cell() {
        let context = LoweringContext::new(None, Some(".ec @\n.TS\n"));
        assert_eq!(
            context.table_execution_source(2, "visible @\" ignored"),
            "visible"
        );
        assert_eq!(
            context.table_execution_source(2, "visible \\\" literal"),
            "visible \\\" literal"
        );
    }
}
