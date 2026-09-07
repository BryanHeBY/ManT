//! Operation-local validation derived from one immutable document.
use crate::{Diagnostic, Document, DocumentIndex, EntryRelationIssue};

/// Complete validation results and index for one borrowed document.
///
/// All fields are derived together; callers cannot pair cached findings with a
/// different or mutated document. This is an in-memory sidecar, not a wire DTO.
/// It does not trust parser provenance or skip checks for external producers.
///
/// A live snapshot cannot be used after mutating its source:
///
/// ```compile_fail
/// fn mutate(document: &mut mant_ir::Document) {
///     let checked = mant_ir::DocumentValidation::new(document);
///     document.blocks.clear();
///     assert!(checked.diagnostics().is_empty());
/// }
/// ```
#[derive(Debug)]
pub struct DocumentValidation<'a> {
    document: &'a Document,
    index: DocumentIndex,
    relations: Vec<EntryRelationIssue>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> DocumentValidation<'a> {
    /// Index and validate this immutable document once for the current operation.
    #[must_use]
    pub fn new(document: &'a Document) -> Self {
        let index = DocumentIndex::build(document);
        let relations = crate::entry::relation_issues(document, &index);
        let diagnostics = super::document::validate_with_index(document, &index, &relations);
        Self {
            document,
            index,
            relations,
            diagnostics,
        }
    }

    /// The exact document whose identity and invariants were checked.
    #[must_use]
    pub fn document(&self) -> &'a Document {
        self.document
    }

    /// Identities, roles, duplicates and fragments from this document.
    #[must_use]
    pub fn index(&self) -> &DocumentIndex {
        &self.index
    }

    /// Typed relationship and name-binding failures, in validation order.
    #[must_use]
    pub fn relation_issues(&self) -> &[EntryRelationIssue] {
        &self.relations
    }

    /// All shared invariant findings, excluding pre-existing producer diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Consume the sidecar and return its invariant findings without cloning.
    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}
