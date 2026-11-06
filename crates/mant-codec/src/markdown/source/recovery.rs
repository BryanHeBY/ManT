//! Recover display text without silently repairing a damaged destination.
use super::MarkdownSource;
use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel};
use pulldown_cmark::{Event, LinkType, Parser, RefDefs, Tag};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

const MAX_CHANGE_RANGES: usize = 4096;

/// Source coordinates are the single prepared-source coordinate system. A
/// saturated suffix stays conservative; no unbounded per-scalar sidecar.
pub(in crate::markdown) struct Recovery<'a> {
    original: &'a str,
    changes: Vec<Range<usize>>,
    parser: Option<Parser<'a>>,
}
impl<'a> Recovery<'a> {
    pub(in crate::markdown) fn new(original: &'a str) -> Self {
        let mut changes: Vec<Range<usize>> = Vec::new();
        for (offset, character) in original.char_indices() {
            if crate::text_safety::is_safe_source_character(character) {
                continue;
            }
            let end = offset + character.len_utf8();
            if let Some(last) = changes.last_mut()
                && last.end == offset
            {
                last.end = end;
                continue;
            }
            if changes.len() == MAX_CHANGE_RANGES {
                changes.last_mut().expect("bounded nonempty ranges").end = original.len();
                break;
            }
            changes.push(offset..end);
        }
        // One bounded first pass for a damaged source, not one page parse per
        // use. Otherwise a definition erased by recovery could silently expose
        // a later shadowed definition with an unrelated target.
        let parser = (!changes.is_empty())
            .then(|| Parser::new_ext(original, crate::markdown::markdown_options()));
        Self {
            original,
            changes,
            parser,
        }
    }
    fn references(&self) -> Option<&RefDefs<'_>> {
        self.parser.as_ref().map(Parser::reference_definitions)
    }
    fn intersects(&self, range: &Range<usize>) -> bool {
        let index = self
            .changes
            .partition_point(|change| change.end <= range.start);
        self.changes
            .get(index)
            .is_some_and(|change| change.start < range.end)
    }
}

#[derive(Default)]
pub(in crate::markdown) struct TargetAdmission {
    remaining: Option<usize>,
    decisions: BTreeMap<ProofKey, bool>,
    reported: BTreeSet<(usize, usize)>,
}

/// Only unchanged uses may share an effective-definition proof. Changed uses
/// and inline destinations retain their own parser-event identity.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ProofKey {
    InlineUse((usize, usize)),
    ReferenceUse {
        use_range: (usize, usize),
        definition: Option<(usize, usize)>,
    },
    ReferenceDefinition {
        original: Option<(usize, usize)>,
        recovered: Option<(usize, usize)>,
    },
}

/// Destination equality has already been checked by the original parser.
/// Reference identity remains separate from that address proof.
enum BindingProof {
    Inline,
    Reference { definition: Range<usize> },
}

impl BindingProof {
    fn matches(&self, link: &LinkUse<'_>) -> bool {
        match self {
            Self::Inline => link.definition.is_none(),
            Self::Reference { definition } => link.definition == Some(definition),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::markdown) struct LinkUse<'a> {
    pub(in crate::markdown) link_type: LinkType,
    pub(in crate::markdown) id: &'a str,
    pub(in crate::markdown) destination: &'a str,
    pub(in crate::markdown) range: &'a Range<usize>,
    pub(in crate::markdown) definition: Option<&'a Range<usize>>,
}

impl LinkUse<'_> {
    fn proof_key(
        &self,
        is_reference: bool,
        changed_use: bool,
        original_definition: Option<&Range<usize>>,
    ) -> ProofKey {
        let definition = self.definition.map(|range| (range.start, range.end));
        if !is_reference {
            ProofKey::InlineUse((self.range.start, self.range.end))
        } else if changed_use {
            ProofKey::ReferenceUse {
                use_range: (self.range.start, self.range.end),
                definition,
            }
        } else {
            ProofKey::ReferenceDefinition {
                original: original_definition.map(|range| (range.start, range.end)),
                recovered: definition,
            }
        }
    }
}

