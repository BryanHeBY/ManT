//! DTO-only, operation-local decoration. No query strings or document access.
mod markdown;
use mant_ir::{EntryKind, Inline};
use mant_protocol::{
    EvidenceBasis, ExplanationEvidence, ExplanationOccurrence, ExplanationTextRoot,
    InlinePresentation, TextPresentation, TextRole, visit_inline_text,
};
use std::{collections::BTreeMap, marker::PhantomData, ops::Range};

#[derive(Clone)]
struct Span {
    chars: Range<usize>,
    kind: Option<EntryKind>,
    matched: bool,
}

/// Keys identify borrowed returned roots, never spellings shared by owners.
#[derive(Default)]
pub(crate) struct LocatedStyles<'a> {
    roots: BTreeMap<(u8, usize), Vec<Span>>,
    names: mant_protocol::EntryStyleMap<'a>,
    lifetime: PhantomData<&'a ExplanationEvidence>,
}

fn key(root: ExplanationTextRoot<'_>) -> (u8, usize) {
    match root {
        ExplanationTextRoot::Inline(nodes) => (0, nodes.as_ptr() as usize),
        ExplanationTextRoot::Text(text) => (1, text.as_ptr() as usize),
    }
}

impl<'a> LocatedStyles<'a> {
    pub(super) fn new(evidence: &'a ExplanationEvidence) -> Self {
        Self::with_pool(evidence, &[])
    }
    pub(super) fn with_pool(
        evidence: &'a ExplanationEvidence,
        pool: &'a [mant_protocol::ExplanationSupport],
    ) -> Self {
        let mut map = Self::default();
        let mut remaining = mant_protocol::MAX_EXPLANATION_POSITIONS;
        if let Some(entry) = &evidence.entry {
            for binding in entry
                .name_bindings
                .iter()
                .take(mant_protocol::MAX_EXPLANATION_NAME_BINDINGS)
            {
                if entry.names.get(binding.name_index as usize).is_some() {
                    for occurrence in binding
                        .occurrences
                        .iter()
                        .take(mant_protocol::MAX_EXPLANATION_OCCURRENCES)
                    {
                        map.occurrence(
                            evidence,
                            pool,
                            occurrence,
                            Some(entry.kind),
                            false,
                            &mut remaining,
                        );
                    }
                }
            }
        }
        for basis in &evidence.bases {
            let occurrences: Box<dyn Iterator<Item = &ExplanationOccurrence> + '_> = match basis {
                EvidenceBasis::Name { matches } => Box::new(
                    matches
                        .iter()
                        .take(mant_protocol::MAX_EXPLANATION_MATCH_RECORDS)
                        .flat_map(|m| {
                            m.occurrences
                                .iter()
                                .take(mant_protocol::MAX_EXPLANATION_OCCURRENCES)
                        }),
                ),
                EvidenceBasis::Form { matches } => Box::new(
                    matches
                        .iter()
                        .take(mant_protocol::MAX_EXPLANATION_MATCH_RECORDS)
                        .flat_map(|m| {
                            m.occurrences
                                .iter()
                                .take(mant_protocol::MAX_EXPLANATION_OCCURRENCES)
                        }),
                ),
                _ => continue,
            };
            for occurrence in occurrences {
                map.occurrence(evidence, pool, occurrence, None, true, &mut remaining);
            }
        }
        if let Some(content) = &evidence.content {
            for preview in &evidence.previews {
                let ranges = &preview.content_ranges;
                if ranges.len() > remaining
                    || ranges.len() > mant_protocol::MAX_EXPLANATION_FRAGMENTS
                {
                    continue;
                }
                let resolved = ranges
                    .iter()
                    .map(|r| Some((content.resolve_range(pool, r)?, r.char_range())))
                    .collect::<Option<Vec<_>>>();
                if let Some(resolved) = resolved {
                    remaining -= resolved.len();
                    for (root, chars) in resolved {
                        map.add(
                            root,
                            Span {
                                chars,
                                kind: None,
                                matched: true,
                            },
                        );
                    }
                }
            }
        }
        map.normalize();
        map
    }

    pub(super) fn for_support(
        block: &'a mant_ir::Block,
        records: impl Iterator<
            Item = (
                &'a ExplanationEvidence,
                &'a [mant_protocol::ExplanationSupport],
            ),
        >,
    ) -> Self {
        let mut map = Self {
            names: mant_protocol::EntryStyleMap::for_blocks(std::slice::from_ref(block)),
            ..Self::default()
        };
        for (evidence, pool) in records {
            let other = Self::with_pool(evidence, pool);
            for (root, spans) in other.roots {
                map.roots.entry(root).or_default().extend(spans);
            }
        }
        map.normalize();
        map
    }

    fn normalize(&mut self) {
        // Normalize overlaps once. Rendering then costs O(chars log fragments),
        // not one range scan per scalar of a potentially large body.
        for spans in self.roots.values_mut() {
            let kind = spans.iter().find_map(|s| s.kind);
            let mut events = BTreeMap::<usize, (i32, i32)>::new();
            for span in spans.iter() {
                let change = (i32::from(span.kind.is_some()), i32::from(span.matched));
                let start = events.entry(span.chars.start).or_default();
                start.0 += change.0;
                start.1 += change.1;
                let end = events.entry(span.chars.end).or_default();
                end.0 -= change.0;
                end.1 -= change.1;
            }
            spans.clear();
            let (mut names, mut matches, mut previous) = (0, 0, 0);
            for (position, change) in events {
                if previous < position && (names > 0 || matches > 0) {
                    spans.push(Span {
                        chars: previous..position,
                        kind: (names > 0).then_some(kind).flatten(),
                        matched: matches > 0,
                    });
                }
                names += change.0;
                matches += change.1;
                previous = position;
            }
        }
    }
    fn occurrence(
        &mut self,
        evidence: &'a ExplanationEvidence,
        pool: &'a [mant_protocol::ExplanationSupport],
        occurrence: &ExplanationOccurrence,
        kind: Option<EntryKind>,
        matched: bool,
        remaining: &mut usize,
    ) {
        let limit = mant_protocol::MAX_EXPLANATION_FRAGMENTS;
        if let Some(entry) = &evidence.entry
            && occurrence.forms.len() <= limit
            && occurrence.forms.len() <= *remaining
        {
            let resolved = occurrence
                .forms
                .iter()
                .map(|r| {
                    Some((
                        ExplanationTextRoot::Inline(r.resolve(&entry.forms)?),
                        r.start_char as usize..r.end_char as usize,
                    ))
                })
                .collect::<Option<Vec<_>>>();
            if let Some(resolved) = resolved {
                *remaining -= resolved.len();
                for (root, chars) in resolved {
                    self.add(
                        root,
                        Span {
                            chars,
                            kind,
                            matched,
                        },
                    );
                }
            }
        }
        if let Some(content) = &evidence.content
            && occurrence.content.len() <= limit
            && occurrence.content.len() <= *remaining
        {
            let resolved = occurrence
                .content
                .iter()
                .map(|r| Some((content.resolve_range(pool, r)?, r.char_range())))
                .collect::<Option<Vec<_>>>();
            if let Some(resolved) = resolved {
                *remaining -= resolved.len();
                for (root, chars) in resolved {
                    self.add(
                        root,
                        Span {
                            chars,
                            kind,
                            matched,
                        },
                    );
                }
            }
        }
    }

    fn add(&mut self, root: ExplanationTextRoot<'a>, span: Span) {
        self.roots.entry(key(root)).or_default().push(span);
    }

    pub(crate) fn inline(
        &self,
        nodes: &[Inline],
        role: TextRole,
        decorate: &dyn Fn(TextPresentation, &str) -> String,
    ) -> String {
        let spans = self
            .roots
            .get(&key(ExplanationTextRoot::Inline(nodes)))
            .map_or(&[][..], Vec::as_slice);
        let mut cursor = 0;
        let mut output = String::new();
        visit_inline_text(nodes, self.names.ranges(nodes), |inline, _, text| {
            pieces(
                text,
                &mut cursor,
                spans,
                TextPresentation {
                    role,
                    inline,
                    matched: false,
                },
                &mut |style, value| output.push_str(&decorate(style, value)),
            );
        });
        output
    }

    pub(crate) fn text(
        &self,
        value: &str,
        decorate: &dyn Fn(TextPresentation, &str) -> String,
    ) -> String {
        let spans = self
            .roots
            .get(&key(ExplanationTextRoot::Text(value)))
            .map_or(&[][..], Vec::as_slice);
        let mut output = String::new();
        pieces(
            value,
            &mut 0,
            spans,
            TextPresentation::default(),
            &mut |style, value| output.push_str(&decorate(style, value)),
        );
        output
    }
}

