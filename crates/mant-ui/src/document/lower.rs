//! IR to logical terminal content and anchors.
use super::inline::styled_reference_inline_lines;
use super::{
    Arc, Block, DocumentAddress, ExternalUri, HashMap, Inline, LineSurface, LinkTarget,
    LogicalLine, LogicalLinkRange, Modifier, NavKind, NavNode, Section, SemanticIndex, Span, Style,
    TLDR_ID, TLDR_VERTICAL_PADDING_ROWS, TldrDocument, WrapMode, inline_anchor_rows, theme,
    tldr_style,
};
use mant_ir::geometry::{compose_origin, coordinate, padding};
use mant_ir::{
    ContentContext, ContentPointKey, ContentRootKey, DisplayPoint, FixedBody, FixedLineKey,
    FixedSectionReader, PlacementTarget,
};
use std::num::NonZeroU32;

mod fixed;
mod lists;
mod table;

fn insert_fixed_anchor(
    anchors: &mut HashMap<String, usize>,
    columns: &mut HashMap<String, usize>,
    fixed: &FixedBody,
    first_row: usize,
    id: &str,
    point: DisplayPoint,
) {
    let location = match point {
        DisplayPoint::RunBoundary { run, byte } => {
            let Some(record) = fixed.surface.runs.get((run.get() - 1) as usize) else {
                return;
            };
            let Some(prefix) = fixed
                .surface
                .run_text(run)
                .and_then(|text| usize::try_from(byte).ok().and_then(|end| text.get(..end)))
            else {
                return;
            };
            let column = mant_render::cells::graphemes(prefix)
                .map(|grapheme| grapheme.columns())
                .sum::<usize>();
            (
                first_row + (record.row.get() - 1) as usize,
                record.column as usize + column,
            )
        }
        DisplayPoint::RowColumn { row, column } => {
            (first_row + (row.get() - 1) as usize, column as usize)
        }
        DisplayPoint::DocumentEnd { row_count } => (first_row + row_count as usize, 0),
    };
    anchors.entry(id.to_owned()).or_insert(location.0);
    columns.entry(id.to_owned()).or_insert(location.1);
}

pub(super) struct DocumentBuilder<'a> {
    pub(super) content: Option<ContentContext<'a>>,
    pub(super) entry_styles: Option<Arc<mant_render::EntryStyleMap<'a>>>,
    pub(super) label: String,
    pub(super) address: Option<DocumentAddress>,
    pub(super) lines: Vec<LogicalLine>,
    pub(super) navigation: Vec<NavNode>,
    pub(super) anchors: HashMap<String, usize>,
    pub(super) reference_origins: Arc<super::references::ReferenceOrigins>,
    pub(super) fixed_reference_origins: HashMap<NonZeroU32, Arc<str>>,
    pub(super) link_targets: HashMap<super::LinkIdentity, LinkTarget>,
    pub(super) fixed_anchor_columns: HashMap<String, usize>,
    fixed_line_rows: HashMap<FixedLineKey, usize>,
    fixed_point_locations: HashMap<ContentPointKey, (usize, usize)>,
    stacked_point_locations: HashMap<ContentPointKey, (usize, usize)>,
    fixed_search_records: std::collections::BTreeMap<ContentRootKey, FixedSearchRecordBuilder>,
    pending_anchors: Vec<String>,
    pending_gap: mant_ir::geometry::GapPlan,
}

struct FixedSearchRecordBuilder {
    record: super::search::RenderedSearchRecord,
    byte_offsets: Vec<usize>,
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
    pub(super) link_targets: HashMap<super::LinkIdentity, LinkTarget>,
    pub(super) fixed_anchor_columns: HashMap<String, usize>,
    pub(super) fixed_search_records: Vec<super::search::RenderedSearchRecord>,
    pub(super) fixed_line_rows: HashMap<FixedLineKey, usize>,
    pub(super) fixed_point_locations: HashMap<ContentPointKey, (usize, usize)>,
    pub(super) stacked_point_locations: HashMap<ContentPointKey, (usize, usize)>,
}

