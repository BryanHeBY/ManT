//! Typed rejection reasons shared by entry recognition and completeness.
use mant_ir::{Diagnostic, DiagnosticLevel, SourceSpan};
#[derive(Debug, Clone, Copy)]
pub(super) enum EntryRejectionReason {
    MissingLeadingParagraph,
    MissingLeadingCode,
    UnsupportedOptionPrefix,
    InvalidOptionName,
    InvalidEntryName,
    InvalidPlaceholder,
    InvalidAliasSeparator,
    MissingDescription,
    UnsupportedInline,
}

impl EntryRejectionReason {
    const ALL: [Self; 9] = [
        Self::MissingLeadingParagraph,
        Self::MissingLeadingCode,
        Self::UnsupportedOptionPrefix,
        Self::InvalidOptionName,
        Self::InvalidEntryName,
        Self::InvalidPlaceholder,
        Self::InvalidAliasSeparator,
        Self::MissingDescription,
        Self::UnsupportedInline,
    ];

    const fn code(self) -> &'static str {
        match self {
            Self::MissingLeadingParagraph => "markdown.semantic-entry.missing-leading-paragraph",
            Self::MissingLeadingCode => "markdown.semantic-entry.missing-leading-code",
            Self::UnsupportedOptionPrefix => "markdown.semantic-entry.unsupported-option-prefix",
            Self::InvalidOptionName => "markdown.semantic-entry.invalid-option-name",
            Self::InvalidEntryName => "markdown.semantic-entry.invalid-entry-name",
            Self::InvalidPlaceholder => "markdown.semantic-entry.invalid-placeholder",
            Self::InvalidAliasSeparator => "markdown.semantic-entry.invalid-alias-separator",
            Self::MissingDescription => "markdown.semantic-entry.missing-description",
            Self::UnsupportedInline => "markdown.semantic-entry.unsupported-inline",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::MissingLeadingParagraph => "item must start with a paragraph",
            Self::MissingLeadingCode => "item must start with a code term",
            Self::UnsupportedOptionPrefix => "option term uses an unsupported prefix",
            Self::InvalidOptionName => "option term has an invalid name",
            Self::InvalidEntryName => "entry term has an invalid name",
            Self::InvalidPlaceholder => "option term has an invalid placeholder",
            Self::InvalidAliasSeparator => {
                "entry aliases must be separated by whitespace, ',', '/', or '|'"
            }
            Self::MissingDescription => {
                "entry term must be followed by a ':' or dash description delimiter"
            }
            Self::UnsupportedInline => "entry term contains an unsupported inline construct",
        }
    }
}

pub(crate) fn is_semantic_entry_rejection_code(code: &str) -> bool {
    code == "markdown.semantic-entry-list"
        || code == "markdown.semantic-entry-metadata"
        || code == "markdown.semantic-value-domain"
        || EntryRejectionReason::ALL
            .iter()
            .any(|reason| reason.code() == code)
}

#[derive(Debug, Clone)]
pub(super) struct EntryRejection {
    reason: EntryRejectionReason,
    term: Option<String>,
    source: Option<SourceSpan>,
}

impl EntryRejection {
    pub(super) fn new(
        reason: EntryRejectionReason,
        term: Option<&str>,
        source: Option<SourceSpan>,
    ) -> Self {
        Self {
            reason,
            term: term.map(ToOwned::to_owned),
            source,
        }
    }

    pub(super) fn emit(self, diagnostics: &mut Vec<Diagnostic>, fallback: SourceSpan) {
        let subject = self.term.as_deref().map_or_else(
            || "semantic-entry item".to_owned(),
            |term| format!("semantic-entry term '{term}'"),
        );
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some(self.reason.code().to_owned()),
            message: format!(
                "{subject} is invalid: {}; the declared list was left unchanged",
                self.reason.message()
            ),
            source: Some(self.source.unwrap_or(fallback)),
        });
    }
}
