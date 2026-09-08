//! One DTO-only decision shared by Forms and body presentation.
use mant_ir::{Block, EntryOwner};
use mant_protocol::{EvidenceClass, ExplanationContent, ExplanationEvidence};

pub(super) struct DefinitionDisplay<'a> {
    pub(super) block: &'a Block,
    owner: Option<EntryOwner<'a>>,
}

impl<'a> DefinitionDisplay<'a> {
    pub(super) fn new(evidence: &'a ExplanationEvidence) -> Option<Self> {
        if !matches!(
            evidence.class,
            EvidenceClass::DirectEntry | EvidenceClass::RelatedEntry
        ) {
            return None;
        }
        let content = evidence.content.as_ref()?;
        let (ExplanationContent::Entry { block } | ExplanationContent::Block { block }) = content
        else {
            return None;
        };
        let owner = if evidence.content_omitted
            || !matches!(content, ExplanationContent::Entry { .. })
        {
            None
        } else {
            match block {
                Block::DefinitionList { items, .. } if items.len() == 1 => {
                    Some(EntryOwner::Definition(&items[0]))
                }
                Block::List { items, .. } if items.len() == 1 => Some(EntryOwner::List(&items[0])),
                _ => None,
            }
            .filter(|owner| complete_forms(*owner, evidence))
        };
        Some(Self { block, owner })
    }

    pub(super) fn includes_forms(&self) -> bool {
        self.owner.is_some()
    }

    pub(super) fn empty_description(&self) -> bool {
        matches!(self.owner, Some(EntryOwner::Definition(item)) if item.description.is_empty())
    }
}

fn complete_forms(owner: EntryOwner<'_>, evidence: &ExplanationEvidence) -> bool {
    let Some(facts) = owner.facts() else {
        return false;
    };
    if facts.id.as_str() != evidence.outline.node.id() {
        return false;
    }
    let Some(forms) = owner.forms() else {
        return false;
    };
    if forms.iter().next().is_none()
        || forms
            .iter()
            .any(|form| crate::inline::plain_text(form).trim().is_empty())
    {
        return false;
    }
    // Exact projected IR, not a fuzzy/string-similarity deduplication. A DTO
    // whose terms were removed or cropped must keep its separately returned
    // Forms. Detail-budget omission does not imply body omission.
    evidence.entry.as_ref().is_none_or(|entry| {
        entry.forms.is_empty() || forms.iter().eq(entry.forms.iter().map(Vec::as_slice))
    })
}
