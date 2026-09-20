//! Native-backed emitter for the sealed K23 projection arena.
//!
//! This layer translates already-owned execution facts. It never rescans
//! source coordinates to discover structure and never invokes the legacy
//! marker/rebind or geometry-driven nesting passes.

use std::collections::{BTreeSet, HashMap};

use libmandoc_rs::{
    BoundaryRequest, ExecutionBoundary, ExecutionEquationFlags, ExecutionManBlockKind,
    ExecutionMdocListKind, ExecutionNodeKey, NativeExecutionReport,
};
use mant_ir::{
    Block, ContentBlockStep, DeclarationGroup, DefinitionItem, DefinitionLayout, Inline,
    LayoutHint, ListItem, ListItemLayout, ListKind, SourceSpan, TableAlignment, TableCell,
    TableCellKind, TableRow, TableRowKind,
};

use super::frame::{NativeFrameError, NativeFramePlan};
use super::{
    Emission, InlineProduct, MaterializedChunk, MaterializedManContinuation, MaterializedManItem,
    MaterializedMdocItem, MaterializedTerm, NodeProduct, ProjectionNode, ProjectionNodeKind,
    RawRecordRef, RecipeEmitter, SpecializedEmission, SpecializedRecipe, StableSemanticItem,
    StableSemanticOwner, StreamEvent, StreamMode,
};
use crate::mandoc::native_execution::ownership::{
    CanonicalEffectKind, CanonicalEffectOutcome, ProjectionArena, ProjectionScopeKind,
};
use crate::mandoc::native_execution::semantics::man::{Ordinal, Presentation};
use crate::mandoc::native_execution::semantics::{
    FlowProjectionPolicy, InlineProjector, document_origin_columns,
    flow_blocks_by_fill_with_default, man_head_evidence, markup_evidence, paragraph,
    remove_structural_heading_bold, source_span,
};
use crate::mandoc::native_execution::{
    NativeAnchor, NativeManBlock, NativeMdocListItem, NativeProjection,
};

#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc::native_execution::semantics) enum NativeEmitterError {
    MissingHeading(u32),
    MissingTable(u32),
    MissingEquation(u32),
    MissingMdocItem(ExecutionNodeKey),
    MissingMdocList(ExecutionNodeKey),
    MissingManBlock(ExecutionNodeKey),
    MissingDefinitionLayout(ExecutionNodeKey),
    InvalidNodeKind,
    InvalidSpecializedRecipe(SpecializedRecipe),
    ContinuationBodyConflict(ExecutionNodeKey),
    StructureOverflow,
    Frame(NativeFrameError),
}

impl std::fmt::Display for NativeEmitterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for NativeEmitterError {}

pub(in crate::mandoc::native_execution::semantics) struct NativeRecipeEmitter<'a> {
    report: &'a NativeExecutionReport,
    projection: &'a NativeProjection,
    arena: &'a ProjectionArena,
    projector: &'a InlineProjector<'a>,
    heading_by_wrapper: HashMap<u32, usize>,
    anchors: HashMap<u32, &'a NativeAnchor>,
    mdoc_items: HashMap<ExecutionNodeKey, &'a NativeMdocListItem>,
    man_blocks: HashMap<ExecutionNodeKey, &'a NativeManBlock>,
    definition_layouts: HashMap<ExecutionNodeKey, DefinitionLayout>,
    frames: NativeFramePlan,
}

/// One source-ordered formatter transaction inside a projection stream.
///
/// Native anchors and placement observations are zero-width facts.  They do
/// not settle the formatter buffer and consequently must not split the atom
/// envelope into separate IR paragraphs.  Only an execution effect that
/// actually commits layout, or the end of the stream, commits this pending
/// transaction.
#[derive(Default)]
struct PendingStreamTransaction {
    atoms: Option<std::ops::Range<u32>>,
    anchors: Vec<u32>,
}

impl PendingStreamTransaction {
    fn extend(&mut self, atoms: &std::ops::Range<u32>) {
        if atoms.is_empty() {
            return;
        }
        match &mut self.atoms {
            Some(pending) => pending.end = pending.end.max(atoms.end),
            None => self.atoms = Some(atoms.clone()),
        }
    }

    fn anchor(&mut self, key: u32) {
        self.anchors.push(key);
    }
}

impl<'a> NativeRecipeEmitter<'a> {
    #[allow(clippy::too_many_arguments)]
    fn commit_pending_stream(
        &self,
        owner: &ProjectionNode,
        mode: StreamMode,
        pending: &mut PendingStreamTransaction,
        boundaries: &[ExecutionBoundary],
        source: Option<SourceSpan>,
        inline: &mut InlineProduct,
        chunk: &mut MaterializedChunk,
    ) -> Result<(), NativeEmitterError> {
        if let Some(atoms) = pending.atoms.take() {
            self.append_native_flow(
                owner,
                mode,
                atoms,
                &pending.anchors,
                boundaries,
                source,
                inline,
                chunk,
            )?;
            pending.anchors.clear();
        } else if !pending.anchors.is_empty() {
            let anchors = self.projector.standalone_anchors(&pending.anchors, source);
            match mode {
                StreamMode::Inline => inline.content.extend(anchors),
                StreamMode::Blocks => chunk
                    .append(MaterializedChunk::from_blocks(paragraph(anchors, source)))
                    .map_err(|_| NativeEmitterError::StructureOverflow)?,
            }
            pending.anchors.clear();
        }
        Ok(())
    }

