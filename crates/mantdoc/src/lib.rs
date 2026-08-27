#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Pure-Rust parser contracts shared by the `mantdoc` migration milestones.

mod ast;
mod chars;
mod diagnostic;
mod escape;
mod input;
mod limits;
#[cfg(feature = "serde")]
mod logical;
mod man;
mod mdoc;
#[allow(dead_code)] // M2 builds and tests it; M3 consumes it for roff request execution.
mod numeric;
mod parser;
mod preprocess;
#[cfg(feature = "render")]
mod renderer;
mod roff;
mod scan;
mod source;

pub use ast::{
    AuthorMode, DisplayKind, Document, MacroSet, Metadata, NodeFlags, NodeId, NodeKind, NodeRef,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, TableAlignment, TableCell,
};
pub use chars::{SpecialCharacter, special_character};
pub use diagnostic::{
    Diagnostic, DiagnosticCode, InvalidDiagnosticCode, InvalidSpan, RelatedSpan, Severity,
    SourceSpan,
};
pub use input::Compression;
pub use limits::{LimitViolation, Limits};
#[cfg(feature = "serde")]
pub use logical::{
    LOGICAL_PARSE_REPORT_SCHEMA_VERSION, LogicalDiagnostic, LogicalDocument, LogicalNode,
    LogicalParseReport, LogicalRelatedSpan, LogicalSourceSpan,
};
pub use parser::{
    FatalError, FatalErrorKind, ParseReport, ParseStatistics, Parser, ParserConfig, RecoveryMode,
    Syntax,
};
#[cfg(feature = "render")]
pub use renderer::{
    DEFAULT_RENDER_OUTPUT_BYTES, DEFAULT_RENDER_WIDTH, MAX_RENDER_OUTPUT_BYTES, MAX_RENDER_WIDTH,
    MIN_RENDER_WIDTH, RenderError, RenderErrorKind, RenderFormat, RenderReport, Renderer,
};
pub use source::{
    BundleError, BundleErrorKind, ContainedRootResolver, IncludeRequest, ResolveError,
    ResolvedSource, Source, SourceBundle, SourceId, SourceName, SourceNameError, SourcePosition,
    SourceResolver,
};

/// The M0 oracle that `mantdoc` initially compares against.
pub const LEGACY_ORACLE_ID: &str = "libmandoc-rs-0.9.1-863d2b3";
