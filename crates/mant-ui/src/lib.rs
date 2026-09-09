#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod app;
mod clipboard;
mod code;
mod document;
mod layout;
mod navigation;
mod reader;
mod scrollbar;
mod text;
mod theme;

pub use app::{App, UpdateOutcome};
pub use clipboard::{CopyFormat, CopyRequest, MAX_COPY_BYTES};
pub use document::{
    DocumentView, ExternalUri, NavKind, NavNode, RenderedDocument, RenderedSearchMatch,
};
pub(crate) use document::{RenderedSelection, TextPosition};
pub use reader::{CopyToClipboard, DiscoverDocuments, OpenDocument, OpenExternal, ReaderServices};
