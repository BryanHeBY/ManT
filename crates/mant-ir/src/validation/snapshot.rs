//! Operation-local validation derived from one immutable document.
use crate::{
    Diagnostic, Document, DocumentBodyRef, DocumentIndex, EntryRelationIssue, FixedBody,
    FixedBodyError,
};

/// A proof for one immutable Fixed body, scoped to one document operation.
/// A successful full validation already checked every present entry fact.
pub(crate) struct FixedBodyValidation<'a> {
    body: &'a FixedBody,
    result: Result<(), FixedBodyError>,
}

impl<'a> FixedBodyValidation<'a> {
    fn new(body: &'a FixedBody) -> Self {
        Self {
            body,
            result: body.validate(),
        }
    }

    pub(crate) fn result_for(&self, body: &FixedBody) -> Option<&Result<(), FixedBodyError>> {
        std::ptr::eq(self.body, body).then_some(&self.result)
    }
}

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
///     document.flow_mut().unwrap().blocks.clear();
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
        let fixed_validation = match document.body() {
            DocumentBodyRef::Fixed(fixed) => Some(FixedBodyValidation::new(fixed)),
            DocumentBodyRef::Flow(_) => None,
        };
        let index = DocumentIndex::build_with_fixed_validation(document, fixed_validation.as_ref());
        let relations = match document.body() {
            DocumentBodyRef::Flow(_) => {
                crate::entry::relation_issues(document, document.content(), &index)
            }
            // Fixed currently validates its conservative head facts in the
            // surface/mark boundary. Explicit relation traversal is still a
            // Flow-only policy until the shared-owner R04 migration lands.
            DocumentBodyRef::Fixed(_) => Vec::new(),
        };
        let diagnostics = super::document::validate_with_index(
            document,
            &index,
            &relations,
            fixed_validation.as_ref(),
        );
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