impl TargetAdmission {
    pub(in crate::markdown) fn accept(
        &mut self,
        recovery: Option<&Recovery<'_>>,
        link: LinkUse<'_>,
        source: &MarkdownSource<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> bool {
        let Some(recovery) = recovery else {
            return true;
        };
        if recovery.changes.is_empty() {
            return true;
        }
        let original_definition = recovery
            .references()
            .and_then(|references| references.get(link.id));
        let is_reference = matches!(
            link.link_type,
            LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut
        );
        let binding_changed = is_reference
            && original_definition.map(|definition| &definition.span) != link.definition;
        let origin = if binding_changed {
            original_definition.map_or(link.definition.unwrap_or(link.range), |definition| {
                &definition.span
            })
        } else {
            link.definition.unwrap_or(link.range)
        };
        let changed_use = recovery.intersects(link.range);
        if !changed_use && !binding_changed && !recovery.intersects(origin) {
            return true;
        }
        let proof_range = if changed_use { link.range } else { origin };
        let diagnostic_key = (proof_range.start, proof_range.end);
        let key = link.proof_key(
            is_reference,
            changed_use,
            original_definition.map(|definition| &definition.span),
        );
        let accepted = if let Some(decision) = self.decisions.get(&key) {
            *decision
        } else {
            // Only damaged local metadata is inspected again with the same
            // parser, never a second Markdown lexer or one full-page parse per
            // reference. Charge failed proofs too; cache effective definitions.
            let fuel = self
                .remaining
                .get_or_insert(recovery.original.len().max(100_000));
            let cost = if is_reference {
                if changed_use { link.range.len() } else { 0 }
            } else {
                link.range.len()
            };
            let accepted = if cost <= *fuel {
                *fuel -= cost;
                original_binding(recovery, &link, changed_use, fuel)
                    .is_some_and(|proof| proof.matches(&link))
            } else {
                false
            };
            self.decisions.insert(key, accepted);
            accepted
        };
        if !accepted && self.reported.insert(diagnostic_key) {
            diagnostics.push(Diagnostic {
                impact: DiagnosticImpact::None,
                level: DiagnosticLevel::Warning,
                code: Some("markdown.recovered-link-target".into()),
                message: "retained the label without a link: source recovery changed the destination or its identity could not be proved within the parsing budget".into(),
                source: Some(source.span(proof_range)),
            });
        }
        accepted
    }
}

fn original_binding(
    recovery: &Recovery<'_>,
    link: &LinkUse<'_>,
    changed_use: bool,
    fuel: &mut usize,
) -> Option<BindingProof> {
    let original = recovery.original;
    let raw = original.get(link.definition.unwrap_or(link.range).clone())?;
    let options = crate::markdown::markdown_options();
    match link.link_type {
        LinkType::Inline | LinkType::Autolink | LinkType::Email => Parser::new_ext(raw, options)
            .into_offset_iter()
            .find_map(|(event, range)| {
                if let Event::Start(Tag::Link { dest_url, .. }) = event {
                    Some(
                        range.start == 0
                            && range.end == raw.len()
                            && dest_url.as_ref() == link.destination,
                    )
                } else {
                    None
                }
            })
            .unwrap_or(false)
            .then_some(BindingProof::Inline),
        LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut => {
            original_reference_binding(recovery, link, changed_use, fuel)
        }
        _ => None,
    }
}

fn original_reference_binding(
    recovery: &Recovery<'_>,
    link: &LinkUse<'_>,
    changed_use: bool,
    fuel: &mut usize,
) -> Option<BindingProof> {
    let original = recovery.original;
    let references = recovery.references()?;
    if !changed_use {
        let definition = references.get(link.id)?;
        return (definition.dest.as_ref() == link.destination).then(|| BindingProof::Reference {
            definition: definition.span.clone(),
        });
    }
    let use_source = original.get(link.range.clone())?;
    let options = crate::markdown::markdown_options();
    let mut original_definition = None;
    let destination_matches = {
        // Locked parse.rs::fetch_link_type_url_title supplies the
        // original use's normalized reference and complete use range.
        // Keep the actual first-pass definition identity: a different
        // definition with the same destination/title is still wrong.
        let resolve = |broken: pulldown_cmark::BrokenLink<'_>| {
            let definition = references.get(&broken.reference)?;
            if broken.span == (0..use_source.len()) {
                original_definition = Some(definition.span.clone());
            }
            if definition.dest.len() > *fuel {
                return None;
            }
            *fuel -= definition.dest.len();
            Some((definition.dest.clone(), "".into()))
        };
        Parser::new_with_broken_link_callback(use_source, options, Some(resolve))
            .into_offset_iter()
            .find_map(|(event, range)| {
                if let Event::Start(Tag::Link { dest_url, .. }) = event {
                    Some(
                        range.start == 0
                            && range.end == use_source.len()
                            && dest_url.as_ref() == link.destination,
                    )
                } else {
                    None
                }
            })
            .unwrap_or(false)
    };
    if destination_matches {
        original_definition.map(|definition| BindingProof::Reference { definition })
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