    fn append_native_flow(
        &self,
        owner: &ProjectionNode,
        mode: StreamMode,
        atoms: std::ops::Range<u32>,
        anchors: &[u32],
        boundaries: &[ExecutionBoundary],
        source: Option<SourceSpan>,
        inline: &mut InlineProduct,
        chunk: &mut MaterializedChunk,
    ) -> Result<(), NativeEmitterError> {
        if atoms.is_empty() {
            return Ok(());
        }
        match mode {
            StreamMode::Inline => inline.content.extend(self.projector.segment(
                atoms,
                anchors,
                boundaries,
                source,
                FlowProjectionPolicy::content(false),
            )),
            StreamMode::Blocks => {
                let sequence = self.report.atoms()[atoms.start as usize].sequence;
                let initial_preformatted = match owner.kind {
                    ProjectionNodeKind::NativeScope {
                        kind:
                            ProjectionScopeKind::Region(
                                libmandoc_rs::ExecutionRegionKind::MdocDisplayUnfilled
                                | libmandoc_rs::ExecutionRegionKind::MdocDisplayLiteral
                                | libmandoc_rs::ExecutionRegionKind::MdocDisplayOneLineLiteral,
                            ),
                        ..
                    } => Some(true),
                    ProjectionNodeKind::NativeScope {
                        kind:
                            ProjectionScopeKind::Region(
                                libmandoc_rs::ExecutionRegionKind::MdocDisplayFilled
                                | libmandoc_rs::ExecutionRegionKind::MdocDisplayRagged
                                | libmandoc_rs::ExecutionRegionKind::MdocDisplayCentered
                                | libmandoc_rs::ExecutionRegionKind::MdocDisplayOneLine,
                            ),
                        ..
                    } => Some(false),
                    _ => None,
                };
                let mut blocks = flow_blocks_by_fill_with_default(
                    self.projector,
                    atoms,
                    anchors,
                    boundaries,
                    source,
                    FlowProjectionPolicy::content(false),
                    initial_preformatted,
                );
                if matches!(
                    owner.kind,
                    ProjectionNodeKind::Root | ProjectionNodeKind::NativeScope { .. }
                ) {
                    let layout = self.frames.flow_layout(self.arena, owner, sequence);
                    for block in &mut blocks {
                        if let Some(current) = mant_ir::geometry::block_layout_mut(block) {
                            current.indent_columns =
                                current.indent_columns.saturating_add(layout.indent_columns);
                        }
                    }
                }
                chunk
                    .append(MaterializedChunk::from_blocks(blocks))
                    .map_err(|_| NativeEmitterError::StructureOverflow)?;
            }
        }
        Ok(())
    }

    pub(in crate::mandoc::native_execution::semantics) fn new(
        report: &'a NativeExecutionReport,
        projection: &'a NativeProjection,
        projector: &'a InlineProjector<'a>,
        arena: &'a ProjectionArena,
    ) -> Result<Self, NativeEmitterError> {
        let heading_by_wrapper = projection
            .headings
            .iter()
            .enumerate()
            .map(|(index, heading)| (heading.wrapper, index))
            .collect();
        Ok(Self {
            report,
            projection,
            arena,
            projector,
            heading_by_wrapper,
            anchors: projection
                .anchors
                .iter()
                .map(|anchor| (anchor.key, anchor))
                .collect(),
            mdoc_items: projection
                .mdoc_lists
                .iter()
                .flat_map(|list| &list.items)
                .map(|item| (item.owner, item))
                .collect(),
            man_blocks: projection
                .man_blocks
                .iter()
                .map(|block| (block.owner, block))
                .collect(),
            definition_layouts: projection
                .definitions
                .iter()
                .map(|definition| (definition.owner, definition.responsive.layout.clone()))
                .collect(),
            frames: NativeFramePlan::new(report, projection, arena)
                .map_err(NativeEmitterError::Frame)?,
        })
    }

    fn event_records(events: &[StreamEvent]) -> Result<Vec<RawRecordRef>, NativeEmitterError> {
        let records = events
            .iter()
            .flat_map(StreamEvent::records)
            .collect::<Vec<_>>();
        let unique = records.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != records.len() {
            return Err(NativeEmitterError::InvalidNodeKind);
        }
        Ok(records)
    }

    fn stream_source(&self, events: &[StreamEvent]) -> Option<SourceSpan> {
        events.iter().find_map(|event| match event {
            StreamEvent::Flow { atoms, .. } => self
                .report
                .atoms()
                .get(atoms.start as usize)
                .and_then(|atom| atom.node)
                .and_then(|node| source_span(self.report, node)),
            StreamEvent::CanonicalEffect { records, .. } => records.iter().find_map(|record| {
                let RawRecordRef::Boundary(key) = record else {
                    return None;
                };
                self.report
                    .boundaries()
                    .get(*key as usize)
                    .and_then(|boundary| boundary.node)
                    .and_then(|node| source_span(self.report, node))
            }),
            StreamEvent::Anchor { key } => self
                .anchors
                .get(key)
                .and_then(|anchor| source_span(self.report, anchor.node)),
            StreamEvent::Placement { key } => self
                .report
                .placements()
                .get(*key as usize)
                .and_then(|placement| source_span(self.report, placement.node)),
            StreamEvent::Control { key } => self
                .report
                .controls()
                .get(*key as usize)
                .and_then(|control| source_span(self.report, control.node)),
        })
    }

