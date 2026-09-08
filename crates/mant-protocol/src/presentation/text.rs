//! Composable source and report roles; never serialized as document content.
use super::InlinePresentation;
use crate::EvidenceClass;
use mant_ir::EntryKind;

/// Content/metadata layer, independent of source markup or a query match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextRole {
    /// Ordinary primary text.
    #[default]
    Body,
    /// Original definition term, whether or not it has semantic names.
    DefinitionTerm,
    /// Document label.
    Document,
    /// Source section title.
    Heading,
    /// A semantic outline/title label, not a body name binding.
    EntryLabel(EntryKind),
    /// Structural coordinate or opaque ID.
    Coordinate,
    /// Logical document or node path.
    Path,
    /// Tree connectors and report boundaries.
    Guide,
    /// Generated metadata label or count.
    Metadata,
    /// A visible limitation, rejection or omission.
    Notice,
    /// Why the returned evidence belongs to this section of a report.
    EvidenceClass(EvidenceClass),
}

/// A complete span style before an output adapter chooses colors.
/// Match is orthogonal to type; interactive selection is applied later by a UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextPresentation {
    /// Content or generated-metadata role.
    pub role: TextRole,
    /// Original inline modifiers and validated name kind.
    pub inline: InlinePresentation,
    /// Only a location reported by the query collector may set this flag.
    pub matched: bool,
}

impl From<TextRole> for TextPresentation {
    fn from(role: TextRole) -> Self {
        Self {
            role,
            ..Self::default()
        }
    }
}
