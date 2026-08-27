use crate::{
    AuthorMode, DiagnosticCode, DisplayKind, MacroSet, NodeKind, NodeRef, NormalizedFont,
    NormalizedListKind, Severity, Source, SourceName,
};

/// Most mdoc unit fixtures intentionally start with the construct under
/// test, commonly a `DESCRIPTION` section.  Keep their assertions focused
/// on that construct; the production parser still emits the prologue
/// warning and `first_section_validation_uses_the_visible_heading` below
/// covers it directly.
#[derive(Default)]
struct Parser(crate::Parser);

impl Parser {
    fn parse(&self, source: Source<'_>) -> Result<crate::ParseReport, crate::FatalError> {
        let mut report = self.0.parse(source)?;
        report.diagnostics.retain(|diagnostic| {
            diagnostic.code.as_str() != DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        });
        Ok(report)
    }
}

mod directives;
mod inline;
mod lists;
mod metadata;
mod recovery;
mod sections;
mod synopsis;