    fn definition_layout(
        &self,
        owner: ExecutionNodeKey,
    ) -> Result<mant_ir::DefinitionLayout, NativeEmitterError> {
        self.definition_layouts
            .get(&owner)
            .cloned()
            .ok_or(NativeEmitterError::MissingDefinitionLayout(owner))
    }

    fn native_mdoc_item(
        &self,
        owner: ExecutionNodeKey,
    ) -> Result<&NativeMdocListItem, NativeEmitterError> {
        self.mdoc_items
            .get(&owner)
            .copied()
            .ok_or(NativeEmitterError::MissingMdocItem(owner))
    }

    fn man_block(&self, owner: ExecutionNodeKey) -> Result<&NativeManBlock, NativeEmitterError> {
        self.man_blocks
            .get(&owner)
            .copied()
            .ok_or(NativeEmitterError::MissingManBlock(owner))
    }

    fn specialized_coverage(&self, recipe: SpecializedRecipe) -> Vec<SpecializedRecipe> {
        let mut covered = vec![recipe];
        match recipe {
            SpecializedRecipe::Table(table) => {
                for row in self
                    .report
                    .table_rows()
                    .iter()
                    .filter(|row| row.table == table)
                {
                    covered.push(SpecializedRecipe::TableRow(row.key));
                    covered.extend(
                        self.report.table_cell_invocations()[row.cell_invocations.start as usize
                            ..row.cell_invocations.end as usize]
                            .iter()
                            .map(|invocation| {
                                SpecializedRecipe::TableCellInvocation(invocation.key)
                            }),
                    );
                }
            }
            SpecializedRecipe::Equation(equation) => covered.extend(
                self.report
                    .equation_invocations()
                    .iter()
                    .filter(|invocation| invocation.equation == equation)
                    .map(|invocation| SpecializedRecipe::EquationInvocation(invocation.key)),
            ),
            SpecializedRecipe::TableRow(_)
            | SpecializedRecipe::TableCellInvocation(_)
            | SpecializedRecipe::EquationInvocation(_) => {}
        }
        covered
    }
}

