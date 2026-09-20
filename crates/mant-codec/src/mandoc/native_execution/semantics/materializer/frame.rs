//! Immutable projection of fixed-CVS placement checkpoints.
//!
//! `mdoc_term.c::termp_bl_pre()` records the parent origin before applying a
//! list offset, and `termp_it_pre()` records the resulting HEAD/BODY origins.
//! `man_term.c::pre_in()` records a persistent origin transition.  Keep those
//! absolute basic-unit endpoints until the final IR edge is known; rounding a
//! BU delta is not equivalent to subtracting independently rounded endpoints.

use std::collections::BTreeMap;

use libmandoc_rs::{
    ExecutionNodeKey, ExecutionPlacementCheckpoint, ExecutionPlacementPhase, NativeExecutionReport,
};
use mant_ir::LayoutHint;

use crate::mandoc::native_execution::NativeProjection;
use crate::mandoc::native_execution::layout::relative_basic_units_to_columns;
use crate::mandoc::native_execution::ownership::{
    DefinitionSegmentKind, ProjectionArena, ProjectionNode, ProjectionNodeKey, ProjectionNodeKind,
};

#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc::native_execution::semantics) enum NativeFrameError {
    MissingListEnter(ExecutionNodeKey),
    MissingListItemPlacement(ExecutionNodeKey),
    MissingDefinitionSegmentOrigin(ExecutionNodeKey),
    DuplicatePlacement(ExecutionNodeKey, ExecutionPlacementPhase),
    InconsistentCellWidth,
}

impl std::fmt::Display for NativeFrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for NativeFrameError {}

#[derive(Clone, Copy, Debug)]
struct FrameEdge {
    parent_origin: i64,
    origin: i64,
    cell: i64,
}

/// One immutable interpretation of all native placement facts.
pub(super) struct NativeFramePlan {
    checkpoints: Vec<ExecutionPlacementCheckpoint>,
    document_origin_bu: i64,
    cell_bu: i64,
    list_edges: BTreeMap<ExecutionNodeKey, FrameEdge>,
    segment_origins: BTreeMap<ExecutionNodeKey, i64>,
    definition_body_origins: BTreeMap<ExecutionNodeKey, i64>,
}

impl NativeFramePlan {
    pub(super) fn new(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
        arena: &ProjectionArena,
    ) -> Result<Self, NativeFrameError> {
        let cell_bu = report
            .placements()
            .first()
            .map_or(1, |checkpoint| checkpoint.cell_bu);
        if cell_bu <= 0
            || report
                .placements()
                .iter()
                .any(|checkpoint| checkpoint.cell_bu != cell_bu)
        {
            return Err(NativeFrameError::InconsistentCellWidth);
        }
        let document_origin_bu = i64::from(report.content_indent_columns()) * cell_bu;
        let mut placements = vec![[None; 6]; report.nodes().len()];
        for checkpoint in report.placements() {
            let slot = &mut placements[checkpoint.node.0 as usize]
                [placement_phase_index(checkpoint.phase)];
            if slot.replace(checkpoint).is_some() {
                return Err(NativeFrameError::DuplicatePlacement(
                    checkpoint.node,
                    checkpoint.phase,
                ));
            }
        }
        let placement = |node: ExecutionNodeKey, phase: ExecutionPlacementPhase| {
            placements[node.0 as usize][placement_phase_index(phase)]
        };
        let mut list_edges = BTreeMap::new();
        for list in &projection.mdoc_lists {
            let enter = placement(list.owner, ExecutionPlacementPhase::Enter)
                .ok_or(NativeFrameError::MissingListEnter(list.owner))?;
            // An empty Bl still owns zero-width targets but never executes an
            // It HEAD.  Fixed CVS termp_bl_pre() records only the list-enter
            // origin in that case, so its structural frame is exactly the
            // parent origin rather than an error or an inferred item offset.
            let origin_bu = match list.items.first() {
                Some(first) => {
                    placement(first.head.node, ExecutionPlacementPhase::Head)
                        .ok_or(NativeFrameError::MissingListItemPlacement(list.owner))?
                        .offset_bu
                }
                None => enter.offset_bu,
            };
            list_edges.insert(
                list.owner,
                FrameEdge {
                    parent_origin: enter.offset_bu,
                    origin: origin_bu,
                    cell: cell_bu,
                },
            );
        }
        let mut segment_origins = BTreeMap::new();
        let mut definition_body_origins = BTreeMap::new();
        for list in &projection.mdoc_lists {
            for item in &list.items {
                // Fixed CVS executes placement for every It HEAD/BODY, but
                // emits a definition contract only for the run-in/definition
                // list families (hang, ohang, inset, diag, tag).  Bullet,
                // enum, item, and column lists therefore derive their frame
                // from the same placement events without pretending to own a
                // definition field.
                let head_origin = placement(item.head.node, ExecutionPlacementPhase::Head)
                    .ok_or(NativeFrameError::MissingListItemPlacement(item.head.node))?
                    .offset_bu;
                segment_origins.insert(item.head.node, head_origin);
                let mut body_origin = None;
                for body in &item.bodies {
                    let origin = placement(body.node, ExecutionPlacementPhase::Body)
                        .ok_or(NativeFrameError::MissingListItemPlacement(body.node))?
                        .offset_bu;
                    segment_origins.insert(body.node, origin);
                    body_origin.get_or_insert(origin);
                }
                definition_body_origins.insert(item.owner, body_origin.unwrap_or(head_origin));
            }
        }
        for block in &projection.man_blocks {
            let Some(contract) = report.wrappers()[block.wrapper as usize].definition else {
                continue;
            };
            segment_origins.insert(block.head.node, contract.head.offset_bu);
            segment_origins.insert(block.body.node, contract.body.offset_bu);
            definition_body_origins.insert(block.owner, contract.body.offset_bu);
        }
        for node in &arena.nodes {
            if let ProjectionNodeKind::DefinitionSegment { kind } = node.kind {
                debug_assert!(matches!(
                    kind,
                    DefinitionSegmentKind::MdocHead
                        | DefinitionSegmentKind::MdocBody { .. }
                        | DefinitionSegmentKind::ManHead
                        | DefinitionSegmentKind::ManBody
                ));
                let source_owner = node
                    .source_owner
                    .expect("definition segment has a native source owner");
                let synthetic_hanging_head = node.content_parent.is_some_and(|parent| {
                    matches!(
                        arena.node(parent).kind,
                        ProjectionNodeKind::ManHangingPair { .. }
                    )
                });
                if !synthetic_hanging_head && !segment_origins.contains_key(&source_owner) {
                    return Err(NativeFrameError::MissingDefinitionSegmentOrigin(
                        source_owner,
                    ));
                }
            }
        }
        let mut checkpoints = report.placements().to_vec();
        checkpoints.sort_by_key(|checkpoint| (checkpoint.sequence, checkpoint.key));
        Ok(Self {
            checkpoints,
            document_origin_bu,
            cell_bu,
            list_edges,
            segment_origins,
            definition_body_origins,
        })
    }

