//! Lower quick-reference content into its existing panel surfaces.
use super::super::{
    ExternalUri, LineSurface, LinkTarget, LogicalLine, LogicalLinkRange, NavKind, NavNode, Span,
    TLDR_ID, TLDR_VERTICAL_PADDING_ROWS, TldrDocument, WrapMode, theme, tldr_style,
};
use super::DocumentBuilder;

impl DocumentBuilder<'_> {
    pub(in crate::document) fn tldr(
        &mut self,
        tldr: &TldrDocument,
        has_document: bool,
        source_label: &'static str,
        document_gap: u16,
    ) {
        self.anchor(NavNode {
            id: TLDR_ID.to_owned(),
            target_id: TLDR_ID.to_owned(),
            title: "TLDR QUICK REFERENCE".to_owned(),
            full_title: None,
            depth: 0,
            kind: NavKind::Tldr,
            has_children: false,
            is_last: false,
            parent_id: None,
        });
        self.push(LogicalLine::empty().surface(LineSurface::TldrTop));
        for _ in 0..TLDR_VERTICAL_PADDING_ROWS {
            self.push(LogicalLine::empty().surface(LineSurface::Tldr));
        }
        for line in mant_render::layout_tldr(tldr) {
            let command = line.spans.iter().any(|span| {
                matches!(
                    span.role,
                    mant_render::TldrRole::Command | mant_render::TldrRole::Placeholder
                )
            });
            let links = line
                .spans
                .iter()
                .scan(0, |offset, span| {
                    let start = *offset;
                    *offset += span.text.chars().count();
                    Some((span, start, *offset))
                })
                .filter(|(span, _, _)| span.role == mant_render::TldrRole::Link)
                .filter_map(|(_, start_scalar, end_scalar)| {
                    tldr.more_information
                        .as_deref()
                        .and_then(ExternalUri::parse)
                        .map(|uri| LogicalLinkRange {
                            target: LinkTarget::External(uri),
                            start_scalar,
                            end_scalar,
                        })
                })
                .collect();
            self.push(LogicalLine {
                indent: line.indent,
                continuation_indent: line.indent,
                layout_padding: 0,
                continuation_layout_padding: 0,
                layout_scalars: Vec::new(),
                spans: line
                    .spans
                    .into_iter()
                    .map(|span| Span::styled(span.text, tldr_style(span.role)))
                    .collect(),
                surface: LineSurface::Tldr,
                wrap_mode: if command {
                    WrapMode::Character
                } else {
                    WrapMode::Word
                },
                table_row: None,
                links,
                reference_marks: Vec::new(),
            });
        }
        for _ in 0..TLDR_VERTICAL_PADDING_ROWS {
            self.push(LogicalLine::empty().surface(LineSurface::Tldr));
        }
        self.push(LogicalLine::empty().surface(LineSurface::TldrBottom));
        self.spacing(document_gap);
        if has_document {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                source_label,
                theme::style(theme::StyleRole::Metadata),
            ));
            self.spacing(document_gap);
        } else {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                "No local man page was found; showing the cached tldr quick reference.",
                theme::style(theme::StyleRole::Notice),
            ));
        }
    }
}
