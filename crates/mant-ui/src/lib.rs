#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod app;
mod code;
mod document;
mod layout;
mod navigation;
mod pager;
mod scrollbar;
mod terminal;
mod theme;
mod tldr;

pub use app::{App, UpdateOutcome};
pub use document::{DocumentView, NavKind, NavNode, RenderedDocument, RenderedSearchMatch};
pub use pager::page_text;
pub use terminal::{run, run_with_catalog};
pub use tldr::{TldrLine, TldrRole, TldrSpan, layout_tldr, render_tldr_terminal};
