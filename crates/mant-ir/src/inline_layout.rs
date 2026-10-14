//! Owner-local hard-row layout, independent of inline text and annotations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Inline;

/// Maximum exceptional row origins retained by one inline owner.
pub const MAX_INLINE_ROW_HINTS: usize = 4096;
/// Maximum magnitude of a relative row displacement, in display cells.
pub const MAX_ROW_INDENT_COLUMNS: i32 = u16::MAX as i32;

/// Horizontal placement within the available content area of a visual row.
/// Without a viewport, consumers retain the ordinary left origin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlignment {
    /// Use the row's resolved left origin.
    #[default]
    Left,
    /// Center each visual line in its available content area.
    Center,
    /// Place each visual line at the area's right edge.
    Right,
}

impl TextAlignment {
    /// Additional viewport padding; empty rows never acquire layout cells.
    #[must_use]
    pub const fn padding(self, available: usize, content_width: usize) -> usize {
        if content_width == 0 {
            return 0;
        }
        let remaining = available.saturating_sub(content_width);
        match self {
            Self::Left => 0,
            Self::Center => remaining / 2,
            Self::Right => remaining,
        }
    }
}

/// An exceptional displacement of one logical hard row within its owner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RowLayoutHint {
    /// Zero-based owner-local row; never a source or viewport coordinate.
    pub row: u32,
    /// Relative display-cell correction applied to both visual origins.
    #[schemars(range(min = -65535, max = 65535))]
    pub indent_columns: i32,
    /// Additional correction for this logical row's first visual line only.
    /// Soft-wrap continuations retain their ordinary owner/hanging origin.
    #[serde(default, skip_serializing_if = "zero_indent")]
    #[schemars(range(min = -65535, max = 65535))]
    pub first_line_indent_columns: i32,
    /// Viewport-relative placement, independent of text and source commands.
    #[serde(default, skip_serializing_if = "left_alignment")]
    pub alignment: TextAlignment,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RowLayoutHintWire {
    row: u32,
    indent_columns: i32,
    #[serde(default)]
    first_line_indent_columns: i32,
    #[serde(default)]
    alignment: TextAlignment,
}

impl<'de> Deserialize<'de> for RowLayoutHint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire: RowLayoutHintWire = deserialize_object(deserializer, "a row layout hint object")?;
        Ok(Self {
            row: wire.row,
            indent_columns: wire.indent_columns,
            first_line_indent_columns: wire.first_line_indent_columns,
            alignment: wire.alignment,
        })
    }
}

/// Optional row exceptions attached to a complete inline content root.
/// Ordinary paragraphs and structural list indentation need no row hints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InlineLayout {
    /// Strictly increasing, unique row corrections. Missing rows use zero.
    #[serde(default, skip_serializing_if = "hints_empty")]
    #[schemars(length(max = 4096))]
    #[serde(serialize_with = "serialize_hints")]
    pub row_hints: Vec<RowLayoutHint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InlineLayoutWire {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_hints")]
    row_hints: Vec<RowLayoutHint>,
}

impl<'de> Deserialize<'de> for InlineLayout {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire: InlineLayoutWire = deserialize_object(deserializer, "an inline layout object")?;
        let layout = Self {
            row_hints: wire.row_hints,
        };
        layout.validate_hints().map_err(serde::de::Error::custom)?;
        Ok(layout)
    }
}

impl InlineLayout {
    /// Whether this owner needs no exceptional row layout.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.row_hints.iter().all(RowLayoutHint::is_empty)
    }

    /// Check the wire budget, ordering, displacements and addressed rows.
    ///
    /// # Errors
    /// Returns an error if a hint cannot address this exact content root.
    pub fn validate(&self, content: &[Inline]) -> Result<(), &'static str> {
        self.validate_hints()?;
        if self.row_hints.last().is_some_and(|hint| {
            usize::try_from(hint.row).map_or(true, |row| row >= logical_row_count(content))
        }) {
            return Err("row layout hint exceeds its inline owner's logical rows");
        }
        Ok(())
    }

    fn validate_hints(&self) -> Result<(), &'static str> {
        if self.row_hints.len() > MAX_INLINE_ROW_HINTS {
            return Err("too many inline row layout hints");
        }
        if self
            .row_hints
            .windows(2)
            .any(|pair| pair[0].row >= pair[1].row)
        {
            return Err("inline row layout hints must be unique and increasing");
        }
        if self.row_hints.iter().any(|hint| {
            [hint.indent_columns, hint.first_line_indent_columns]
                .iter()
                .any(|value| !(-MAX_ROW_INDENT_COLUMNS..=MAX_ROW_INDENT_COLUMNS).contains(value))
        }) {
            return Err("inline row displacement exceeds the accepted range");
        }
        Ok(())
    }

    /// Read a row's correction without changing any preceding row's origin.
    #[must_use]
    pub fn row_indent(&self, row: usize) -> i32 {
        self.row_layout(row).indent_columns
    }

    /// Read all presentation corrections for one logical row.
    #[must_use]
    pub fn row_layout(&self, row: usize) -> RowLayoutHint {
        u32::try_from(row)
            .ok()
            .and_then(|row| {
                self.row_hints
                    .binary_search_by_key(&row, |hint| hint.row)
                    .ok()
                    .map(|index| self.row_hints[index])
            })
            .unwrap_or_default()
    }
}

