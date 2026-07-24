//! Maps parser byte ranges back to exact Markdown slices and source locations.

use std::ops::Range;

use mant_ast::{Block, Diagnostic, DiagnosticLevel, LayoutHint, SourceSpan};

/// Original Markdown together with a compact byte-to-line index.
pub(super) struct MarkdownSource<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> MarkdownSource<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            text.match_indices('\n')
                .map(|(offset, _)| offset.saturating_add(1)),
        );
        Self { text, line_starts }
    }

    pub(super) fn raw(&self, range: &Range<usize>) -> &'a str {
        let start = range.start.min(self.text.len());
        let end = range.end.clamp(start, self.text.len());
        self.text.get(start..end).unwrap_or_default()
    }

    pub(super) fn span(&self, range: &Range<usize>) -> SourceSpan {
        let start = self.position(range.start);
        let end = self.position(range.end);
        SourceSpan {
            line: start.0,
            column: start.1,
            end_line: Some(end.0),
            end_column: Some(end.1),
        }
    }

    pub(super) fn unsupported_block(
        &self,
        name: &str,
        range: Range<usize>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Block {
        self.report_unsupported(name, range.clone(), diagnostics);
        Block::Unsupported {
            name: Some(name.to_owned()),
            text: self.raw(&range).to_owned(),
            layout: LayoutHint::default(),
            source: Some(self.span(&range)),
        }
    }

    pub(super) fn unsupported_inline(
        &self,
        name: &str,
        range: Range<usize>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> String {
        self.report_unsupported(name, range.clone(), diagnostics);
        self.raw(&range).to_owned()
    }

    fn report_unsupported(
        &self,
        name: &str,
        range: Range<usize>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Unsupported,
            code: Some("markdown.unsupported".to_owned()),
            message: format!("preserved unsupported Markdown {name} as source text"),
            source: Some(self.span(&range)),
        });
    }

    fn position(&self, offset: usize) -> (u32, u32) {
        let offset = offset.min(self.text.len());
        let line_index = self.line_starts.partition_point(|start| *start <= offset);
        let line_index = line_index.saturating_sub(1);
        let line_start = self.line_starts[line_index];
        (
            u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
            u32::try_from(offset.saturating_sub(line_start).saturating_add(1)).unwrap_or(u32::MAX),
        )
    }
}
