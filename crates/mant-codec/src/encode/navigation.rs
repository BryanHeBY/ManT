//! Source-wide local destination decisions shared by Markdown renderers.

use mant_ir::{
    AMBIGUOUS_FRAGMENT_ALIAS, DOCUMENT_ROOT_ID, Diagnostic, DocumentIndex,
    OVERVIEW_IDENTITY_COLLISION,
};
use std::collections::HashSet;

mod report;
pub use report::render_scoped_report_page;

#[derive(Debug, Clone, Copy, Default)]
pub(super) enum LocalDestinations<'a> {
    #[default]
    Unrestricted,
    Indexed(&'a DocumentIndex),
    OverviewConflict,
    AliasConflict,
}

impl<'a> LocalDestinations<'a> {
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

    pub(super) fn index(self) -> Option<&'a DocumentIndex> {
        if let Self::Indexed(index) = self {
            Some(index)
        } else {
            None
        }
    }
}

/// Output-local IDs for one source of a combined Markdown page.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReportNavigation<'a> {
    pub(super) source: usize,
    pub(super) anchors: Option<&'a HashSet<String>>,
}

impl ReportNavigation<'_> {
    pub(super) fn fragment(self, original: &str) -> String {
        // Encode original bytes injectively. A literal, percent-encoded or
        // sanitized source spelling cannot alias another source or fragment.
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut fragment = format!("mant-report-{}-", self.source);
        fragment.reserve(original.len().saturating_mul(2));
        for byte in original.bytes() {
            fragment.push(char::from(HEX[usize::from(byte >> 4)]));
            fragment.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        fragment
    }

    pub(super) fn has_anchor(self, fragment: &str) -> bool {
        self.anchors
            .is_none_or(|anchors| anchors.contains(fragment))
    }

    pub(super) fn anchor(self, original: &str) -> String {
        // An inline destination can occur inside a link label. A span stays
        // zero width there and does not introduce a nested HTML hyperlink.
        format!("<span id=\"{}\"></span>", self.fragment(original))
    }
}