/// Borrowed content and layout of one complete owner, without copying text.
#[derive(Clone, Copy, Debug)]
pub struct InlineContentRef<'a> {
    /// The owner's authoritative inline content, including annotations.
    pub content: &'a [Inline],
    /// Sparse row corrections in this owner's logical coordinate space.
    pub layout: &'a InlineLayout,
}

impl<'a> InlineContentRef<'a> {
    /// Borrow text without any owner-local layout exceptions.
    #[must_use]
    pub const fn unpositioned(content: &'a [Inline]) -> Self {
        static EMPTY: InlineLayout = InlineLayout {
            row_hints: Vec::new(),
        };
        Self {
            content,
            layout: &EMPTY,
        }
    }

    /// Rebase row hints for a checked scalar selection attached to a new root.
    /// The returned layout never adds text or changes source coordinates.
    #[must_use]
    pub fn sliced_layout(self, chars: std::ops::Range<usize>) -> Option<InlineLayout> {
        if chars.start > chars.end {
            return None;
        }
        let mut scalar = 0usize;
        let mut row = 0u32;
        let mut first_row = 0u32;
        let mut last_row = 0u32;
        for text in text_chunks(self.content) {
            for character in text.chars() {
                if scalar >= chars.end {
                    break;
                }
                row = row.saturating_add(u32::from(character == '\n'));
                scalar += 1;
                if scalar <= chars.start {
                    first_row = row;
                }
                last_row = row;
            }
            if scalar >= chars.end {
                break;
            }
        }
        if scalar < chars.end {
            return None;
        }
        Some(InlineLayout {
            row_hints: self
                .layout
                .row_hints
                .iter()
                .filter(|hint| (first_row..=last_row).contains(&hint.row))
                .map(|hint| RowLayoutHint {
                    row: hint.row - first_row,
                    ..*hint
                })
                .collect(),
        })
    }
}

/// A definition label with its own local row layout and authoritative text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionTerm {
    /// Original styled label content; layout is never inserted as text.
    pub content: Vec<Inline>,
    /// Optional owner-local row layout.
    #[serde(default, skip_serializing_if = "InlineLayout::is_empty")]
    pub inline_layout: InlineLayout,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefinitionTermWire {
    content: Vec<Inline>,
    #[serde(default)]
    inline_layout: InlineLayout,
}

impl<'de> Deserialize<'de> for DefinitionTerm {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire: DefinitionTermWire =
            deserialize_object(deserializer, "a definition term object")?;
        wire.inline_layout
            .validate(&wire.content)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            content: wire.content,
            inline_layout: wire.inline_layout,
        })
    }
}

impl DefinitionTerm {
    /// Borrow label text without applying its optional presentation layout.
    #[must_use]
    pub fn as_slice(&self) -> &[Inline] {
        &self.content
    }
    /// Borrow this complete label root and its local layout.
    #[must_use]
    pub fn inline_content(&self) -> InlineContentRef<'_> {
        InlineContentRef {
            content: &self.content,
            layout: &self.inline_layout,
        }
    }
}

impl From<Vec<Inline>> for DefinitionTerm {
    fn from(content: Vec<Inline>) -> Self {
        Self {
            content,
            inline_layout: InlineLayout::default(),
        }
    }
}

impl AsRef<[Inline]> for DefinitionTerm {
    fn as_ref(&self) -> &[Inline] {
        &self.content
    }
}

