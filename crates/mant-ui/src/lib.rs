#![doc = include_str!("../README.md")]

mod app;
mod code;
mod document;
mod layout;
mod navigation;
mod scrollbar;
mod terminal;
mod theme;
mod tldr;

pub use app::{App, UpdateOutcome};
pub use document::{DocumentView, NavItem, NavKind, RenderedDocument, RenderedSearchMatch};
pub use terminal::{run, run_with_catalog};
pub use tldr::{TldrLine, TldrRole, TldrSpan, layout_tldr, render_tldr_terminal};
