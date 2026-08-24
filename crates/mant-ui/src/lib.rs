#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod app;
mod clipboard;
mod code;
mod document;
mod layout;
mod navigation;
mod pager;
mod scrollbar;
mod terminal;
mod text;
mod theme;
mod tldr;

pub use app::{App, UpdateOutcome};
pub use clipboard::{CopyFormat, CopyRequest};
pub use document::{DocumentView, NavKind, NavNode, RenderedDocument, RenderedSearchMatch};
pub(crate) use document::{RenderedSelection, TextPosition};
pub use pager::page_text;
pub use terminal::{
    run, run_with_catalog, run_with_catalog_and_scope, run_with_catalog_and_scope_and_copy,
};
pub use tldr::{TldrLine, TldrRole, TldrSpan, layout_tldr, render_tldr_terminal};
