//! Ordinary list markers, source owners and item boundary facts.
use super::{Block, SourceSpan};
use crate::EntryFacts;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Marker behavior of an ordinary list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ListKind {
    /// Unordered list with bullets.
    Bullet,
    /// Unordered list with dash markers.
    Dash,
    /// Ordered list with ordinal markers.
    Ordered {
        /// First ordinal, or unknown. Renderers use one for an unknown start
        /// without changing the source fact. Zero and `u64::MAX` are valid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<u64>,
    },
    /// Marker-free list.
    Plain,
}

// Empty struct variants enforce closure even for bullet/plain during real
// Serde decoding. Unit variants alone can discard unknown tagged fields.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ClosedListKind {
    Bullet {},
    Dash {},
    Ordered {
        #[serde(default)]
        start: Option<u64>,
    },
    Plain {},
}

impl<'de> Deserialize<'de> for ListKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ClosedListKind::deserialize(deserializer)? {
            ClosedListKind::Bullet {} => Self::Bullet,
            ClosedListKind::Dash {} => Self::Dash,
            ClosedListKind::Ordered { start } => Self::Ordered { start },
            ClosedListKind::Plain {} => Self::Plain,
        })
    }
}

impl ListKind {
    /// Displayed ordinal for a zero-based item, saturating at `u64::MAX`.
    /// Non-ordered lists have no ordinal. Unknown starts display from one.
    #[must_use]
    pub fn ordinal(self, index: usize) -> Option<u64> {
        match self {
            Self::Ordered { start } => Some(
                start
                    .unwrap_or(1)
                    .saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
            ),
            Self::Bullet | Self::Dash | Self::Plain => None,
        }
    }

    /// Kind of an excerpt starting at a zero-based original item. The returned
    /// ordered kind records its effective ordinal; the source remains unchanged.
    #[must_use]
    pub fn for_excerpt(self, index: usize) -> Self {
        self.ordinal(index)
            .map_or(self, |start| Self::Ordered { start: Some(start) })
    }
}

/// A list item contains blocks so nested lists and displays remain intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListItem {
    /// Item boundary geometry, independent of content and semantic facts.
    #[serde(default, skip_serializing_if = "ListItemLayout::is_empty")]
    pub layout: ListItemLayout,
    /// Original item span, independent of any removed declaration or first
    /// visible block. Unknown for synthetic content; never an identity key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    /// Optional semantic facts; these never replace or render the item body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts>,
    /// Arbitrary item content in source order.
    pub blocks: Vec<Block>,
}

/// Resolved list-item boundary. Absent spacing inherits list compactness;
/// explicit zero is tight, while larger requests precede the whole marker and
/// body, including items beginning with displays or other non-paragraph blocks.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListItemLayout {
    /// Blank rows before this item; missing/null inherits list compactness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

impl ListItemLayout {
    /// Whether the layout only inherits the containing list's defaults.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.spacing_before_lines.is_none()
    }
}
