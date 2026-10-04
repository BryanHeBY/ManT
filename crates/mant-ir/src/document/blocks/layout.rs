//! Resolved block origins and leading boundary geometry.
use super::super::is_zero_u16;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Presentation hints retained from roff but optional for semantic outputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutHint {
    /// Signed terminal-cell displacement from the actual parent's content
    /// origin. Compose once before clamping a final display position; negative
    /// children can outdent without erasing their parent's source geometry.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub indent_columns: i32,
    /// Additional displacement for paragraph continuation lines, relative to
    /// the first-line origin. Applies to hard breaks and visual wraps, not to
    /// later sibling blocks; zero preserves ordinary filled flow.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub continuation_indent_columns: i32,
    /// Resolved blank rows owned by this block's leading boundary. Zero means
    /// tight spacing, not inheritance. Producers resolve source/Markdown
    /// defaults and assign each request exactly one consumption point.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
}

impl LayoutHint {
    /// Return whether the hint requests no additional layout behavior.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.indent_columns == 0
            && self.continuation_indent_columns == 0
            && self.spacing_before_lines == 0
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde skip predicates receive a reference.
const fn is_zero_i32(value: &i32) -> bool {
    *value == 0
}