    pub(super) fn list_layout(
        &self,
        owner: ExecutionNodeKey,
    ) -> Result<LayoutHint, NativeFrameError> {
        let edge = self
            .list_edges
            .get(&owner)
            .ok_or(NativeFrameError::MissingListEnter(owner))?;
        Ok(LayoutHint {
            indent_columns: relative_basic_units_to_columns(
                edge.origin,
                edge.parent_origin,
                edge.cell,
            ),
            ..LayoutHint::default()
        })
    }

    pub(super) fn flow_layout(
        &self,
        arena: &ProjectionArena,
        owner: &ProjectionNode,
        sequence: u64,
    ) -> LayoutHint {
        let index = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.sequence <= sequence);
        let origin_bu = index
            .checked_sub(1)
            .map_or(self.document_origin_bu, |index| {
                self.checkpoints[index].offset_bu
            });
        let parent_origin = self.content_origin(arena, owner.key);
        LayoutHint {
            indent_columns: relative_basic_units_to_columns(origin_bu, parent_origin, self.cell_bu),
            ..LayoutHint::default()
        }
    }

    fn content_origin(&self, arena: &ProjectionArena, mut key: ProjectionNodeKey) -> i64 {
        loop {
            let node = arena.node(key);
            if let Some(source_owner) = node.source_owner
                && let Some(origin) = self.segment_origins.get(&source_owner)
            {
                return *origin;
            }
            match node.kind {
                ProjectionNodeKind::Root => return self.document_origin_bu,
                ProjectionNodeKind::MdocItem { owner, .. }
                | ProjectionNodeKind::ManDefinitionItem { primary: owner, .. } => {
                    if let Some(origin) = self.definition_body_origins.get(&owner) {
                        return *origin;
                    }
                }
                ProjectionNodeKind::MdocList { owner, .. } => {
                    if let Some(edge) = self.list_edges.get(&owner) {
                        return edge.origin;
                    }
                }
                ProjectionNodeKind::ManHangingPair {
                    term_origin_columns,
                    ..
                } => {
                    return self.document_origin_bu.saturating_add(
                        i64::from(term_origin_columns).saturating_mul(self.cell_bu),
                    );
                }
                _ => {}
            }
            let Some(parent) = node.content_parent else {
                return self.document_origin_bu;
            };
            key = parent;
        }
    }
}

const fn placement_phase_index(phase: ExecutionPlacementPhase) -> usize {
    match phase {
        ExecutionPlacementPhase::Enter => 0,
        ExecutionPlacementPhase::Content => 1,
        ExecutionPlacementPhase::Head => 2,
        ExecutionPlacementPhase::Body => 3,
        ExecutionPlacementPhase::OriginTransition => 4,
        ExecutionPlacementPhase::Exit => 5,
    }
}
