//! Borrowed owner access at the Flow/Fixed semantic-index boundary.
//!
//! Neither arm owns an alternate body. Flow references resolve against their
//! content context; Fixed references are admitted only after the final display
//! selection and native role pass `FixedBody::validated_entry`.

use crate::{
    ContentContext, ContentReadError, EntryFacts, EntryOwner, FixedBody, Inline, InlineView,
    OwnerMark, SemanticDocumentTarget, SemanticEntry, SourceSpan, TextSelection, ValueDomain,
};

/// One borrowed owner; semantic projection checks body-specific references.
#[derive(Clone, Copy)]
pub struct EntryOwnerView<'a>(OwnerBackend<'a>);

#[derive(Clone, Copy)]
enum OwnerBackend<'a> {
    /// Original Flow item and its matching content store.
    Flow {
        /// Existing Flow owner, not a reconstructed item.
        owner: EntryOwner<'a>,
        /// Store used to validate visible forms and name bindings.
        content: ContentContext<'a>,
    },
    /// Native mark and its matching final display surface.
    Fixed {
        /// Checked native owner mark.
        owner: &'a OwnerMark,
        /// Surface containing all referenced display slices.
        body: &'a FixedBody,
        /// Facts validated against this mark and surface at construction.
        facts: &'a EntryFacts<TextSelection>,
    },
}

impl<'a> EntryOwnerView<'a> {
    /// Borrow a Flow item and the context used to resolve its content.
    #[must_use]
    pub const fn flow(owner: EntryOwner<'a>, content: ContentContext<'a>) -> Self {
        Self(OwnerBackend::Flow { owner, content })
    }

    /// Borrow a Fixed mark only when its facts close against final display.
    #[must_use]
    pub fn fixed(body: &'a FixedBody, owner: &'a OwnerMark) -> Option<Self> {
        let index = usize::try_from(owner.key.get() - 1).ok()?;
        if !std::ptr::eq(body.owners.get(index)?, owner) {
            return None;
        }
        let facts = body.validated_entry(owner)?;
        Some(Self(OwnerBackend::Fixed { owner, body, facts }))
    }

    /// Authored owner source, never inferred from displayed glyphs.
    #[must_use]
    pub const fn source(self) -> Option<SourceSpan> {
        match self.0 {
            OwnerBackend::Flow { owner, .. } => owner.source(),
            OwnerBackend::Fixed { owner, .. } => owner.source,
        }
    }

    /// Materialize only the index metadata, after source-specific validation.
    ///
    /// Traversal and parentage remain with the original body's owner graph.
    /// No body text is copied into the resulting entry.
    ///
    /// # Errors
    /// Returns [`ContentReadError`] if Flow references fail to resolve in the
    /// supplied store. A Fixed mark failing its final-display checks is omitted.
    pub fn semantic_entry(self) -> Result<Option<SemanticEntry>, ContentReadError> {
        match self.0 {
            OwnerBackend::Flow { owner, content } => flow_entry(owner, content),
            OwnerBackend::Fixed { body, facts, .. } => Ok(fixed_entry(facts, body)),
        }
    }
}

fn flow_entry(
    owner: EntryOwner<'_>,
    content: ContentContext<'_>,
) -> Result<Option<SemanticEntry>, ContentReadError> {
    let Some(identity) = owner.facts() else {
        return Ok(None);
    };
    let forms = content.entry_forms(owner)?.unwrap_or_default();
    let value_domain = identity.value_domain.clone().or_else(|| {
        owner
            .has_value_choices()
            .then_some(ValueDomain::Choices { exhaustive: false })
    });
    Ok(Some(SemanticEntry {
        id: identity.id.clone(),
        kind: identity.kind,
        names: content
            .entry_validated_names(owner)?
            .unwrap_or_default()
            .to_vec(),
        alias_groups: if identity.alias_groups.is_empty() {
            Vec::new()
        } else {
            content
                .entry_validated_alias_groups(owner)?
                .unwrap_or_default()
                .to_vec()
        },
        alias_of: identity.alias_of.clone(),
        case: identity.case,
        forms: forms
            .iter()
            .map(|form| content.plain_text(form))
            .collect::<Result<Vec<_>, _>>()?,
        document_targets: document_targets(content, &forms)?,
        children: Vec::new(),
        value_domain,
    }))
}

fn fixed_entry(facts: &EntryFacts<TextSelection>, body: &FixedBody) -> Option<SemanticEntry> {
    let forms = facts
        .forms
        .iter()
        .map(|form| body.selection_text(form))
        .collect::<Option<Vec<_>>>()?;
    Some(SemanticEntry {
        id: facts.id.clone(),
        kind: facts.kind,
        names: facts.names.clone(),
        alias_groups: facts.alias_groups.clone(),
        alias_of: facts.alias_of.clone(),
        case: facts.case,
        forms,
        document_targets: Vec::new(),
        children: Vec::new(),
        value_domain: facts.value_domain.clone(),
    })
}

fn document_targets(
    content: ContentContext<'_>,
    terms: &crate::EntryForms<'_>,
) -> Result<Vec<SemanticDocumentTarget>, ContentReadError> {
    let mut targets = Vec::new();
    for term in terms.iter() {
        collect_document_targets(content, term, &mut targets)?;
    }
    Ok(targets)
}

fn collect_document_targets(
    content: ContentContext<'_>,
    inlines: &[Inline],
    output: &mut Vec<SemanticDocumentTarget>,
) -> Result<(), ContentReadError> {
    for inline in inlines {
        match content.inline(inline)? {
            InlineView::Link(link)
                if crate::DocumentReference::from_link_target(link.target()).is_some() =>
            {
                let candidate = SemanticDocumentTarget {
                    label: content.plain_text(link.children())?,
                    reference: crate::DocumentReference::from_link_target(link.target())
                        .expect("the match guard accepts a document reference"),
                };
                if !output.contains(&candidate) {
                    output.push(candidate);
                }
            }
            InlineView::Strong(children) | InlineView::Emphasis(children) => {
                collect_document_targets(content, children, output)?;
            }
            InlineView::Link(link) => {
                collect_document_targets(content, link.children(), output)?;
            }
            InlineView::Text(_)
            | InlineView::Code(_)
            | InlineView::Anchor(_)
            | InlineView::LineBreak => {}
        }
    }
    Ok(())
}
