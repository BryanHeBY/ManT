//! Render-only membership for one validated legacy link occurrence.
use mant_ir::LinkTarget;
#[cfg(feature = "native-link-layout")]
use mant_ir::{Document, Inline, LinkLayout};
use std::{collections::HashMap, ops::Range};

#[derive(Default)]
pub(super) struct LinkMembers(HashMap<usize, Vec<Range<usize>>>);

impl LinkMembers {
    #[cfg(feature = "native-link-layout")]
    pub(super) fn validate(document: Option<&Document>, layout: &LinkLayout) -> Option<Self> {
        let mut ranges = HashMap::new();
        let mut occurrences = std::collections::HashSet::new();
        let mut budget = 1024 * 1024;
        if layout.receipts.len() > 1000 {
            return None;
        }
        for receipt in &layout.receipts {
            let Inline::Link {
                target, children, ..
            } = receipt.origin.resolve_link(document?)?
            else {
                return None;
            };
            // This first route only supports described native external links.
            if !matches!(target, LinkTarget::External { uri } if mant_ir::is_valid_external_uri(uri))
                || target != &receipt.target
                || !occurrences.insert(receipt.occurrence)
                || !(2..=1024).contains(&receipt.members.len())
                || receipt.members.windows(2).any(|w| w[0].end >= w[1].start)
            {
                return None;
            }
            let mut actual = String::new();
            let mut expected = String::new();
            if !checked_text(children, &mut actual, &mut budget, 64)
                || !checked_text(&receipt.expected_children, &mut expected, &mut budget, 64)
                || children != &receipt.expected_children
                || !whole_members(&actual, &receipt.members)
                || ranges
                    .insert(std::ptr::from_ref(target).addr(), receipt.members.clone())
                    .is_some()
            {
                return None;
            }
        }
        Some(Self(ranges))
    }

    pub(super) fn cursor(&self) -> MemberCursor<'_> {
        MemberCursor {
            membership: &self.0,
            positions: HashMap::new(),
        }
    }
}

#[cfg(feature = "native-link-layout")]
fn checked_text(nodes: &[Inline], text: &mut String, budget: &mut usize, depth: usize) -> bool {
    if depth == 0 || nodes.len() > *budget {
        return false;
    }
    *budget -= nodes.len();
    for node in nodes {
        let value = match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                value.as_str()
            }
            Inline::LineBreak { .. } => "\n",
            Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } => {
                let cost = fragment_aliases
                    .iter()
                    .fold(id.as_str().len(), |cost, alias| {
                        cost.saturating_add(alias.to_string().len())
                            .saturating_add(1)
                    });
                if cost > *budget {
                    return false;
                }
                *budget -= cost;
                continue;
            }
            // Nested competing targets are not part of this first contract.
            Inline::Link { .. } => return false,
            Inline::Strong { children } | Inline::Emphasis { children } => {
                if !checked_text(children, text, budget, depth - 1) {
                    return false;
                }
                continue;
            }
        };
        if value.len() > *budget {
            return false;
        }
        *budget -= value.len();
        text.push_str(value);
    }
    true
}

#[cfg(feature = "native-link-layout")]
fn whole_members(text: &str, members: &[Range<usize>]) -> bool {
    if members.iter().any(|r| r.start >= r.end) {
        return false;
    }
    let mut offsets = members.iter().flat_map(|r| [r.start, r.end]).peekable();
    let mut scalar = 0;
    for glyph in mant_render::cells::graphemes(text) {
        if offsets.peek() == Some(&scalar) {
            offsets.next();
        }
        scalar += glyph.text().chars().count();
        if offsets.peek().is_some_and(|offset| *offset < scalar) {
            return false;
        }
    }
    if offsets.peek() == Some(&scalar) {
        offsets.next();
    }
    offsets.next().is_none()
}

pub(super) struct MemberCursor<'a> {
    membership: &'a HashMap<usize, Vec<Range<usize>>>,
    positions: HashMap<usize, (usize, usize)>,
}

impl MemberCursor<'_> {
    pub(super) fn emit<'text>(
        &mut self,
        target: Option<&'text LinkTarget>,
        text: &'text str,
        mut emit: impl FnMut(Option<&'text LinkTarget>, &'text str),
    ) {
        let Some((address, target, ranges)) = target.and_then(|target| {
            let address = std::ptr::from_ref(target).addr();
            Some((address, target, self.membership.get(&address)?))
        }) else {
            emit(target, text);
            return;
        };
        let (scalar, index) = self.positions.entry(address).or_default();
        let mut start = 0;
        let mut admitted = ranges
            .get(*index)
            .is_some_and(|range| range.contains(scalar));
        for (offset, _) in text.char_indices() {
            while ranges.get(*index).is_some_and(|range| *scalar >= range.end) {
                *index += 1;
            }
            let owned = ranges
                .get(*index)
                .is_some_and(|range| range.contains(scalar));
            if owned != admitted {
                if start < offset {
                    emit(admitted.then_some(target), &text[start..offset]);
                }
                start = offset;
                admitted = owned;
            }
            *scalar += 1;
        }
        if start < text.len() {
            emit(admitted.then_some(target), &text[start..]);
        }
    }
}
