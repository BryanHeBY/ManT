//! Private syntax decoration over unchanged inline content.
//!
//! Source projection, context-specific recognition and ordered range composition
//! share original coordinates. Theme resolution and viewport layout stay with
//! their existing consumers; no highlighting state enters document IR.

mod code;
pub(crate) mod entry;
mod ranges;
pub(crate) mod source;
mod usage;

pub(crate) use code::{CodeHighlights, HighlightContext, MAX_BLOCK_BYTES};
