//! IR to logical terminal content and anchors.
use super::StyledInlineLine;
use super::{
    Arc, Block, DocumentAddress, ExternalUri, HashMap, Inline, LineSurface, LinkTarget, ListKind,
    LogicalLine, LogicalLinkRange, LogicalTableCell, LogicalTableLayout, Modifier, NavKind,
    NavNode, Section, SemanticIndex, Span, Style, TLDR_ID, TLDR_VERTICAL_PADDING_ROWS,
    TldrDocument, UnicodeWidthStr, WrapMode, inline_anchor_ids, shifted_links, spans_width,
    styled_inline_lines, theme, tldr_style,
};
pub(super) struct DocumentBuilder {
    pub(super) label: String,
    pub(super) address: Option<DocumentAddress>,
    pub(super) lines: Vec<LogicalLine>,
    pub(super) navigation: Vec<NavNode>,
    pub(super) anchors: HashMap<String, usize>,
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

impl DocumentBuilder {
    pub(super) fn finish(self) -> BuiltDocument {
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
            label,
            address,
            lines: Vec::new(),
            navigation: Vec::new(),
            anchors: HashMap::new(),
        }
    }

    pub(super) fn push(&mut self, line: LogicalLine) {
        self.lines.push(line);
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
        for line in crate::tldr::layout_tldr(tldr) {
            let command = line.spans.iter().any(|span| {
                matches!(
                    span.role,
                    crate::tldr::TldrRole::Command | crate::tldr::TldrRole::Placeholder
                )
            });
            let links = line
                .spans
                .iter()
                .filter(|span| span.role == crate::tldr::TldrRole::Link)
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
            title: section.title.clone(),
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
        self.push(LogicalLine::plain(
            depth * 4,
            section.title.clone(),
            Style::default()
                .fg(theme::HEADING)
                .add_modifier(Modifier::BOLD),
        ));
        self.blocks(&section.blocks, depth * 4 + 3);
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

    pub(super) fn blocks(&mut self, blocks: &[Block], base_indent: usize) {
        for block in blocks {
            self.block(block, base_indent);
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn block(&mut self, block: &Block, base_indent: usize) {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                self.spacing(layout.spacing_before_lines);
                self.inline_lines(
                    children,
                    base_indent + usize::from(layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                );
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                self.spacing(layout.spacing_before_lines);
                self.inline_lines_with_surface(
                    children,
                    base_indent + usize::from(layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                    LineSurface::Code,
                );
            }
            Block::List {
                kind,
                start,
                compact,
                items,
                layout,
                ..
            } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 && !compact {
                        self.spacing(1);
                    }
                    let marker = match kind {
                        ListKind::Bullet => "• ".to_owned(),
                        ListKind::Ordered => format!(
                            "{}. ",
                            start
                                .unwrap_or(1)
                                .saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
                        ),
                        ListKind::Plain => String::new(),
                    };
                    let has_marker = !marker.is_empty();
                    if has_marker
                        && let Some(Block::Paragraph {
                            children, layout, ..
                        }) = item.blocks.first()
                    {
                        self.spacing(layout.spacing_before_lines);
                        let marker_width = UnicodeWidthStr::width(marker.as_str());
                        let content_indent =
                            indent + marker_width + usize::from(layout.indent_columns);
                        let mut inline_lines = styled_inline_lines(
                            children,
                            Style::default().fg(theme::TEXT),
                            self.address.as_ref(),
                        );
                        let first = inline_lines
                            .first_mut()
                            .map_or_else(StyledInlineLine::default, std::mem::take);
                        let mut spans =
                            vec![Span::styled(marker, Style::default().fg(theme::HEADING))];
                        spans.push(Span::raw(" ".repeat(usize::from(layout.indent_columns))));
                        spans.extend(first.spans);
                        self.push(
                            LogicalLine::hanging(indent, content_indent, spans).with_links(
                                shifted_links(first.links, content_indent.saturating_sub(indent)),
                            ),
                        );
                        for line in inline_lines.into_iter().skip(1) {
                            self.push(
                                LogicalLine::hanging(content_indent, content_indent, line.spans)
                                    .with_links(line.links),
                            );
                        }
                        self.blocks(&item.blocks[1..], indent + marker_width);
                    } else {
                        if has_marker {
                            self.push(LogicalLine::plain(
                                indent,
                                marker,
                                Style::default().fg(theme::HEADING),
                            ));
                        }
                        self.blocks(&item.blocks, indent + usize::from(has_marker) * 2);
                    }
                }
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    let spacing = item
                        .spacing_before_lines
                        .unwrap_or(u16::from(index > 0 && !compact));
                    self.spacing(spacing);
                    if let Some(identity) = &item.identity {
                        self.anchors
                            .insert(identity.id.to_string(), self.lines.len());
                    }
                    if item.inline_term {
                        self.inline_definition(item, indent);
                    } else {
                        for term in &item.terms {
                            self.inline_lines(
                                term,
                                indent,
                                Style::default().fg(theme::SUBTEXT_BRIGHT),
                            );
                        }
                        self.blocks(&item.description, indent + 4);
                    }
                }
            }
            Block::Table { rows, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                let grid = mant_ir::TableGrid::new(rows);
                let rows = (0..grid.rows.len())
                    .map(|row| {
                        grid.slots(row, 256)
                            .unwrap_or_else(|| {
                                grid.rows[row]
                                    .iter()
                                    .map(|positioned| Some(positioned.cell))
                                    .collect()
                            })
                            .into_iter()
                            .map(|cell| {
                                let mut builder = Self::new(String::new(), self.address.clone());
                                if let Some(cell) = cell {
                                    builder.blocks(&cell.blocks, 0);
                                }
                                let content = builder.finish().content;
                                let mut rendered = LogicalTableCell::new(
                                    content.lines,
                                    cell.and_then(|cell| cell.alignment),
                                );
                                rendered.anchors = content.anchors;
                                rendered
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let mut table_layout = LogicalTableLayout::for_rows(&rows);
                table_layout.force_stack = grid.column_count > 256;
                let table_layout = Arc::new(table_layout);
                for cells in rows {
                    self.push(LogicalLine::table(indent, cells, Arc::clone(&table_layout)));
                }
            }
            Block::Equation { value, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                self.push(
                    LogicalLine::plain(
                        base_indent + usize::from(layout.indent_columns),
                        value.clone(),
                        Style::default().fg(theme::YELLOW),
                    )
                    .wrap_mode(WrapMode::Character),
                );
            }
            Block::VerticalSpace { lines, .. } => self.spacing(*lines),
            Block::ThematicBreak { .. } => self.push(LogicalLine::rule(base_indent)),
            Block::Unsupported { text, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                self.push(LogicalLine::plain(
                    base_indent + usize::from(layout.indent_columns),
                    text.clone(),
                    Style::default().fg(theme::PEACH),
                ));
            }
        }
    }

    pub(super) fn spacing(&mut self, lines: u16) {
        for _ in 0..lines {
            self.lines.push(LogicalLine::empty());
        }
    }

    pub(super) fn inline_definition(&mut self, item: &mant_ir::DefinitionItem, indent: usize) {
        let mut term_spans = Vec::new();
        let mut term_links = Vec::new();
        for (index, term) in item.terms.iter().enumerate() {
            for line in styled_inline_lines(
                term,
                Style::default().fg(theme::SUBTEXT_BRIGHT),
                self.address.as_ref(),
            ) {
                let offset = spans_width(&term_spans);
                term_links.extend(shifted_links(line.links, offset));
                term_spans.extend(line.spans);
            }
            term_spans.push(Span::styled(
                if index + 1 < item.terms.len() {
                    ", "
                } else {
                    " "
                },
                Style::default().fg(theme::SUBTEXT_BRIGHT),
            ));
        }
        let term_width = spans_width(&term_spans);

        if let Some(Block::Paragraph {
            children, layout, ..
        }) = item.description.first()
            && layout.spacing_before_lines == 0
        {
            let description_indent = indent + term_width + usize::from(layout.indent_columns);
            term_spans.push(Span::raw(" ".repeat(usize::from(layout.indent_columns))));
            let mut description_lines = styled_inline_lines(
                children,
                Style::default().fg(theme::TEXT),
                self.address.as_ref(),
            );
            let first = description_lines
                .first_mut()
                .map_or_else(StyledInlineLine::default, std::mem::take);
            let description_offset = spans_width(&term_spans);
            term_links.extend(shifted_links(first.links, description_offset));
            term_spans.extend(first.spans);
            self.push(
                LogicalLine::hanging(indent, description_indent, term_spans).with_links(term_links),
            );
            for line in description_lines.into_iter().skip(1) {
                self.push(
                    LogicalLine::hanging(description_indent, description_indent, line.spans)
                        .with_links(line.links),
                );
            }
            self.blocks(&item.description[1..], description_indent);
        } else {
            self.push(LogicalLine::hanging(indent, indent, term_spans).with_links(term_links));
            self.blocks(&item.description, indent + term_width);
        }
    }

    pub(super) fn inline_lines(&mut self, nodes: &[Inline], indent: usize, base_style: Style) {
        self.inline_lines_with_surface(nodes, indent, base_style, LineSurface::Normal);
    }

    pub(super) fn inline_lines_with_surface(
        &mut self,
        nodes: &[Inline],
        indent: usize,
        base_style: Style,
        surface: LineSurface,
    ) {
        for id in inline_anchor_ids(nodes) {
            self.anchors.entry(id).or_insert(self.lines.len());
        }
        let lines = styled_inline_lines(nodes, base_style, self.address.as_ref())
            .into_iter()
            .map(|line| {
                let spans = if surface == LineSurface::Code {
                    crate::code::highlight(line.spans)
                } else {
                    line.spans
                };
                LogicalLine {
                    indent,
                    continuation_indent: indent,
                    spans,
                    surface,
                    wrap_mode: if surface == LineSurface::Code {
                        WrapMode::Character
                    } else {
                        WrapMode::Word
                    },
                    table_row: None,
                    links: line.links,
                }
            })
            .collect::<Vec<_>>();

        for line in lines {
            self.push(line);
        }
    }
}
