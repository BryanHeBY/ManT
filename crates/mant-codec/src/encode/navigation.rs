//! Source-wide local destination decisions shared by Markdown renderers.

use mant_ir::{
    AMBIGUOUS_FRAGMENT_ALIAS, DOCUMENT_ROOT_ID, Diagnostic, DocumentIndex,
    OVERVIEW_IDENTITY_COLLISION,
};

#[derive(Debug, Clone, Copy, Default)]
pub(super) enum LocalDestinations<'a> {
    #[default]
    Unrestricted,
    Indexed(&'a DocumentIndex),
    OverviewConflict,
    AliasConflict,
}

impl LocalDestinations<'_> {
    pub(super) fn from_diagnostics(diagnostics: &[Diagnostic]) -> Self {
        let mut destinations = Self::Unrestricted;
        for finding in diagnostics {
            match finding.code.as_deref() {
                // A detached report carries no machine-readable alias spelling
                // or ownership proof. Its source may have omitted either owner.
                Some(AMBIGUOUS_FRAGMENT_ALIAS) => return Self::AliasConflict,
                Some(OVERVIEW_IDENTITY_COLLISION) => destinations = Self::OverviewConflict,
                _ => {}
            }
        }
        destinations
    }

    pub(super) fn allows(self, fragment: &str) -> bool {
        match self {
            Self::Unrestricted => true,
            Self::Indexed(index) => !index.is_fragment_ambiguous(fragment),
            Self::OverviewConflict => fragment != DOCUMENT_ROOT_ID,
            Self::AliasConflict => false,
        }
    }

    pub(super) fn is_overview_destination(self, fragment: &str) -> bool {
        fragment == DOCUMENT_ROOT_ID
            || matches!(self, Self::Indexed(index) if index.is_overview_alias(fragment))
    }

    pub(super) fn overview_markers(self) -> String {
        let aliases = match self {
            Self::Indexed(index) => Some(index),
            _ => None,
        };
        super::inline::render_html_anchors(
            std::iter::once(DOCUMENT_ROOT_ID).chain(
                aliases
                    .into_iter()
                    .flat_map(DocumentIndex::overview_aliases)
                    .map(mant_ir::FragmentAlias::as_str),
            ),
            |fragment| self.allows(fragment),
        )
    }
}