impl<'a> DocumentBuilder<'a> {
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
            link_targets: self.link_targets,
            fixed_anchor_columns: self.fixed_anchor_columns,
            fixed_line_rows: self.fixed_line_rows,
            fixed_point_locations: self.fixed_point_locations,
            stacked_point_locations: self.stacked_point_locations,
            fixed_search_records: self
                .fixed_search_records
                .into_values()
                .map(|mut entry| {
                    entry.record.cells.sort_by_key(|cell| {
                        (
                            cell.fragment.row,
                            cell.fragment.start_column,
                            cell.source_start,
                        )
                    });
                    entry.record
                })
                .collect(),
        }
    }
    pub(super) fn new(
        label: String,
        address: Option<DocumentAddress>,
        content: Option<ContentContext<'a>>,
    ) -> Self {
        Self {
            content,
            entry_styles: None,
            label,
            address,
            lines: Vec::new(),
            navigation: Vec::new(),
            anchors: HashMap::new(),
            reference_origins: Arc::default(),
            fixed_reference_origins: HashMap::new(),
            link_targets: HashMap::new(),
            fixed_anchor_columns: HashMap::new(),
            fixed_line_rows: HashMap::new(),
            fixed_point_locations: HashMap::new(),
            stacked_point_locations: HashMap::new(),
            fixed_search_records: std::collections::BTreeMap::new(),
            pending_anchors: Vec::new(),
            pending_gap: mant_ir::geometry::GapPlan::default(),
        }
    }

    pub(super) fn push(&mut self, line: LogicalLine) {
        self.resolve_pending_anchors();
        self.pending_gap = mant_ir::geometry::GapPlan::default();
        self.lines.push(line);
    }

    /// Read final native rows and locate their marks on the same immutable surface.
    #[allow(clippy::too_many_lines)] // One final surface and its marks share the same row map.
    pub(super) fn native_fixed_rows(&mut self, fixed: &FixedBody, index: &SemanticIndex) {
        let first_row = self.lines.len();
        let mut run_scalars: HashMap<NonZeroU32, (usize, usize)> = HashMap::new();
        for row in &fixed.surface.rows {
            let first = (row.first_run.get() - 1) as usize;
            let end = first + row.run_count as usize;
            let mut spans = Vec::new();
            let mut column = 0;
            let mut scalar = 0;
            for run in &fixed.surface.runs[first..end] {
                if run.column > column {
                    spans.push(Span::raw(" ".repeat((run.column - column) as usize)));
                    scalar += (run.column - column) as usize;
                }
                let value = fixed
                    .surface
                    .run_text(run.key)
                    .expect("validated Fixed run");
                run_scalars.insert(run.key, (self.lines.len(), scalar));
                scalar += value.chars().count();
                let mut style = Style::default().fg(theme::TEXT);
                if run.label.style.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if run.label.style.underline {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if run.label.link.is_some() {
                    style = style.fg(theme::LINK).add_modifier(Modifier::UNDERLINED);
                }
                spans.push(Span::styled(value.to_owned(), style));
                column = run.column + run.width;
            }
            if row.column_count > column {
                spans.push(Span::raw(" ".repeat((row.column_count - column) as usize)));
            }
            self.push(LogicalLine {
                indent: 0,
                continuation_indent: 0,
                spans,
                glyph_projections: Vec::new(),
                surface: LineSurface::Fixed,
                wrap_mode: WrapMode::NoWrap,
                table_row: None,
                links: Vec::new(),
                reference_marks: Vec::new(),
            });
        }

        if let Some(first) = fixed.surface.rows.first() {
            insert_fixed_anchor(
                &mut self.anchors,
                &mut self.fixed_anchor_columns,
                fixed,
                first_row,
                super::ROOT_ID,
                DisplayPoint::RowColumn {
                    row: first.key,
                    column: 0,
                },
            );
        } else {
            insert_fixed_anchor(
                &mut self.anchors,
                &mut self.fixed_anchor_columns,
                fixed,
                first_row,
                super::ROOT_ID,
                DisplayPoint::DocumentEnd { row_count: 0 },
            );
        }
        if let Ok(reader) = FixedSectionReader::new(fixed) {
            let has_preface = reader
                .root_preface_parts()
                .is_ok_and(|parts| !parts.is_empty());
            if has_preface || !fixed.anchors.is_empty() || !index.root().is_empty() {
                self.navigation.push(NavNode {
                    id: super::ROOT_ID.to_owned(),
                    target_id: super::ROOT_ID.to_owned(),
                    title: "OVERVIEW".to_owned(),
                    full_title: None,
                    depth: 0,
                    kind: NavKind::Root,
                    has_children: !index.root().is_empty(),
                    is_last: reader.roots().is_empty(),
                    parent_id: None,
                });
                self.entry_group(
                    super::ROOT_ID,
                    super::ROOT_ID,
                    index.root(),
                    1,
                    reader.roots().is_empty(),
                );
            }
            for heading in &fixed.headings {
                let id = heading.id.as_str();
                insert_fixed_anchor(
                    &mut self.anchors,
                    &mut self.fixed_anchor_columns,
                    fixed,
                    first_row,
                    id,
                    heading.at,
                );
                for alias in &heading.rendered_fragment_aliases {
                    insert_fixed_anchor(
                        &mut self.anchors,
                        &mut self.fixed_anchor_columns,
                        fixed,
                        first_row,
                        alias.as_str(),
                        heading.at,
                    );
                }
                let depth = reader
                    .breadcrumbs(heading.key)
                    .map_or(0, |chain| chain.len() - 1);
                let parent_id = heading
                    .parent
                    .and_then(|key| reader.heading(key))
                    .map(|parent| parent.id.to_string());
                let siblings = heading
                    .parent
                    .and_then(|parent| reader.children(parent))
                    .unwrap_or_else(|| reader.roots());
                self.navigation.push(NavNode {
                    id: id.to_owned(),
                    target_id: id.to_owned(),
                    title: reader
                        .label(heading.key)
                        .filter(|label| !label.is_empty())
                        .unwrap_or_else(|| format!("SECTION {}", heading.key)),
                    full_title: None,
                    depth,
                    kind: NavKind::Section,
                    has_children: !index.section(id).is_empty()
                        || reader
                            .children(heading.key)
                            .is_some_and(|children| !children.is_empty()),
                    is_last: siblings.last() == Some(&heading.key),
                    parent_id,
                });
                self.entry_group(
                    id,
                    id,
                    index.section(id),
                    depth + 1,
                    reader
                        .children(heading.key)
                        .is_none_or(<[std::num::NonZeroU32]>::is_empty),
                );
            }
        }
        for owner in &fixed.owners {
            let at = owner
                .head
                .parts
                .first()
                .map(|slice| DisplayPoint::RunBoundary {
                    run: slice.run,
                    byte: slice.start_byte,
                })
                .or(owner.empty_point);
            if let Some(at) = at {
                insert_fixed_anchor(
                    &mut self.anchors,
                    &mut self.fixed_anchor_columns,
                    fixed,
                    first_row,
                    owner.id.as_str(),
                    at,
                );
            }
        }
        for anchor in &fixed.anchors {
            insert_fixed_anchor(
                &mut self.anchors,
                &mut self.fixed_anchor_columns,
                fixed,
                first_row,
                anchor.id.as_str(),
                anchor.at,
            );
            insert_fixed_anchor(
                &mut self.anchors,
                &mut self.fixed_anchor_columns,
                fixed,
                first_row,
                anchor.rendered_fragment.as_str(),
                anchor.at,
            );
        }
        for link in &fixed.links {
            if let (Some(id), Some(first)) = (
                self.fixed_reference_origins.get(&link.key),
                link.label.parts.first(),
            ) {
                insert_fixed_anchor(
                    &mut self.anchors,
                    &mut self.fixed_anchor_columns,
                    fixed,
                    first_row,
                    id,
                    DisplayPoint::RunBoundary {
                        run: first.run,
                        byte: first.start_byte,
                    },
                );
            }
            let Some(target) = link
                .target
                .as_ref()
                .and_then(|target| super::inline::local_link_target(target, self.address.as_ref()))
            else {
                continue;
            };
            let identity = super::LinkIdentity::NativeFixed(link.key);
            self.link_targets.insert(identity, target);
            for slice in &link.label.parts {
                let Some(&(row, first_scalar)) = run_scalars.get(&slice.run) else {
                    continue;
                };
                let Some(text) = fixed.surface.run_text(slice.run) else {
                    continue;
                };
                let (Ok(start), Ok(end)) = (
                    usize::try_from(slice.start_byte),
                    usize::try_from(slice.end_byte),
                ) else {
                    continue;
                };
                let Some(prefix) = text.get(..start) else {
                    continue;
                };
                let Some(fragment) = text.get(start..end) else {
                    continue;
                };
                let start_scalar = first_scalar + prefix.chars().count();
                let end_scalar = start_scalar + fragment.chars().count();
                if start_scalar < end_scalar {
                    self.lines[row].links.push(LogicalLinkRange {
                        identity,
                        start_scalar,
                        end_scalar,
                    });
                }
            }
        }
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
        let information_link = tldr.more_information.as_deref().and_then(|value| {
            let target = ExternalUri::parse(value).map(LinkTarget::External)?;
            let identity = super::LinkIdentity::TldrMoreInformation;
            self.link_targets.insert(identity, target);
            Some(identity)
        });
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
                    information_link.map(|identity| LogicalLinkRange {
                        identity,
                        start_scalar,
                        end_scalar,
                    })
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
                glyph_projections: Vec::new(),
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

    pub(super) fn section_with_position<'b>(
        &mut self,
        section: &'b Section,
        semantic_index: &SemanticIndex,
        depth: usize,
        is_last: bool,
        parent_id: Option<&str>,
    ) where
        'a: 'b,
    {
        self.spacing(section.spacing_before_lines);
        let entries = semantic_index.section(&section.id);
        let has_children = !entries.is_empty() || !section.children.is_empty();
        self.anchor(NavNode {
            id: section.id.to_string(),
            target_id: section.id.to_string(),
            title: self
                .content()
                .heading_single_line_text(&section.heading)
                .expect("validated document heading must resolve"),
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
    pub(super) fn heading<'b>(&mut self, heading: &'b mant_ir::Heading, indent: i32)
    where
        'a: 'b,
    {
        self.inline_lines(
            &heading.content,
            indent,
            Style::default()
                .fg(theme::HEADING)
                .add_modifier(Modifier::BOLD),
        );
    }

    pub(super) fn blocks<'b>(&mut self, blocks: &'b [Block], base_indent: i32)
    where
        'a: 'b,
    {
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

    pub(super) fn block<'b>(&mut self, block: &'b Block, base_indent: i32)
    where
        'a: 'b,
    {
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
            Block::FixedDisplay { children, view, .. } => {
                self.styled_inlines(children, Style::default().fg(theme::TEXT));
                let mut anchors = HashMap::new();
                fixed::collect_inline_anchors(children, &mut anchors);
                self.fixed_view(*view, &anchors);
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
            Block::Table {
                rows,
                fixed_view,
                layout,
                ..
            } => {
                if let Some(view) = fixed_view {
                    let mut anchors = HashMap::new();
                    for cell in rows.iter().flat_map(|row| &row.cells) {
                        fixed::collect_block_anchors(&cell.blocks, &mut anchors);
                    }
                    self.fixed_view(*view, &anchors);
                } else {
                    self.table(rows, compose_origin(base_indent, layout.indent_columns));
                }
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

    #[allow(clippy::too_many_lines)] // Geometry, links, zero-width points and logical search share one physical-row walk.
    fn fixed_view(
        &mut self,
        key: mant_ir::FixedViewKey,
        anchors: &HashMap<ContentPointKey, Vec<String>>,
    ) {
        let view = self
            .content()
            .fixed_view(key)
            .expect("validated fixed display must resolve");
        let physical_lines = view
            .physical_lines(self.content())
            .expect("validated fixed display must materialize");
        for (line, geometry) in physical_lines.into_iter().zip(&view.lines) {
            let logical_row = self.lines.len();
            self.fixed_line_rows.insert(geometry.key, logical_row);
            for placement in &geometry.placements {
                if let PlacementTarget::Point(point) = placement.target {
                    self.fixed_point_locations.entry(point).or_insert((
                        logical_row,
                        usize::try_from(placement.start_column).expect("validated column"),
                    ));
                }
            }
            let base_style = Style::default().fg(theme::TEXT);
            let mut column_styles = vec![
                base_style;
                usize::try_from(geometry.terminal_columns)
                    .expect("validated width")
            ];
            for placement in &geometry.placements {
                let PlacementTarget::Content(reference) = placement.target else {
                    continue;
                };
                let atom = self.content().atom(reference.atom).expect("validated atom");
                if let Some(occurrence) = atom.link
                    && let Some(target) = self.content().occurrence(occurrence)
                    && let Some(target) =
                        super::inline::local_link_target(&target.target, self.address.as_ref())
                {
                    self.link_targets
                        .entry(super::LinkIdentity::Content(occurrence))
                        .or_insert(target);
                }
                let style = fixed::atom_style(self.content(), atom);
                let start = usize::try_from(placement.start_column).expect("validated column");
                let end = usize::try_from(placement.end_column).expect("validated column");
                column_styles[start..end].fill(style);
            }
            for decoration in &geometry.decorations {
                let style = if matches!(
                    decoration.kind,
                    mant_ir::DecorationKind::Border | mant_ir::DecorationKind::Rule
                ) {
                    Style::default().fg(theme::OVERLAY)
                } else {
                    base_style
                };
                let start = usize::try_from(decoration.start_column).expect("validated column");
                let end =
                    start + usize::try_from(decoration.width_columns).expect("validated width");
                column_styles[start..end].fill(style);
            }
            let mut column_scalars =
                vec![
                    0_usize;
                    usize::try_from(geometry.terminal_columns).expect("validated width") + 1
                ];
            let mut column = 0_usize;
            let mut scalar = 0_usize;
            let mut styled_spans = Vec::new();
            let mut current_style = base_style;
            let mut current_text = String::new();
            for grapheme in mant_render::cells::graphemes(&line) {
                let width = grapheme.columns();
                let style = column_styles.get(column).copied().unwrap_or(base_style);
                if style != current_style && !current_text.is_empty() {
                    styled_spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style,
                    ));
                }
                current_style = style;
                current_text.push_str(grapheme.text());
                for value in &mut column_scalars[column..column + width] {
                    *value = scalar;
                }
                column += width;
                scalar += grapheme.text().chars().count();
                column_scalars[column] = scalar;
            }
            if !current_text.is_empty() {
                styled_spans.push(Span::styled(current_text, current_style));
            }
            let links = geometry
                .placements
                .iter()
                .filter_map(|placement| {
                    let PlacementTarget::Content(reference) = placement.target else {
                        return None;
                    };
                    let occurrence = self.content().atom(reference.atom)?.link?;
                    let start = usize::try_from(placement.start_column).ok()?;
                    let end = usize::try_from(placement.end_column).ok()?;
                    Some(LogicalLinkRange {
                        identity: super::LinkIdentity::Content(occurrence),
                        start_scalar: *column_scalars.get(start)?,
                        end_scalar: *column_scalars.get(end)?,
                    })
                })
                .collect();
            let reference_marks =
                geometry
                    .placements
                    .iter()
                    .filter_map(|placement| {
                        let PlacementTarget::Point(point) = placement.target else {
                            return None;
                        };
                        let scalar_offset =
                            *column_scalars.get(usize::try_from(placement.start_column).ok()?)?;
                        Some(anchors.get(&point)?.iter().map(move |id| {
                            super::model::ReferenceMark {
                                id: Arc::from(id.as_str()),
                                scalar_offset,
                            }
                        }))
                    })
                    .flatten()
                    .collect();
            let content = self.content();
            for placement in &geometry.placements {
                let PlacementTarget::Content(reference) = placement.target else {
                    continue;
                };
                let atom = content
                    .atom(reference.atom)
                    .expect("validated fixed placement atom");
                let entry = self
                    .fixed_search_records
                    .entry(atom.root)
                    .or_insert_with(|| {
                        let text = content
                            .root_logical_text(atom.root)
                            .expect("validated fixed placement root");
                        let mut byte_offsets = text
                            .char_indices()
                            .map(|(byte, _)| byte)
                            .collect::<Vec<_>>();
                        byte_offsets.push(text.len());
                        FixedSearchRecordBuilder {
                            record: super::search::RenderedSearchRecord {
                                text,
                                cells: Vec::new(),
                            },
                            byte_offsets,
                        }
                    });
                for root_scalar in placement.root_scalar_range.clone() {
                    let scalar_index =
                        usize::try_from(root_scalar).expect("validated scalar range");
                    let (start_column, end_column) = match placement.map {
                        mant_ir::CellMapKind::Affine { columns_per_scalar } => {
                            let relative = root_scalar - placement.root_scalar_range.start;
                            let start =
                                placement.start_column + relative * u32::from(columns_per_scalar);
                            (start, start + u32::from(columns_per_scalar))
                        }
                        mant_ir::CellMapKind::GraphemeCluster {}
                        | mant_ir::CellMapKind::Overlay {} => {
                            (placement.start_column, placement.end_column)
                        }
                    };
                    entry
                        .record
                        .cells
                        .push(super::search::RenderedSearchSourceCell {
                            source_start: entry.byte_offsets[scalar_index],
                            source_end: entry.byte_offsets[scalar_index + 1],
                            fragment: super::search::RenderedSearchFragment {
                                row: logical_row,
                                start_column: usize::try_from(start_column)
                                    .expect("validated column"),
                                end_column: usize::try_from(end_column).expect("validated column"),
                            },
                        });
                }
            }
            let mut visible = LogicalLine::plain(0, line, base_style)
                .surface(LineSurface::Fixed)
                .wrap_mode(WrapMode::NoWrap)
                .with_links(links)
                .with_reference_marks(reference_marks);
            visible.spans = styled_spans;
            self.push(visible);
        }
    }

    pub(super) fn spacing(&mut self, lines: u16) {
        let before = self.pending_gap.rows(0);
        self.pending_gap.append_resolved(lines);
        for _ in before..self.pending_gap.rows(0) {
            self.lines.push(LogicalLine::empty());
        }
    }

    pub(super) fn inline_lines<'b>(&mut self, nodes: &'b [Inline], indent: i32, base_style: Style)
    where
        'a: 'b,
    {
        self.inline_lines_with_surface(nodes, indent, base_style, LineSurface::Normal);
    }

    fn styled_inlines<'b>(
        &mut self,
        nodes: &'b [Inline],
        style: Style,
    ) -> Vec<super::StyledInlineLine>
    where
        'a: 'b,
    {
        styled_reference_inline_lines(
            self.content(),
            nodes,
            style,
            self.address.as_ref(),
            self.entry_styles
                .as_ref()
                .map_or(&[][..], |styles| styles.ranges(nodes)),
            false,
            &self.reference_origins,
            &mut self.link_targets,
        )
    }

    pub(super) fn inline_lines_with_surface<'b>(
        &mut self,
        nodes: &'b [Inline],
        indent: i32,
        base_style: Style,
        surface: LineSurface,
    ) where
        'a: 'b,
    {
        let content = self.content();
        let targets = inline_anchor_rows(content, nodes);
        let lines = styled_reference_inline_lines(
            content,
            nodes,
            base_style,
            self.address.as_ref(),
            self.entry_styles
                .as_ref()
                .map_or(&[][..], |styles| styles.ranges(nodes)),
            surface == LineSurface::Code,
            &self.reference_origins,
            &mut self.link_targets,
        );
        if lines.len() == 1
            && lines[0].spans.is_empty()
            && !(surface == LineSurface::Code
                && content
                    .has_literal_rows(nodes)
                    .expect("validated document content must resolve"))
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
                glyph_projections: line.glyph_projections,
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

    pub(super) fn content(&self) -> ContentContext<'a> {
        #[cfg(test)]
        if self.content.is_none() {
            return crate::test_content::content();
        }
        self.content
            .expect("document lowering requires an authoritative content store")
    }
}
