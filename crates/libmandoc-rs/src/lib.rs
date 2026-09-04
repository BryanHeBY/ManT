#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(test)]
mod build_config;

mod ast;
mod compression;
mod diagnostics;
#[allow(unsafe_code)]
mod ffi;
mod parser;
#[cfg(feature = "render")]
mod renderer;
mod source_bundle;
mod special_character;

pub use ast::{
    AuthorMode, DisplayKind, Document, MacroSet, Metadata, Node, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, TableAlignment, TableCell,
};
pub use compression::MAX_DECOMPRESSED_SOURCE_BYTES;
pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticLevel, SourceLocation};
pub use parser::{
    Compression, IncludePolicy, InputFormat, ParseError, ParseErrorKind, ParseOptions, ParseReport,
    Parser,
};
#[cfg(feature = "render")]
pub use renderer::{
    DEFAULT_RENDER_OUTPUT_BYTES, DEFAULT_RENDER_WIDTH, MAX_RENDER_OUTPUT_BYTES, MAX_RENDER_WIDTH,
    MIN_RENDER_WIDTH, RenderError, RenderErrorKind, RenderFormat, RenderReport, Renderer,
};
pub use source_bundle::{
    MAX_SOURCE_BUNDLE_BYTES, MAX_SOURCE_BUNDLE_FILE_BYTES, MAX_SOURCE_BUNDLE_FILES, SourceBundle,
    SourceBundleError, SourceBundleErrorKind,
};
pub use special_character::{SpecialCharacter, special_character};

/// Pinned upstream version compiled by this crate's build script.
pub const LIBMANDOC_VERSION: &str = "1.14.6";

/// Private output of the FFI boundary before diagnostics become public values.
struct RawDocument {
    document: Document,
    diagnostics: String,
    node_truncated: bool,
    equation_truncated: bool,
}

#[cfg(feature = "render")]
struct RawRender {
    output: Vec<u8>,
    diagnostics: String,
}

#[cfg(test)]
mod tests;
