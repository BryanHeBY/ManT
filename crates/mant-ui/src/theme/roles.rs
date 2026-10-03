//! Presentation roles retain meaning independently of the selected palette.

use mant_ir::EntryKind;

/// Text roles and their owning surfaces. Widgets request meaning, not a hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum StyleRole {
    Text,
    Heading,
    Strong,
    Emphasis,
    InlineCode,
    Link,
    ListMarker,
    Equation,
    Unsupported,
    Metadata,
    Parameter,
    Command,
    Environment,
    Configuration,
    Variable,
    Value,
    Term,
    OutlineRoot,
    OutlineSection,
    OutlineEntries,
    OutlineReferences,
    OutlineReference,
    OutlineTerm,
    Notice,
    TldrTitle,
    TldrExample,
    TldrCommand,
    TldrFrame,
    Rule,
    CodeComment,
    CodeString,
    CodeOption,
    CodeNumber,
    CodeKeyword,
    TreeGuide,
    TreeFocus,
    TreeContinuationFocus,
    ContentSurface,
    OutlineSurface,
    CodeSurface,
    TldrSurface,
    TldrOutlineSurface,
}

impl StyleRole {
    pub(super) const COUNT: usize = Self::TldrOutlineSurface as usize + 1;

    pub(crate) const fn for_entry(kind: EntryKind) -> Self {
        match mant_render::entry_tone(kind) {
            mant_render::EntryTone::Primary => Self::Term,
            mant_render::EntryTone::Parameter => Self::Parameter,
            mant_render::EntryTone::Command => Self::Command,
            mant_render::EntryTone::Environment => Self::Environment,
            mant_render::EntryTone::Configuration => Self::Configuration,
            mant_render::EntryTone::Variable => Self::Variable,
            mant_render::EntryTone::Value => Self::Value,
        }
    }
}

/// Interaction decorations are applied after semantic/source styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum InteractionRole {
    Selection,
    OutlineSelection,
    TldrOutlineSelection,
    SearchMatch,
    SearchActive,
}

impl InteractionRole {
    pub(super) const COUNT: usize = Self::SearchActive as usize + 1;
}
