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

impl TableTextBlock {
    pub(super) const fn contains_line(&self, line: u32) -> bool {
        line >= self.start_line && line <= self.end_line
    }
}

impl<'a> LoweringContext<'a> {
    pub(super) fn new(default_name: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            macro_set: MacroSet::None,
            native_heads: RefCell::default(),
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
}
