//! Structured-rendering profiles, limits, errors, and entry points.

use super::StructuredDocument;
use crate::{InputFormat, SourceBundle};
use std::{error::Error, fmt};

/// Default deterministic native render width.
pub const DEFAULT_STRUCTURED_WIDTH: u32 = 78;
/// Smallest supported native render width.
pub const MIN_STRUCTURED_WIDTH: u32 = 20;
/// Largest supported native render width.
pub const MAX_STRUCTURED_WIDTH: u32 = 1_000;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructuredProfile {
    #[default]
    Utf8,
    Ascii,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredStage {
    Marshal,
    Resolve,
    Parse,
    Render,
    Finalize,
    Check,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredLimitKind {
    InputSources,
    Sources,
    SourcePathBytes,
    DecodedSourceBytesPerSource,
    DecodedSourceBytesTotal,
    SourceMapEntries,
    SourceMapBytes,
    BuilderOperations,
    BuilderAllocatedBytes,
    ContentBytes,
    Owners,
    Blocks,
    ContentAtoms,
    ContentRefs,
    ContentPoints,
    Links,
    Tables,
    TableRows,
    TableCells,
    FixedViews,
    FixedLines,
    Placements,
    Decorations,
    Forms,
    NameHints,
    Relations,
    ConnectionAtoms,
    AnnotationRuns,
    AnnotationMutations,
    RelationEdges,
    Diagnostics,
    TransferObjects,
    TransferEdges,
    TransferBytes,
    NestingDepth,
    IncludeDepth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredErrorKind {
    InvalidInput,
    Reentrant,
    Budget,
    BuilderAllocation,
    Native,
    InvalidResult,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredError {
    pub(crate) kind: StructuredErrorKind,
    pub(crate) stage: StructuredStage,
    pub(crate) limit: Option<StructuredLimitKind>,
    pub(crate) observed: u64,
    pub(crate) allowed: u64,
    pub(crate) message: String,
}

impl StructuredError {
    pub(crate) fn new(
        kind: StructuredErrorKind,
        stage: StructuredStage,
        limit: Option<StructuredLimitKind>,
        observed: u64,
        allowed: u64,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            stage,
            limit,
            observed,
            allowed,
            message: message.into(),
        }
    }

    fn invalid_width(width: u32) -> Self {
        Self::new(
            StructuredErrorKind::InvalidInput,
            StructuredStage::Marshal,
            None,
            u64::from(width),
            u64::from(MAX_STRUCTURED_WIDTH),
            format!(
                "structured render width must be between {MIN_STRUCTURED_WIDTH} and {MAX_STRUCTURED_WIDTH}"
            ),
        )
    }

    #[must_use]
    pub const fn kind(&self) -> StructuredErrorKind {
        self.kind
    }
    #[must_use]
    pub const fn stage(&self) -> StructuredStage {
        self.stage
    }
    #[must_use]
    pub const fn limit(&self) -> Option<StructuredLimitKind> {
        self.limit
    }
    #[must_use]
    pub const fn observed(&self) -> u64 {
        self.observed
    }
    #[must_use]
    pub const fn allowed(&self) -> u64 {
        self.allowed
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StructuredError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for StructuredError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructuredLimits {
    pub max_input_sources: u64,
    pub max_sources: u64,
    pub max_source_path_bytes: u64,
    pub max_decoded_source_bytes_per_source: u64,
    pub max_decoded_source_bytes_total: u64,
    pub max_source_map_entries: u64,
    pub max_source_map_bytes: u64,
    pub max_builder_operations: u64,
    pub max_builder_allocated_bytes: u64,
    pub max_content_bytes: u64,
    pub max_owners: u64,
    pub max_blocks: u64,
    pub max_content_atoms: u64,
    pub max_content_refs: u64,
    pub max_content_points: u64,
    pub max_links: u64,
    pub max_tables: u64,
    pub max_table_rows: u64,
    pub max_table_cells: u64,
    pub max_fixed_views: u64,
    pub max_fixed_lines: u64,
    pub max_placements: u64,
    pub max_decorations: u64,
    pub max_forms: u64,
    pub max_name_hints: u64,
    pub max_relations: u64,
    pub max_connection_atoms: u64,
    pub max_annotation_runs: u64,
    pub max_annotation_mutations: u64,
    pub max_relation_edges: u64,
    pub max_diagnostics: u64,
    pub max_transfer_objects: u64,
    pub max_transfer_edges: u64,
    pub max_transfer_bytes: u64,
    pub max_nesting_depth: u64,
    pub max_include_depth: u64,
}

impl Default for StructuredLimits {
    fn default() -> Self {
        Self {
            max_input_sources: 4_096,
            max_sources: 4_096,
            max_source_path_bytes: 4 * 1024 * 1024,
            max_decoded_source_bytes_per_source: 64 * 1024 * 1024,
            max_decoded_source_bytes_total: 256 * 1024 * 1024,
            max_source_map_entries: 4_194_304,
            max_source_map_bytes: 64 * 1024 * 1024,
            max_builder_operations: 67_108_864,
            max_builder_allocated_bytes: 256 * 1024 * 1024,
            max_content_bytes: 128 * 1024 * 1024,
            max_owners: 1_048_576,
            max_blocks: 1_048_576,
            max_content_atoms: 4_194_304,
            max_content_refs: 8_388_608,
            max_content_points: 1_048_576,
            max_links: 1_048_576,
            max_tables: 1_048_576,
            max_table_rows: 1_048_576,
            max_table_cells: 4_194_304,
            max_fixed_views: 1_048_576,
            max_fixed_lines: 1_048_576,
            max_placements: 8_388_608,
            max_decorations: 8_388_608,
            max_forms: 1_048_576,
            max_name_hints: 1_048_576,
            max_relations: 4_194_304,
            max_connection_atoms: 4_194_304,
            max_annotation_runs: 4_194_304,
            max_annotation_mutations: 16_777_216,
            max_relation_edges: 8_388_608,
            max_diagnostics: 65_536,
            max_transfer_objects: 16_777_216,
            max_transfer_edges: 16_777_216,
            max_transfer_bytes: 512 * 1024 * 1024,
            max_nesting_depth: 256,
            max_include_depth: 64,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredRenderer {
    profile: StructuredProfile,
    width: u32,
    limits: StructuredLimits,
}

impl Default for StructuredRenderer {
    fn default() -> Self {
        Self {
            profile: StructuredProfile::default(),
            width: DEFAULT_STRUCTURED_WIDTH,
            limits: StructuredLimits::default(),
        }
    }
}

impl StructuredRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn profile(&self) -> StructuredProfile {
        self.profile
    }
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }
    #[must_use]
    pub const fn limits(&self) -> &StructuredLimits {
        &self.limits
    }

    #[must_use]
    pub const fn with_profile(mut self, profile: StructuredProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_width(mut self, width: u32) -> Result<Self, StructuredError> {
        if !(MIN_STRUCTURED_WIDTH..=MAX_STRUCTURED_WIDTH).contains(&width) {
            return Err(StructuredError::invalid_width(width));
        }
        self.width = width;
        Ok(self)
    }

    #[must_use]
    pub const fn with_limits(mut self, limits: StructuredLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn render_bundle(
        &self,
        root: &str,
        bundle: &SourceBundle,
        format: InputFormat,
    ) -> Result<StructuredDocument, StructuredError> {
        crate::ffi::render_structured(root, bundle, format, self.profile, self.width, &self.limits)
    }
}

pub fn render_bundle(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
) -> Result<StructuredDocument, StructuredError> {
    StructuredRenderer::default().render_bundle(root, bundle, format)
}
