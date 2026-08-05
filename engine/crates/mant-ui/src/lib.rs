//! Ratatui frontend for the renderer-neutral `ManT` document model.
//!
//! The crate is intentionally independent from command-line parsing. The final
//! `mant` process can therefore choose between interactive and structured
//! output without making the UI another process boundary.

mod app;
mod document;
mod terminal;
mod theme;

pub use app::App;
pub use document::{DocumentView, NavItem, RenderedDocument};
pub use terminal::run;
