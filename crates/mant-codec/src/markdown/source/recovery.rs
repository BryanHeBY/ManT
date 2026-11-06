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
    decisions: BTreeMap<(usize, usize), bool>,
    reported: BTreeSet<(usize, usize)>,
}

#[derive(Clone, Copy)]
pub(in crate::markdown) struct LinkUse<'a> {
    pub(in crate::markdown) link_type: LinkType,
    pub(in crate::markdown) id: &'a str,
    pub(in crate::markdown) destination: &'a str,
    pub(in crate::markdown) range: &'a Range<usize>,
    pub(in crate::markdown) definition: Option<&'a Range<usize>>,
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
        let key = (proof_range.start, proof_range.end);
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
                original_destination_matches(recovery, &link, changed_use, fuel)
            } else {
                false
            };
            self.decisions.insert(key, accepted);
            accepted
        };
        if !accepted && self.reported.insert(key) {
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

fn original_destination_matches(
    recovery: &Recovery<'_>,
    link: &LinkUse<'_>,
    changed_use: bool,
    fuel: &mut usize,
) -> bool {
    let original = recovery.original;
    let Some(raw) = original.get(link.definition.unwrap_or(link.range).clone()) else {
        return false;
    };
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
            .unwrap_or(false),
        LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut => {
            let Some(references) = recovery.references() else {
                return false;
            };
            if !changed_use {
                return references
                    .get(link.id)
                    .is_some_and(|definition| definition.dest.as_ref() == link.destination);
            }
            let Some(use_source) = original.get(link.range.clone()) else {
                return false;
            };
            // The original definition is the resolver: a repaired reference ID
            // cannot prove itself against the recovered document's bindings.
            let resolve = |broken: pulldown_cmark::BrokenLink<'_>| {
                let definition = references.get(&broken.reference)?;
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
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_budget_and_source_change_storage_are_bounded() {
        let original = "[a\x1bb](https://a.test)";
        let recovery = Recovery::new(original);
        let source = MarkdownSource::new(original);
        let mut diagnostics = Vec::new();
        let mut admission = TargetAdmission {
            remaining: Some(0),
            ..TargetAdmission::default()
        };
        let range = 0..original.len();
        let link = || LinkUse {
            link_type: LinkType::Inline,
            id: "",
            destination: "https://a.test",
            range: &range,
            definition: None,
        };
        assert!(!admission.accept(Some(&recovery), link(), &source, &mut diagnostics));
        let mut normal = TargetAdmission::default();
        assert!(normal.accept(Some(&recovery), link(), &source, &mut diagnostics));
        let large = "x\x1b".repeat(10_000);
        let changes = Recovery::new(&large);
        assert_eq!(changes.changes.len(), MAX_CHANGE_RANGES);
        assert!(changes.intersects(&(large.len() - 1..large.len())));
    }

    #[test]
    fn callback_charges_the_original_reference_destination_before_copying() {
        let use_ = "[x][r\x1bf]";
        let original = format!(
            "{use_}\n\n[r f]: https://short.test\n[r\x1bf]: https://long.test/{}\n",
            "&amp;".repeat(1024)
        );
        let recovery = Recovery::new(&original);
        let source = MarkdownSource::new(&original);
        let range = 0..use_.len();
        let definition = original.find("[r f]:").unwrap()..original.find("\n[r\x1bf]:").unwrap();
        let mut admission = TargetAdmission {
            remaining: Some(100),
            ..TargetAdmission::default()
        };
        let mut diagnostics = Vec::new();
        assert!(!admission.accept(
            Some(&recovery),
            LinkUse {
                link_type: LinkType::Reference,
                id: "r f",
                destination: "https://short.test",
                range: &range,
                definition: Some(&definition)
            },
            &source,
            &mut diagnostics
        ));
        // Only source parsing was affordable. The larger original target was
        // refused before clone, without charging the recovered short target.
        assert_eq!(admission.remaining, Some(100 - use_.len()));
    }
}