impl FromIterator<Inline> for DefinitionTerm {
    fn from_iter<T: IntoIterator<Item = Inline>>(iter: T) -> Self {
        iter.into_iter().collect::<Vec<_>>().into()
    }
}

impl std::ops::Deref for DefinitionTerm {
    type Target = [Inline];
    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

impl<'a> IntoIterator for &'a DefinitionTerm {
    type Item = &'a Inline;
    type IntoIter = std::slice::Iter<'a, Inline>;
    fn into_iter(self) -> Self::IntoIter {
        self.content.iter()
    }
}

/// Distinct origins of the first visual line and its soft-wrap continuations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowOrigins {
    /// Origin of this logical row's first visual line.
    pub first_visual_origin: i32,
    /// Origin of subsequent visual lines of this same logical row.
    pub continuation_origin: i32,
}

/// Compose both visual origins with the same owner-local row correction.
/// Clipping is left to the final display leaf; hanging layout stays distinct.
#[must_use]
pub const fn resolve_row_origins(first: i32, continuation: i32, correction: i32) -> RowOrigins {
    RowOrigins {
        first_visual_origin: first.saturating_add(correction),
        continuation_origin: continuation.saturating_add(correction),
    }
}

/// Number of addressable logical row positions, including the open tail.
/// Empty roots and anchors have position zero without producing a blank row.
#[must_use]
pub fn logical_row_count(content: &[Inline]) -> usize {
    text_chunks(content).fold(1usize, |count, text| {
        count.saturating_add(text.bytes().filter(|byte| *byte == b'\n').count())
    })
}

fn text_chunks(content: &[Inline]) -> impl Iterator<Item = &str> {
    let mut stack = vec![content.iter()];
    std::iter::from_fn(move || {
        loop {
            let Some(node) = stack.last_mut()?.next() else {
                stack.pop();
                continue;
            };
            match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => return Some(value.as_str()),
                Inline::LineBreak {} => return Some("\n"),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => stack.push(children.iter()),
                Inline::Anchor { .. } => {}
            }
        }
    })
}

fn hints_empty(hints: &[RowLayoutHint]) -> bool {
    hints.iter().all(RowLayoutHint::is_empty)
}

impl RowLayoutHint {
    /// Resolve first and continued visual origins without clipping.
    #[must_use]
    pub const fn resolve(self, first: i32, continuation: i32) -> RowOrigins {
        RowOrigins {
            first_visual_origin: first
                .saturating_add(self.indent_columns)
                .saturating_add(self.first_line_indent_columns),
            continuation_origin: continuation.saturating_add(self.indent_columns),
        }
    }

    /// Whether this row adds no presentation preference.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indent_columns == 0
            && self.first_line_indent_columns == 0
            && self.alignment == TextAlignment::Left
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde skip predicates borrow fields.
const fn zero_indent(value: &i32) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde skip predicates borrow fields.
fn left_alignment(value: &TextAlignment) -> bool {
    *value == TextAlignment::Left
}

/// Restrict these new owner-layout wire types to JSON objects while retaining
/// their derived closed-field decoding and the caller's semantic validation.
fn deserialize_object<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
    expected: &'static str,
) -> Result<T, D::Error> {
    struct Object<T> {
        expected: &'static str,
        value: std::marker::PhantomData<T>,
    }

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Object<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.expected)
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(Object {
        expected,
        value: std::marker::PhantomData,
    })
}

fn serialize_hints<S: serde::Serializer>(
    hints: &[RowLayoutHint],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut sequence = serializer.serialize_seq(None)?;
    for hint in hints.iter().filter(|hint| !hint.is_empty()) {
        sequence.serialize_element(hint)?;
    }
    sequence.end()
}

fn deserialize_hints<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<RowLayoutHint>, D::Error> {
    struct Hints;
    impl<'de> serde::de::Visitor<'de> for Hints {
        type Value = Vec<RowLayoutHint>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bounded inline row layout hints")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut hints =
                Vec::with_capacity(seq.size_hint().unwrap_or(0).min(MAX_INLINE_ROW_HINTS));
            while let Some(hint) = seq.next_element()? {
                if hints.len() == MAX_INLINE_ROW_HINTS {
                    return Err(serde::de::Error::custom("too many inline row layout hints"));
                }
                hints.push(hint);
            }
            Ok(hints)
        }
    }
    deserializer.deserialize_seq(Hints)
}

#[cfg(test)]
mod tests;
