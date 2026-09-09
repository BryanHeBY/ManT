//! IR to logical terminal content and anchors.
use super::inline::styled_reference_inline_lines;
use super::{
    Arc, Block, DocumentAddress, ExternalUri, HashMap, Inline, LineSurface, LinkTarget,
    LogicalLine, LogicalLinkRange, Modifier, NavKind, NavNode, Section, SemanticIndex, Span, Style,
    TLDR_ID, TLDR_VERTICAL_PADDING_ROWS, TldrDocument, UnicodeWidthStr, WrapMode,
    inline_anchor_rows, theme, tldr_style,
};
use mant_ir::geometry::{compose_origin, coordinate, padding};

mod lists;
mod table;
pub(super) struct DocumentBuilder<'a> {
    pub(super) entry_styles: Arc<mant_render::EntryStyleMap<'a>>,
    pub(super) label: String,
    pub(super) address: Option<DocumentAddress>,
    pub(super) lines: Vec<LogicalLine>,
    pub(super) navigation: Vec<NavNode>,
    pub(super) anchors: HashMap<String, usize>,
    pub(super) reference_origins: Arc<super::references::ReferenceOrigins>,
    pending_anchors: Vec<String>,
    pending_gap: mant_ir::geometry::GapPlan,
}

/// Logical payload and its anchors must cross layout boundaries together.
pub(super) struct LogicalFragment {
    pub(super) lines: Vec<LogicalLine>,
    pub(super) anchors: HashMap<String, usize>,
}

pub(super) struct BuiltDocument {
    pub(super) label: String,
    pub(super) navigation: Vec<NavNode>,
    pub(super) content: LogicalFragment,
}

