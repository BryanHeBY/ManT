//! Project borrowed inline owners into styled rows and deferred targets.
use super::super::inline::{styled_plain_text_lines, styled_reference_content_lines};
use super::super::{
    LineSurface, LogicalLine, Style, StyledInlineLine, WrapMode, inline_anchor_rows, theme,
};
use super::DocumentBuilder;

impl DocumentBuilder<'_> {
    /// Headings use the same original inline path as prose: links, anchors,
    /// hard lines and nested source styles must not pass through a plain label.
    pub(in crate::document) fn heading(&mut self, heading: &mant_ir::Heading, indent: i32) {
        self.inline_lines_with_surface(
            mant_ir::InlineContentRef {
                content: &heading.content,
                layout: &heading.inline_layout,
            },
            indent,
            theme::style(theme::StyleRole::Heading),
            LineSurface::Normal,
        );
    }

    pub(super) fn plain_block_lines(
        &mut self,
        value: &str,
        indent: i32,
        style: Style,
        wrap_mode: WrapMode,
    ) {
        self.push_styled_lines(
            styled_plain_text_lines(value, style),
            indent,
            LineSurface::Normal,
            wrap_mode,
        );
    }

    #[cfg(test)]
    pub(in crate::document) fn inline_lines(
        &mut self,
        nodes: &[mant_ir::Inline],
        indent: i32,
        base_style: Style,
    ) {
        self.inline_lines_with_surface(
            mant_ir::InlineContentRef::unpositioned(nodes),
            indent,
            base_style,
            LineSurface::Normal,
        );
    }

    pub(super) fn styled_inlines(
        &self,
        content: mant_ir::InlineContentRef<'_>,
        style: Style,
    ) -> Vec<StyledInlineLine> {
        styled_reference_content_lines(
            content,
            style,
            self.address.as_ref(),
            self.entry_styles.ranges(content.content),
            false,
            &self.reference_origins,
        )
    }

    pub(in crate::document) fn inline_lines_with_surface(
        &mut self,
        content: mant_ir::InlineContentRef<'_>,
        indent: i32,
        base_style: Style,
        surface: LineSurface,
    ) {
        self.inline_lines_with_geometry(content, indent, indent, base_style, surface);
    }

    pub(super) fn inline_lines_with_geometry(
        &mut self,
        content: mant_ir::InlineContentRef<'_>,
        indent: i32,
        continuation: i32,
        base_style: Style,
        surface: LineSurface,
    ) {
        self.inline_lines_with_geometry_tail(
            content,
            indent,
            continuation,
            base_style,
            surface,
            false,
        );
    }

    pub(super) fn inline_lines_with_geometry_tail(
        &mut self,
        content: mant_ir::InlineContentRef<'_>,
        indent: i32,
        continuation: i32,
        base_style: Style,
        surface: LineSurface,
        trim_paragraph_tail: bool,
    ) {
        let nodes = content.content;
        let targets = inline_anchor_rows(nodes);
        let mut lines = styled_reference_content_lines(
            content,
            base_style,
            self.address.as_ref(),
            self.entry_styles.ranges(nodes),
            surface == LineSurface::Code,
            &self.reference_origins,
        );
        let mut deferred_targets = if trim_paragraph_tail {
            Self::trim_paragraph_tail(&mut lines)
        } else {
            Vec::new()
        };
        if lines.len() == 1
            && lines[0].spans.is_empty()
            && mant_ir::logical_row_count(nodes) == 1
            && !(surface == LineSurface::Code && mant_ir::geometry::has_literal_rows(nodes))
        {
            self.defer_anchors(targets.into_iter().map(|(id, _)| id));
            self.defer_anchors(
                lines[0]
                    .reference_marks
                    .iter()
                    .map(|mark| mark.id.to_string()),
            );
            return;
        }
        for (id, row) in targets {
            if trim_paragraph_tail && row >= lines.len() {
                deferred_targets.push(id);
            } else {
                self.anchors.entry(id).or_insert(self.lines.len() + row);
            }
        }
        self.push_styled_lines_with_geometry(
            lines,
            indent,
            continuation,
            surface,
            if surface == LineSurface::Code {
                WrapMode::Character
            } else {
                WrapMode::Word
            },
        );
        self.defer_anchors(deferred_targets);
    }

    /// A final delimiter opens one provisional row. Earlier empty rows have
    /// already completed and remain visible (`term.c::term_newln/term_vspace`).
    pub(super) fn trim_paragraph_tail(lines: &mut Vec<StyledInlineLine>) -> Vec<String> {
        if lines.len() > 1
            && lines
                .last()
                .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
        {
            let tail = lines.pop().expect("ordinary paragraph terminator");
            return tail
                .reference_marks
                .into_iter()
                .map(|mark| mark.id.to_string())
                .collect();
        }
        Vec::new()
    }

    fn push_styled_lines(
        &mut self,
        lines: Vec<StyledInlineLine>,
        indent: i32,
        surface: LineSurface,
        wrap_mode: WrapMode,
    ) {
        self.push_styled_lines_with_geometry(lines, indent, indent, surface, wrap_mode);
    }

    fn push_styled_lines_with_geometry(
        &mut self,
        lines: Vec<StyledInlineLine>,
        indent: i32,
        continuation: i32,
        surface: LineSurface,
        wrap_mode: WrapMode,
    ) {
        for (index, line) in lines.into_iter().enumerate() {
            let mut logical = LogicalLine::row_geometry(
                if index == 0 { indent } else { continuation },
                continuation,
                line.indent_columns,
                line.spans,
            );
            logical.surface = surface;
            logical.wrap_mode = wrap_mode;
            logical.links = line.links;
            logical.reference_marks = line.reference_marks;
            self.push(logical);
        }
    }
}
