//! IR to logical terminal content and anchors.
use super::StyledInlineLine;
use super::inline::{styled_bound_inline_lines, styled_display_inline_lines};
use super::{
    Arc, Block, DocumentAddress, ExternalUri, HashMap, Inline, LineSurface, LinkTarget, ListKind,
    LogicalLine, LogicalLinkRange, LogicalTableCell, LogicalTableLayout, Modifier, NavKind,
    NavNode, Section, SemanticIndex, Span, Style, TLDR_ID, TLDR_VERTICAL_PADDING_ROWS,
    TldrDocument, UnicodeWidthStr, WrapMode, inline_anchor_ids, shifted_links, spans_width, theme,
    tldr_style,
};
use mant_protocol::geometry::{compose_origin, coordinate, marker_run_in_gap, padding};
pub(super) struct DocumentBuilder<'a> {
    pub(super) entry_styles: Arc<mant_protocol::EntryStyleMap<'a>>,
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

impl DocumentBuilder<'_> {
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
            entry_styles: Arc::default(),
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

    pub(super) fn blocks(&mut self, blocks: &[Block], base_indent: i32) {
        for block in blocks {
            self.block(block, base_indent);
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn block(&mut self, block: &Block, base_indent: i32) {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                self.spacing(layout.spacing_before_lines);
                self.inline_lines(
                    children,
                    compose_origin(base_indent, layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                );
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                self.spacing(layout.spacing_before_lines);
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
                self.spacing(layout.spacing_before_lines);
                let indent = compose_origin(base_indent, layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 && !compact {
                        self.spacing(1);
                    }
                    let item_start = self.lines.len();
                    let marker = match kind {
                        ListKind::Bullet => "• ".to_owned(),
                        ListKind::Ordered { .. } => {
                            format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                        }
                        ListKind::Plain => String::new(),
                    };
                    let has_marker = !marker.is_empty();
                    let marker_width = mant_protocol::geometry::text_width(&marker);
                    if has_marker
                        && let Some(Block::Paragraph {
                            children, layout, ..
                        }) = item.blocks.first()
                        && let Some(gap) =
                            marker_run_in_gap(indent, marker_width, layout.indent_columns)
                    {
                        self.spacing(layout.spacing_before_lines);
                        let content_indent = compose_origin(
                            compose_origin(indent, coordinate(marker_width)),
                            layout.indent_columns,
                        );
                        let mut inline_lines = styled_bound_inline_lines(
                            children,
                            Style::default().fg(theme::TEXT),
                            self.address.as_ref(),
                            self.entry_styles.ranges(children),
                        );
                        let first = inline_lines
                            .first_mut()
                            .map_or_else(StyledInlineLine::default, std::mem::take);
                        let mut spans =
                            vec![Span::styled(marker, Style::default().fg(theme::HEADING))];
                        spans.push(Span::raw(" ".repeat(gap)));
                        spans.extend(first.spans);
                        self.push(
                            LogicalLine::hanging(padding(indent), padding(content_indent), spans)
                                .with_links(shifted_links(
                                    first.links,
                                    marker_width.saturating_add(gap),
                                )),
                        );
                        for line in inline_lines.into_iter().skip(1) {
                            self.push(
                                LogicalLine::hanging(
                                    padding(content_indent),
                                    padding(content_indent),
                                    line.spans,
                                )
                                .with_links(line.links),
                            );
                        }
                        self.blocks(
                            &item.blocks[1..],
                            compose_origin(indent, coordinate(marker_width)),
                        );
                    } else {
                        if has_marker {
                            self.push(LogicalLine::plain(
                                padding(indent),
                                marker,
                                Style::default().fg(theme::HEADING),
                            ));
                        }
                        self.blocks(
                            &item.blocks,
                            compose_origin(indent, coordinate(marker_width)),
                        );
                    }
                    if let Some(facts) = &item.entry {
                        // Leading spacing is presentation, not the semantic landing row.
                        let first_content = self.lines[item_start..]
                            .iter()
                            .position(|line| !line.spans.is_empty() || line.table_row.is_some())
                            .map_or(item_start, |offset| item_start + offset);
                        self.anchors.insert(facts.id.to_string(), first_content);
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
                let indent = compose_origin(base_indent, layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    let spacing = item
                        .layout
                        .spacing_before_lines
                        .unwrap_or(u16::from(index > 0 && !compact));
                    self.spacing(spacing);
                    if let Some(identity) = &item.entry {
                        self.anchors
                            .insert(identity.id.to_string(), self.lines.len());
                    }
                    if item.layout.inline_term {
                        self.inline_definition(item, indent);
                    } else {
                        for term in &item.terms {
                            self.inline_lines(term, indent, Style::default().fg(theme::TEXT));
                        }
                        self.blocks(
                            &item.description,
                            compose_origin(
                                indent,
                                i32::from(mant_ir::DefinitionItem::DESCRIPTION_INDENT_COLUMNS),
                            ),
                        );
                    }
                }
            }
            Block::Table { rows, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                let indent = compose_origin(base_indent, layout.indent_columns);
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
                                builder.entry_styles = Arc::clone(&self.entry_styles);
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
                    self.push(LogicalLine::table(
                        padding(indent),
                        cells,
                        Arc::clone(&table_layout),
                    ));
                }
            }
            Block::Equation { value, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
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
                self.spacing(layout.spacing_before_lines);
                self.push(LogicalLine::plain(
                    padding(compose_origin(base_indent, layout.indent_columns)),
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

    pub(super) fn inline_definition(&mut self, item: &mant_ir::DefinitionItem, indent: i32) {
        let mut head_lines = Vec::new();
        let mut head_targets = Vec::new();
        for term in &item.terms {
            for id in inline_anchor_ids(term) {
                head_targets.push((id, head_lines.len()));
            }
            let lines = styled_bound_inline_lines(
                term,
                Style::default().fg(theme::TEXT),
                self.address.as_ref(),
                self.entry_styles.ranges(term),
            );
            if lines.len() != 1 || !lines[0].spans.is_empty() {
                head_lines.extend(lines);
            }
        }
        // A trailing zero-width root shares the last head/body row when that
        // row runs in. Its provisional slot is not a new visible line.
        let last_target_row = if item.inline_description().is_some() {
            head_lines.len().saturating_sub(1)
        } else {
            head_lines.len()
        };
        for (id, row) in head_targets {
            self.anchors
                .entry(id)
                .or_insert(self.lines.len() + row.min(last_target_row));
        }
        let block_origin = compose_origin(
            indent,
            i32::from(mant_ir::DefinitionItem::DESCRIPTION_INDENT_COLUMNS),
        );
        let last = head_lines.pop().unwrap_or_default();
        for line in head_lines {
            self.push(
                LogicalLine::hanging(padding(indent), padding(indent), line.spans)
                    .with_links(line.links),
            );
        }
        let mut term_spans = last.spans;
        let mut term_links = last.links;
        let term_width = spans_width(&term_spans);
        if let Some((children, layout)) = item.inline_description() {
            let continuation_indent = compose_origin(block_origin, layout.indent_columns);
            let description_indent = continuation_indent.max(compose_origin(
                indent,
                coordinate(term_width.saturating_add(1)),
            ));
            term_spans.push(Span::raw(
                " ".repeat(
                    padding(description_indent)
                        .saturating_sub(padding(indent).saturating_add(term_width))
                        .max(1),
                ),
            ));
            let mut description_lines = styled_bound_inline_lines(
                children,
                Style::default().fg(theme::TEXT),
                self.address.as_ref(),
                self.entry_styles.ranges(children),
            );
            let first = description_lines
                .first_mut()
                .map_or_else(StyledInlineLine::default, std::mem::take);
            let description_offset = spans_width(&term_spans);
            term_links.extend(shifted_links(first.links, description_offset));
            term_spans.extend(first.spans);
            self.push(
                LogicalLine::hanging(padding(indent), padding(continuation_indent), term_spans)
                    .with_links(term_links),
            );
            for line in description_lines.into_iter().skip(1) {
                self.push(
                    LogicalLine::hanging(
                        padding(continuation_indent),
                        padding(continuation_indent),
                        line.spans,
                    )
                    .with_links(line.links),
                );
            }
            self.blocks(&item.description[1..], block_origin);
        } else {
            self.push(
                LogicalLine::hanging(padding(indent), padding(indent), term_spans)
                    .with_links(term_links),
            );
            self.blocks(&item.description, block_origin);
        }
    }

    pub(super) fn inline_lines(&mut self, nodes: &[Inline], indent: i32, base_style: Style) {
        self.inline_lines_with_surface(nodes, indent, base_style, LineSurface::Normal);
    }

    pub(super) fn inline_lines_with_surface(
        &mut self,
        nodes: &[Inline],
        indent: i32,
        base_style: Style,
        surface: LineSurface,
    ) {
        for id in inline_anchor_ids(nodes) {
            self.anchors.entry(id).or_insert(self.lines.len());
        }
        let lines = styled_display_inline_lines(
            nodes,
            base_style,
            self.address.as_ref(),
            self.entry_styles.ranges(nodes),
            surface == LineSurface::Code,
        );
        if lines.len() == 1 && lines[0].spans.is_empty() {
            return;
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
            })
            .collect::<Vec<_>>();

        for line in lines {
            self.push(line);
        }
    }
}