impl DocumentBuilder<'_> {
    pub(super) fn finish(mut self) -> BuiltDocument {
        // No source content follows these targets. Keep the end-of-document
        // sentinel rather than inventing a visible row or landing in a gap.
        self.resolve_pending_anchors();
        BuiltDocument {
            label: self.label,
            navigation: self.navigation,
            content: LogicalFragment {
                lines: self.lines,
                anchors: self.anchors,
            },
        }
    }
    pub(super) fn new(label: String, address: Option<DocumentAddress>) -> Self {
        Self {
            entry_styles: Arc::default(),
            label,
            address,
            lines: Vec::new(),
            navigation: Vec::new(),
            anchors: HashMap::new(),
            reference_origins: Arc::default(),
            pending_anchors: Vec::new(),
            pending_gap: mant_ir::geometry::GapPlan::default(),
        }
    }

    pub(super) fn push(&mut self, line: LogicalLine) {
        self.resolve_pending_anchors();
        self.pending_gap = mant_ir::geometry::GapPlan::default();
        self.lines.push(line);
    }

    fn resolve_pending_anchors(&mut self) {
        for id in self.pending_anchors.drain(..) {
            self.anchors.entry(id).or_insert(self.lines.len());
        }
    }

    fn defer_anchors(&mut self, ids: impl IntoIterator<Item = String>) {
        self.pending_anchors.extend(ids);
    }

    pub(super) fn tldr(
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
                .filter(|span| span.role == mant_render::TldrRole::Link)
                .filter_map(|span| {
                    tldr.more_information
                        .as_deref()
                        .and_then(ExternalUri::parse)
                        .map(|uri| (span, uri))
                })
                .map(|(span, uri)| LogicalLinkRange {
                    target: LinkTarget::External(uri),
                    start_column: 0,
                    end_column: UnicodeWidthStr::width(span.text.as_str()),
                })
                .collect();
            self.push(LogicalLine {
                indent: line.indent,
                continuation_indent: line.indent,
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
                Style::default().fg(theme::SUBTEXT),
            ));
            self.spacing(document_gap);
        } else {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                "No local man page was found; showing the cached tldr quick reference.",
                Style::default().fg(theme::YELLOW),
            ));
        }
    }

    pub(super) fn anchor(&mut self, node: NavNode) {
        self.anchors
            .insert(node.target_id.clone(), self.lines.len());
        self.navigation(node);
    }

    pub(super) fn navigation(&mut self, node: NavNode) {
        self.navigation.push(node);
    }

    pub(super) fn section_with_position(
        &mut self,
        section: &Section,
        semantic_index: &SemanticIndex,
        depth: usize,
        is_last: bool,
        parent_id: Option<&str>,
    ) {
        self.spacing(section.spacing_before_lines);
        let entries = semantic_index.section(&section.id);
        let has_children = !entries.is_empty() || !section.children.is_empty();
        self.anchor(NavNode {
            id: section.id.to_string(),
            target_id: section.id.to_string(),
            title: section.heading.plain_text(),
            full_title: None,
            depth,
            kind: NavKind::Section,
            has_children,
            is_last,
            parent_id: parent_id.map(str::to_owned),
        });
        for alias in &section.fragment_aliases {
            self.anchors
                .entry(alias.to_string())
                .or_insert(self.lines.len());
        }
        self.entry_group(
            &section.id,
            &section.id,
            entries,
            depth + 1,
            section.children.is_empty(),
        );
        self.heading(&section.heading, coordinate(depth.saturating_mul(4)));
        self.blocks(
            &section.blocks,
            coordinate(depth.saturating_mul(4).saturating_add(3)),
        );
        let child_count = section.children.len();
        for (index, child) in section.children.iter().enumerate() {
            self.section_with_position(
                child,
                semantic_index,
                depth + 1,
                index + 1 == child_count,
                Some(&section.id),
            );
        }
    }

    /// Headings use the same original inline path as prose: links, anchors,
    /// hard lines and nested source styles must not pass through a plain label.
    pub(super) fn heading(&mut self, heading: &mant_ir::Heading, indent: i32) {
        self.inline_lines(
            &heading.content,
            indent,
            Style::default()
                .fg(theme::HEADING)
                .add_modifier(Modifier::BOLD),
        );
    }

    pub(super) fn blocks(&mut self, blocks: &[Block], base_indent: i32) {
        let mut gap = mant_ir::geometry::GapPlan::default();
        for block in blocks {
            gap.append_resolved(mant_ir::geometry::block_gap(block));
            if matches!(block, Block::VerticalSpace { .. }) {
                continue;
            }
            self.spacing(gap.rows(0));
            gap = mant_ir::geometry::GapPlan::default();
            self.block(block, base_indent);
        }
        self.spacing(gap.rows(0));
    }

    pub(super) fn block(&mut self, block: &Block, base_indent: i32) {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                let start = self.lines.len();
                self.inline_lines(
                    children,
                    compose_origin(base_indent, layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                );
                let continuation = padding(compose_origin(
                    compose_origin(base_indent, layout.indent_columns),
                    layout.continuation_indent_columns,
                ));
                for (index, line) in self.lines[start..].iter_mut().enumerate() {
                    line.continuation_indent = continuation;
                    if index > 0 {
                        line.indent = continuation;
                    }
                }
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                self.inline_lines_with_surface(
                    children,
                    compose_origin(base_indent, layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                    LineSurface::Code,
                );
            }
            Block::List {
                kind,
                compact,
                items,
                layout,
                ..
            } => {
                self.list(
                    *kind,
                    *compact,
                    items,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                self.definitions(
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::Table { rows, layout, .. } => {
                self.table(rows, compose_origin(base_indent, layout.indent_columns));
            }
            Block::Equation { value, layout, .. } => {
                self.push(
                    LogicalLine::plain(
                        padding(compose_origin(base_indent, layout.indent_columns)),
                        value.clone(),
                        Style::default().fg(theme::YELLOW),
                    )
                    .wrap_mode(WrapMode::Character),
                );
            }
            Block::VerticalSpace { lines, .. } => self.spacing(*lines),
            Block::ThematicBreak { .. } => self.push(LogicalLine::rule(padding(base_indent))),
            Block::Unsupported { text, layout, .. } => {
                self.push(LogicalLine::plain(
                    padding(compose_origin(base_indent, layout.indent_columns)),
                    text.clone(),
                    Style::default().fg(theme::PEACH),
                ));
            }
        }
    }

    pub(super) fn spacing(&mut self, lines: u16) {
        let before = self.pending_gap.rows(0);
        self.pending_gap.append_resolved(lines);
        for _ in before..self.pending_gap.rows(0) {
            self.lines.push(LogicalLine::empty());
        }
    }

    pub(super) fn inline_lines(&mut self, nodes: &[Inline], indent: i32, base_style: Style) {
        self.inline_lines_with_surface(nodes, indent, base_style, LineSurface::Normal);
    }

    fn styled_inlines(&self, nodes: &[Inline], style: Style) -> Vec<super::StyledInlineLine> {
        styled_reference_inline_lines(
            nodes,
            style,
            self.address.as_ref(),
            self.entry_styles.ranges(nodes),
            false,
            &self.reference_origins,
        )
    }

    pub(super) fn inline_lines_with_surface(
        &mut self,
        nodes: &[Inline],
        indent: i32,
        base_style: Style,
        surface: LineSurface,
    ) {
        let targets = inline_anchor_rows(nodes);
        let lines = styled_reference_inline_lines(
            nodes,
            base_style,
            self.address.as_ref(),
            self.entry_styles.ranges(nodes),
            surface == LineSurface::Code,
            &self.reference_origins,
        );
        if lines.len() == 1
            && lines[0].spans.is_empty()
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
            self.anchors.entry(id).or_insert(self.lines.len() + row);
        }
        let lines = lines
            .into_iter()
            .map(|line| LogicalLine {
                indent: padding(indent),
                continuation_indent: padding(indent),
                spans: line.spans,
                surface,
                wrap_mode: if surface == LineSurface::Code {
                    WrapMode::Character
                } else {
                    WrapMode::Word
                },
                table_row: None,
                links: line.links,
                reference_marks: line.reference_marks,
            })
            .collect::<Vec<_>>();

        for line in lines {
            self.push(line);
        }
    }
}