impl RecipeEmitter for NativeRecipeEmitter<'_> {
    type Error = NativeEmitterError;

    fn heading(&mut self, node: &ProjectionNode) -> Result<Emission<Vec<Inline>>, Self::Error> {
        let ProjectionNodeKind::Heading { wrapper, .. } = node.kind else {
            return Err(NativeEmitterError::InvalidNodeKind);
        };
        let index = *self
            .heading_by_wrapper
            .get(&wrapper)
            .ok_or(NativeEmitterError::MissingHeading(wrapper))?;
        let heading = &self.projection.headings[index];
        let source = source_span(self.report, heading.head);
        let mut content = self.projector.segment(
            heading.atoms.clone(),
            &[],
            &heading.boundaries,
            source,
            FlowProjectionPolicy::content(false),
        );
        remove_structural_heading_bold(&mut content);
        if content.is_empty() {
            content.push(Inline::Text {
                value: heading.label.clone(),
            });
        }
        let consumed = node
            .pieces
            .iter()
            .flat_map(|piece| match piece {
                crate::mandoc::native_execution::ownership::ProjectionPiece::Stream {
                    events,
                    ..
                } => events.iter().flat_map(StreamEvent::records).collect(),
                _ => Vec::new(),
            })
            .collect();
        Ok(Emission {
            value: content,
            consumed,
        })
    }

    fn stream(
        &mut self,
        owner: &ProjectionNode,
        mode: StreamMode,
        events: &[StreamEvent],
    ) -> Result<Emission<NodeProduct>, Self::Error> {
        let consumed = Self::event_records(events)?;
        let source = self.stream_source(events);
        let boundary_keys = events
            .iter()
            .flat_map(|event| match event {
                StreamEvent::CanonicalEffect { records, .. } => records.as_slice(),
                StreamEvent::Flow { .. }
                | StreamEvent::Anchor { .. }
                | StreamEvent::Placement { .. }
                | StreamEvent::Control { .. } => &[],
            })
            .filter_map(|record| match record {
                RawRecordRef::Boundary(key) => Some(*key),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let boundaries = boundary_keys
            .into_iter()
            .filter_map(|key| self.report.boundaries().get(key as usize))
            .filter(|boundary| boundary.request != BoundaryRequest::VerticalSpace)
            .cloned()
            .collect::<Vec<_>>();
        let mut pending = PendingStreamTransaction::default();
        let mut inline = InlineProduct::default();
        let mut chunk = MaterializedChunk::default();
        for event in events {
            match event {
                StreamEvent::Anchor { key } => {
                    pending.anchor(*key);
                }
                // A placement is an observation of the current native device
                // position, not a formatter operation.  It remains in the
                // consumer receipt but cannot settle this transaction.
                StreamEvent::Placement { .. } => {}
                // The control action owns its native state transition; all
                // output-bearing consequences are materialized by the
                // separately owned canonical effect closure.
                StreamEvent::Control { .. } => {}
                StreamEvent::CanonicalEffect { outcome, .. } => match outcome {
                    CanonicalEffectOutcome::StateOnly
                    | CanonicalEffectOutcome::StructuralSpacing => {}
                    CanonicalEffectOutcome::Trace(steps) => {
                        for step in steps {
                            match step.kind {
                                CanonicalEffectKind::SoftDeviceWrap
                                | CanonicalEffectKind::FieldSettlement => {}
                                CanonicalEffectKind::FieldEnd { .. } => {
                                    return Err(NativeEmitterError::InvalidNodeKind);
                                }
                                CanonicalEffectKind::LogicalLineCommit { empty } => {
                                    self.commit_pending_stream(
                                        owner,
                                        mode,
                                        &mut pending,
                                        &boundaries,
                                        source,
                                        &mut inline,
                                        &mut chunk,
                                    )?;
                                    match mode {
                                        StreamMode::Inline => {
                                            inline.content.push(Inline::LineBreak)
                                        }
                                        StreamMode::Blocks => chunk.line_commit(empty),
                                    }
                                }
                                CanonicalEffectKind::BlankDeviceLine => {
                                    self.commit_pending_stream(
                                        owner,
                                        mode,
                                        &mut pending,
                                        &boundaries,
                                        source,
                                        &mut inline,
                                        &mut chunk,
                                    )?;
                                    match (mode, step.inline_flow) {
                                        (StreamMode::Inline, _) => {
                                            inline.content.push(Inline::LineBreak)
                                        }
                                        (StreamMode::Blocks, true) => chunk.line_commit(true),
                                        (StreamMode::Blocks, false) => {
                                            chunk.blank_device_line(source)
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                StreamEvent::Flow { atoms, .. } => {
                    pending.extend(atoms);
                }
            }
        }
        self.commit_pending_stream(
            owner,
            mode,
            &mut pending,
            &boundaries,
            source,
            &mut inline,
            &mut chunk,
        )?;
        let value = match mode {
            StreamMode::Inline => NodeProduct::Inline(inline),
            StreamMode::Blocks => NodeProduct::Blocks(chunk),
        };
        Ok(Emission { value, consumed })
    }

    fn specialized_root(
        &mut self,
        _owner: &ProjectionNode,
        recipe: SpecializedRecipe,
    ) -> Result<SpecializedEmission<MaterializedChunk>, Self::Error> {
        let blocks = match recipe {
            SpecializedRecipe::Table(key) => {
                let table = self
                    .projection
                    .tables
                    .iter()
                    .find(|table| table.key == key)
                    .ok_or(NativeEmitterError::MissingTable(key.0))?;
                let source = table
                    .rows
                    .first()
                    .and_then(|row| source_span(self.report, row.node));
                let leading = self
                    .projector
                    .standalone_anchors(&table.leading_anchors, source);
                let indent_columns = i32::try_from(table.offset_bu / table.cell_bu)
                    .unwrap_or(i32::MAX)
                    .saturating_sub(document_origin_columns(self.report));
                table.to_ir_blocks(leading, indent_columns)
            }
            SpecializedRecipe::Equation(key) => {
                let equation = self
                    .projection
                    .equations
                    .iter()
                    .find(|equation| equation.key == key)
                    .ok_or(NativeEmitterError::MissingEquation(key.0))?;
                if equation.flags.contains(ExecutionEquationFlags::NO_CONTENT) {
                    Vec::new()
                } else {
                    let source = source_span(self.report, equation.node);
                    vec![
                        if equation.flags.contains(ExecutionEquationFlags::DISPLAY) {
                            Block::Equation {
                                value: crate::mandoc::roff_escape::visible_text(
                                    &equation.normalized_text,
                                ),
                                display: true,
                                layout: LayoutHint::default(),
                                source,
                            }
                        } else {
                            Block::Paragraph {
                                children: vec![Inline::Text {
                                    value: crate::mandoc::roff_escape::visible_text(
                                        &equation.normalized_text,
                                    ),
                                }],
                                layout: LayoutHint::default(),
                                source,
                            }
                        },
                    ]
                }
            }
            other => return Err(NativeEmitterError::InvalidSpecializedRecipe(other)),
        };
        let anchors = match recipe {
            SpecializedRecipe::Table(key) => self
                .projection
                .tables
                .iter()
                .find(|table| table.key == key)
                .map_or_else(Vec::new, |table| table.leading_anchors.clone()),
            _ => Vec::new(),
        };
        Ok(SpecializedEmission {
            value: MaterializedChunk::from_blocks(blocks),
            covered: self.specialized_coverage(recipe),
            anchors,
        })
    }

    fn mdoc_item(
        &mut self,
        owner: &ProjectionNode,
        leading: InlineProduct,
        head: InlineProduct,
        bodies: Vec<MaterializedChunk>,
    ) -> Result<MaterializedMdocItem, Self::Error> {
        let ProjectionNodeKind::MdocItem {
            owner: native_owner,
            flow_epoch,
            kind,
            ..
        } = owner.kind
        else {
            return Err(NativeEmitterError::InvalidNodeKind);
        };
        let native = self.native_mdoc_item(native_owner)?;
        Ok(MaterializedMdocItem {
            item: StableSemanticItem(owner.key),
            flow_epoch,
            leading: leading.content,
            term: MaterializedTerm {
                owner: StableSemanticOwner {
                    projection: owner.key,
                    native: native_owner,
                },
                content: head.content,
                evidence: markup_evidence(&[native.head.node], self.report, self.projector),
            },
            bodies,
            // Only run-in/tag-style items have a definition head/body field
            // contract.  Bullet, enum, item, and column presentation is
            // represented by the enclosing list/table layout instead.
            layout: if definition_list_kind(kind) {
                self.definition_layout(native_owner)?
            } else {
                mant_ir::DefinitionLayout::default()
            },
            source: source_span(self.report, native_owner),
        })
    }

    fn mdoc_list(
        &mut self,
        owner: &ProjectionNode,
        mut leading: MaterializedChunk,
        items: Vec<MaterializedMdocItem>,
    ) -> Result<MaterializedChunk, Self::Error> {
        let ProjectionNodeKind::MdocList {
            owner: native_owner,
            kind,
        } = owner.kind
        else {
            return Err(NativeEmitterError::InvalidNodeKind);
        };
        let native_list = self
            .projection
            .mdoc_lists
            .iter()
            .find(|list| list.owner == native_owner)
            .ok_or(NativeEmitterError::MissingMdocList(native_owner))?;
        let source = source_span(self.report, native_owner);
        // A native line commitment owned by `Bl`/`It` happens before the
        // structural list is emitted.  Settle it now so it cannot leak past
        // the list and become spacing before the following paragraph.
        leading.settle_before_structural(source);
        let block_index = u32::try_from(leading.blocks.len())
            .map_err(|_| NativeEmitterError::StructureOverflow)?;
        let list_layout = self
            .frames
            .list_layout(native_owner)
            .map_err(NativeEmitterError::Frame)?;
        let block = if kind == ExecutionMdocListKind::Column {
            Block::Table {
                rows: items
                    .into_iter()
                    .map(|item| {
                        let mut cells = item
                            .bodies
                            .into_iter()
                            .map(|body| TableCell {
                                kind: TableCellKind::Text,
                                blocks: body.blocks,
                                column_span: 1,
                                row_span: 1,
                                alignment: Some(TableAlignment::Left),
                            })
                            .collect::<Vec<_>>();
                        if cells.is_empty() {
                            cells.push(TableCell {
                                kind: TableCellKind::Text,
                                blocks: paragraph(item.leading, item.source),
                                column_span: 1,
                                row_span: 1,
                                alignment: Some(TableAlignment::Left),
                            });
                        } else if !item.leading.is_empty() {
                            prepend_inline(&mut cells[0].blocks, item.leading, item.source);
                        }
                        TableRow {
                            kind: TableRowKind::Data,
                            cells,
                        }
                    })
                    .collect(),
                layout: list_layout,
                source,
            }
        } else if definition_list_kind(kind) {
            let mut definitions = Vec::with_capacity(items.len());
            let mut groups = Vec::new();
            let mut pending = None;
            let mut previous_epoch = None;
            for (index, mut item) in items.into_iter().enumerate() {
                if previous_epoch.is_some_and(|epoch| epoch != item.flow_epoch) {
                    pending = None;
                }
                let mut term = item.leading;
                term.extend(item.term.content.clone());
                let mut description = MaterializedChunk::default();
                for body in item.bodies.drain(..) {
                    description
                        .append(body)
                        .map_err(|_| NativeEmitterError::StructureOverflow)?;
                }
                if description.blocks.is_empty() {
                    pending = Some(pending.unwrap_or(index));
                } else if let Some(start) = pending.take()
                    && start < index
                {
                    groups.push(DeclarationGroup {
                        start_item: start,
                        end_item: index + 1,
                    });
                }
                let prefix = [
                    ContentBlockStep::Block { index: block_index },
                    ContentBlockStep::DefinitionItem {
                        index: u32::try_from(index)
                            .map_err(|_| NativeEmitterError::StructureOverflow)?,
                    },
                ];
                let nested = description.nest(&prefix);
                leading.semantic_items.extend(nested.semantic_items);
                definitions.push(DefinitionItem {
                    terms: vec![term],
                    description: nested.blocks,
                    entry: None,
                    source: item.source,
                    layout: item.layout,
                });
                leading
                    .register_definition_item(item.item, prefix.to_vec(), vec![item.term])
                    .map_err(|_| NativeEmitterError::StructureOverflow)?;
                previous_epoch = Some(item.flow_epoch);
            }
            Block::DefinitionList {
                items: definitions,
                declaration_groups: groups,
                compact: native_list.compact,
                layout: list_layout,
                source,
            }
        } else {
            let kind = ordinary_list_kind(kind).ok_or(NativeEmitterError::InvalidNodeKind)?;
            let mut list_items = Vec::new();
            for (index, mut item) in items.into_iter().enumerate() {
                let mut body = MaterializedChunk::from_blocks(paragraph(item.leading, item.source));
                for next in item.bodies.drain(..) {
                    body.append(next)
                        .map_err(|_| NativeEmitterError::StructureOverflow)?;
                }
                let prefix = [
                    ContentBlockStep::Block { index: block_index },
                    ContentBlockStep::ListItem {
                        index: u32::try_from(index)
                            .map_err(|_| NativeEmitterError::StructureOverflow)?,
                    },
                ];
                let nested = body.nest(&prefix);
                leading.semantic_items.extend(nested.semantic_items);
                list_items.push(ListItem {
                    layout: ListItemLayout::default(),
                    source: item.source,
                    entry: None,
                    blocks: nested.blocks,
                });
            }
            Block::List {
                kind,
                items: list_items,
                compact: native_list.compact,
                layout: list_layout,
                source,
            }
        };
        leading.blocks.push(block);
        Ok(leading)
    }

    fn man_item(
        &mut self,
        owner: &ProjectionNode,
        head: InlineProduct,
        mut body: MaterializedChunk,
        continuations: Vec<MaterializedManContinuation>,
    ) -> Result<MaterializedManItem, Self::Error> {
        let (primary, _kind, presentation, origin_columns, layout, source, evidence) =
            match owner.kind {
                ProjectionNodeKind::ManDefinitionItem { primary, kind } => {
                    let native = self.man_block(primary)?;
                    let layout = self.definition_layout(primary)?;
                    let presentation = super::super::man::classify(
                        std::slice::from_ref(&head.content),
                        body.blocks.is_empty(),
                        native,
                        self.report,
                    );
                    let origin_columns = self
                        .projection
                        .definitions
                        .iter()
                        .find(|definition| definition.owner == primary)
                        .map(|definition| {
                            definition
                                .responsive
                                .label_origin_columns
                                .saturating_sub(document_origin_columns(self.report))
                        })
                        .ok_or(NativeEmitterError::MissingDefinitionLayout(primary))?;
                    (
                        primary,
                        kind,
                        presentation,
                        origin_columns,
                        layout,
                        source_span(self.report, primary),
                        man_head_evidence(native, &head.content, self.report, self.projector),
                    )
                }
                ProjectionNodeKind::ManHangingPair {
                    term,
                    scope,
                    term_origin_columns,
                    ..
                } => {
                    let body_origin = body
                        .blocks
                        .iter()
                        .find_map(mant_ir::geometry::block_layout)
                        .map_or(term_origin_columns.saturating_add(4), |layout| {
                            layout.indent_columns
                        });
                    mant_ir::geometry::rebase_roots(&mut body.blocks, body_origin, 0);
                    (
                        term,
                        ExecutionManBlockKind::RelativeIndent,
                        Presentation::Definition,
                        term_origin_columns,
                        DefinitionLayout {
                            placement: mant_ir::DefinitionPlacement::Stacked,
                            body_indent_columns: body_origin.saturating_sub(term_origin_columns),
                            spacing_before_lines: Some(0),
                            ..DefinitionLayout::default()
                        },
                        source_span(self.report, term).or_else(|| source_span(self.report, scope)),
                        markup_evidence(&[term], self.report, self.projector),
                    )
                }
                _ => return Err(NativeEmitterError::InvalidNodeKind),
            };
        let mut terms = vec![MaterializedTerm {
            owner: StableSemanticOwner {
                projection: owner.key,
                native: primary,
            },
            evidence,
            content: head.content,
        }];
        for continuation in continuations {
            let continuation_native = self.man_block(continuation.owner)?;
            terms.push(MaterializedTerm {
                owner: StableSemanticOwner {
                    projection: owner.key,
                    native: continuation.owner,
                },
                evidence: man_head_evidence(
                    continuation_native,
                    &continuation.head.content,
                    self.report,
                    self.projector,
                ),
                content: continuation.head.content,
            });
            if !continuation.body.blocks.is_empty() {
                if !body.blocks.is_empty() {
                    return Err(NativeEmitterError::ContinuationBodyConflict(primary));
                }
                body = continuation.body;
            }
        }
        Ok(MaterializedManItem {
            item: StableSemanticItem(owner.key),
            presentation,
            origin_columns,
            terms,
            description: body,
            layout,
            source,
        })
    }

    fn man_run(
        &mut self,
        _owner: &ProjectionNode,
        mut leading: MaterializedChunk,
        items: Vec<MaterializedManItem>,
    ) -> Result<MaterializedChunk, Self::Error> {
        let mut definitions = Vec::<MaterializedManItem>::new();
        let mut ordered_tail = None::<(usize, Ordinal)>;
        for item in items {
            match item.presentation {
                Presentation::Definition => {
                    if definitions
                        .last()
                        .is_some_and(|previous| previous.origin_columns != item.origin_columns)
                    {
                        flush_man_definitions(&mut leading, &mut definitions)?;
                    }
                    ordered_tail = None;
                    definitions.push(item);
                }
                Presentation::Bullet => {
                    flush_man_definitions(&mut leading, &mut definitions)?;
                    ordered_tail = None;
                    append_man_bullet(&mut leading, item)?;
                }
                Presentation::Ordered(marker) => {
                    flush_man_definitions(&mut leading, &mut definitions)?;
                    ordered_tail = append_man_ordered(&mut leading, item, marker, ordered_tail)?;
                }
            }
        }
        flush_man_definitions(&mut leading, &mut definitions)?;
        Ok(leading)
    }

    fn ordinary_scope(
        &mut self,
        owner: &ProjectionNode,
        mut children: MaterializedChunk,
    ) -> Result<MaterializedChunk, Self::Error> {
        if matches!(
            owner.kind,
            ProjectionNodeKind::NativeScope {
                kind: ProjectionScopeKind::Region(
                    libmandoc_rs::ExecutionRegionKind::MdocSynopsisItem
                ),
                ..
            }
        ) {
            // Fixed CVS intentionally renders Ft/Fn components on device
            // rows, but the source-neutral synopsis declaration is one
            // addressable signature.  Preserve the declaration boundary
            // (the surrounding item scope) while normalizing its internal
            // device-row commits to lexical spaces.
            for block in &mut children.blocks {
                if let Some(inlines) = super::inline_children_mut(block) {
                    synopsis_breaks_to_spaces(inlines);
                }
            }
            let macro_name = owner
                .source_owner
                .and_then(|key| self.report.nodes().get(key.0 as usize))
                .and_then(|node| node.macro_name.as_deref());
            // This is the same declaration grammar as fixed CVS
            // `synopsis_pre()` and the legacy source producer: Ft is a
            // prefix of Fn/Fo, while function and Fd declarations close the
            // semantic row.  In/Cd/Vt allow ordinary explanatory text to
            // follow on their row but start a new declaration themselves.
            let (before, after) = match macro_name {
                Some("Ft") => (true, false),
                Some("Fn" | "Fo") => (false, true),
                Some("Fd") => (true, true),
                Some("In" | "Cd" | "Vt") => (true, false),
                _ => (false, false),
            };
            if before {
                for block in &mut children.blocks {
                    if let Some(inlines) = super::inline_children_mut(block) {
                        trim_synopsis_leading_space(inlines);
                        break;
                    }
                }
            }
            children.set_inline_barriers(before, after);
        }
        Ok(children)
    }
}

fn synopsis_breaks_to_spaces(inlines: &mut [Inline]) {
    for inline in inlines {
        match inline {
            Inline::LineBreak => {
                *inline = Inline::Text {
                    value: " ".to_owned(),
                };
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => synopsis_breaks_to_spaces(children),
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => {}
        }
    }
}

fn trim_synopsis_leading_space(inlines: &mut Vec<Inline>) {
    loop {
        let Some(first) = inlines.first_mut() else {
            return;
        };
        let empty = match first {
            Inline::Text { value } => {
                let trimmed = value.trim_start_matches(char::is_whitespace);
                if trimmed.len() != value.len() {
                    *value = trimmed.to_owned();
                }
                value.is_empty()
            }
            _ => false,
        };
        if !empty {
            return;
        }
        inlines.remove(0);
    }
}

fn prepend_inline(blocks: &mut Vec<Block>, mut inline: Vec<Inline>, source: Option<SourceSpan>) {
    if inline.is_empty() {
        return;
    }
    match blocks.first_mut() {
        Some(Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) => {
            inline.append(children);
            *children = inline;
        }
        _ => blocks
            .splice(0..0, paragraph(inline, source))
            .for_each(drop),
    }
}

fn flush_man_definitions(
    output: &mut MaterializedChunk,
    pending: &mut Vec<MaterializedManItem>,
) -> Result<(), NativeEmitterError> {
    if pending.is_empty() {
        return Ok(());
    }
    let block_index =
        u32::try_from(output.blocks.len()).map_err(|_| NativeEmitterError::StructureOverflow)?;
    let origin_columns = pending[0].origin_columns;
    let mut definitions = Vec::with_capacity(pending.len());
    let mut groups = Vec::new();
    let mut open_group = None;
    for (index, item) in pending.drain(..).enumerate() {
        if item.description.blocks.is_empty() {
            open_group = Some(open_group.unwrap_or(index));
        } else if let Some(start) = open_group.take()
            && start < index
        {
            groups.push(DeclarationGroup {
                start_item: start,
                end_item: index + 1,
            });
        }
        let prefix = [
            ContentBlockStep::Block { index: block_index },
            ContentBlockStep::DefinitionItem {
                index: u32::try_from(index).map_err(|_| NativeEmitterError::StructureOverflow)?,
            },
        ];
        let nested = item.description.nest(&prefix);
        output.semantic_items.extend(nested.semantic_items);
        definitions.push(DefinitionItem {
            terms: item.terms.iter().map(|term| term.content.clone()).collect(),
            description: nested.blocks,
            entry: None,
            source: item.source,
            layout: item.layout,
        });
        output
            .register_definition_item(item.item, prefix.to_vec(), item.terms)
            .map_err(|_| NativeEmitterError::StructureOverflow)?;
    }
    output.blocks.push(Block::DefinitionList {
        items: definitions,
        declaration_groups: groups,
        compact: true,
        layout: LayoutHint {
            indent_columns: origin_columns,
            ..LayoutHint::default()
        },
        source: None,
    });
    Ok(())
}

fn append_man_bullet(
    output: &mut MaterializedChunk,
    item: MaterializedManItem,
) -> Result<(), NativeEmitterError> {
    let spacing = item.layout.spacing_before_lines.unwrap_or(0);
    let (block_index, item_index) = match output.blocks.last() {
        Some(Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        }) => (output.blocks.len() - 1, items.len()),
        _ => {
            output.blocks.push(Block::List {
                kind: ListKind::Bullet,
                compact: spacing == 0,
                items: Vec::new(),
                layout: LayoutHint::default(),
                source: item.source,
            });
            (output.blocks.len() - 1, 0)
        }
    };
    let list_item = man_list_item(output, item, block_index, item_index, 2)?;
    let Block::List { compact, items, .. } = &mut output.blocks[block_index] else {
        unreachable!("selected bullet block")
    };
    *compact &= spacing == 0;
    items.push(list_item);
    Ok(())
}

fn append_man_ordered(
    output: &mut MaterializedChunk,
    item: MaterializedManItem,
    marker: Ordinal,
    tail: Option<(usize, Ordinal)>,
) -> Result<Option<(usize, Ordinal)>, NativeEmitterError> {
    let spacing = item.layout.spacing_before_lines.unwrap_or(0);
    let block_index = if let Some((block, previous)) = tail
        && previous.style == marker.style
        && previous.value.checked_add(1) == Some(marker.value)
        && matches!(
            output.blocks.get(block),
            Some(Block::List {
                kind: ListKind::Ordered { .. },
                ..
            })
        ) {
        block
    } else {
        output.blocks.push(Block::List {
            kind: ListKind::Ordered {
                start: Some(marker.value),
            },
            compact: spacing == 0,
            items: Vec::new(),
            layout: LayoutHint::default(),
            source: item.source,
        });
        output.blocks.len() - 1
    };
    let item_index = match &output.blocks[block_index] {
        Block::List { items, .. } => items.len(),
        _ => unreachable!("selected ordered block"),
    };
    let marker_width = mant_ir::geometry::coordinate(mant_ir::geometry::text_width(&format!(
        "{}. ",
        marker.value
    )));
    let list_item = man_list_item(output, item, block_index, item_index, marker_width)?;
    let Block::List { compact, items, .. } = &mut output.blocks[block_index] else {
        unreachable!("selected ordered block")
    };
    *compact &= spacing == 0;
    items.push(list_item);
    Ok(Some((block_index, marker)))
}

fn man_list_item(
    output: &mut MaterializedChunk,
    item: MaterializedManItem,
    block_index: usize,
    item_index: usize,
    marker_width: i32,
) -> Result<ListItem, NativeEmitterError> {
    let MaterializedManItem {
        terms,
        mut description,
        layout,
        source,
        ..
    } = item;
    mant_ir::geometry::rebase_roots(
        &mut description.blocks,
        layout.body_indent_columns,
        marker_width,
    );
    let mut anchors = Vec::new();
    for term in &terms {
        crate::mandoc::targets::inline_anchor_ids(&term.content, &mut anchors);
    }
    crate::mandoc::targets::attach_targets(
        &mut description.blocks,
        anchors,
        LayoutHint::default(),
        source,
    );
    let prefix = [
        ContentBlockStep::Block {
            index: u32::try_from(block_index).map_err(|_| NativeEmitterError::StructureOverflow)?,
        },
        ContentBlockStep::ListItem {
            index: u32::try_from(item_index).map_err(|_| NativeEmitterError::StructureOverflow)?,
        },
    ];
    let nested = description.nest(&prefix);
    output.semantic_items.extend(nested.semantic_items);
    Ok(ListItem {
        layout: ListItemLayout {
            spacing_before_lines: layout.spacing_before_lines,
        },
        source,
        entry: None,
        blocks: nested.blocks,
    })
}

const fn definition_list_kind(kind: ExecutionMdocListKind) -> bool {
    matches!(
        kind,
        ExecutionMdocListKind::Hang
            | ExecutionMdocListKind::Overhang
            | ExecutionMdocListKind::Inset
            | ExecutionMdocListKind::Diagnostic
            | ExecutionMdocListKind::Tag
    )
}

const fn ordinary_list_kind(kind: ExecutionMdocListKind) -> Option<ListKind> {
    match kind {
        ExecutionMdocListKind::Bullet
        | ExecutionMdocListKind::Dash
        | ExecutionMdocListKind::Hyphen => Some(ListKind::Bullet),
        ExecutionMdocListKind::Enum => Some(ListKind::Ordered { start: None }),
        ExecutionMdocListKind::Item => Some(ListKind::Plain),
        ExecutionMdocListKind::Hang
        | ExecutionMdocListKind::Overhang
        | ExecutionMdocListKind::Inset
        | ExecutionMdocListKind::Diagnostic
        | ExecutionMdocListKind::Tag
        | ExecutionMdocListKind::Column => None,
    }
}