fn pieces(
    value: &str,
    cursor: &mut usize,
    spans: &[Span],
    base: TextPresentation,
    emit: &mut impl FnMut(TextPresentation, &str),
) {
    if spans.is_empty() {
        *cursor += value.chars().count();
        emit(base, value);
        return;
    }
    let mut start = 0;
    let mut style = base;
    for (offset, _) in value.char_indices() {
        let mut next = base;
        let index = spans.partition_point(|span| span.chars.end <= *cursor);
        if let Some(span) = spans.get(index).filter(|span| span.chars.contains(cursor)) {
            next.matched = span.matched;
            next.inline.entry_kind = span.kind.or(next.inline.entry_kind);
        }
        if next != style {
            if offset > start {
                emit(style, &value[start..offset]);
            }
            style = next;
            start = offset;
        }
        *cursor += 1;
    }
    if start < value.len() {
        emit(style, &value[start..]);
    }
}

/// One supplied preview range, never a search over the window.
pub(super) fn preview(
    text: &str,
    start: u32,
    end: u32,
    decorate: &dyn Fn(TextPresentation, &str) -> String,
) -> String {
    let chars = start as usize..end as usize;
    let spans = if chars.start < chars.end && chars.end <= text.chars().count() {
        vec![Span {
            chars,
            kind: None,
            matched: true,
        }]
    } else {
        vec![]
    };
    let mut output = String::new();
    pieces(
        text,
        &mut 0,
        &spans,
        TextPresentation {
            role: TextRole::Body,
            inline: InlinePresentation::default(),
            matched: false,
        },
        &mut |style, value| output.push_str(&decorate(style, value)),
    );
    output
}
