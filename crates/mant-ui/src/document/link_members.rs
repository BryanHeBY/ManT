//! Render-only membership for one validated legacy link occurrence.
use mant_ir::LinkTarget;
#[cfg(feature = "native-link-layout")]
use mant_ir::{Document, Inline, LinkLayoutReceipt};
use std::{collections::HashMap, ops::Range};

#[derive(Default)]
pub(super) struct LinkMembers {
    membership: HashMap<usize, Vec<Range<usize>>>,
    restrict_unidentified: bool,
    restrict_unidentified_email: bool,
}

impl LinkMembers {
    #[cfg(feature = "native-link-layout")]
    pub(super) fn from_layout(
        document: Option<&Document>,
        layout: Option<&mant_render::NativeLinkLayout>,
    ) -> Self {
        let Some(layout) = layout else {
            return Self::default();
        };
        let (mut members, _) = Self::collect(document, layout.receipts());
        members.restrict_unidentified |= layout.restrict_unidentified();
        members.restrict_unidentified_email |= layout.restrict_unidentified_email();
        for restriction in layout.restrictions() {
            let Some(target) = document
                .and_then(|doc| restriction.origin.resolve_link(doc))
                .and_then(|link| match link {
                    Inline::Link { target, .. } if target == &restriction.target => Some(target),
                    _ => None,
                })
            else {
                // SourceBoundLinkLayout cannot name another document; a
                // failed structural check cannot be interpreted as Whole.
                if matches!(restriction.target, LinkTarget::Email { .. }) {
                    members.restrict_unidentified_email = true;
                } else {
                    members.restrict_unidentified = true;
                }
                continue;
            };
            members
                .membership
                .insert(std::ptr::from_ref(target).addr(), Vec::new());
        }
        members
    }

    #[cfg(all(test, feature = "native-link-layout"))]
    pub(super) fn validate(
        document: Option<&Document>,
        receipts: &[LinkLayoutReceipt],
    ) -> Option<Self> {
        let (members, accepted) = Self::collect(document, receipts);
        accepted.then_some(members)
    }

    #[cfg(feature = "native-link-layout")]
    fn collect(document: Option<&Document>, receipts: &[LinkLayoutReceipt]) -> (Self, bool) {
        let mut ranges: HashMap<usize, Vec<Range<usize>>> = HashMap::new();
        let mut occurrences = HashMap::new();
        let mut budget = 1024 * 1024;
        let mut supported = receipts.len() <= 1000;
        for receipt in receipts.iter().take(1000) {
            let Some(Inline::Link {
                target, children, ..
            }) = document.and_then(|document| receipt.origin.resolve_link(document))
            else {
                supported = false;
                continue;
            };
            // The paired loader supplies this position. An unrecognized
            // origin/target cannot disable a different source link.
            let supported_target = match target {
                LinkTarget::External { uri } => mant_ir::is_valid_external_uri(uri),
                LinkTarget::Email { .. } => {
                    super::inline::local_link_target(target, None).is_some()
                }
                _ => false,
            };
            if !supported_target || target != &receipt.target {
                supported = false;
                continue;
            }
            let address = std::ptr::from_ref(target).addr();
            let repeated = occurrences.insert(receipt.occurrence, address);
            if let Some(earlier) = repeated
                && let Some(members) = ranges.get_mut(&earlier)
            {
                members.clear();
            }
            let mut actual = String::new();
            let mut expected = String::new();
            let representable = repeated.is_none()
                && !ranges.contains_key(&address)
                && (1..=1024).contains(&receipt.members.len())
                && receipt
                    .members
                    .windows(2)
                    .all(|pair| pair[0].end < pair[1].start)
                && checked_text(children, &mut actual, &mut budget, 64)
                && checked_text(&receipt.expected_children, &mut expected, &mut budget, 64)
                && children == &receipt.expected_children
                && whole_members(&actual, &receipt.members);
            // Do not make the document unreadable, or accidentally revive
            // foreign glyph clicks from the legacy wrapper. A matched but
            // unsupported origin keeps its text, with no clickable cells.
            let members = if representable {
                receipt.members.clone()
            } else {
                Vec::new()
            };
            ranges.insert(address, members);
            supported &= representable;
        }
        (
            Self {
                membership: ranges,
                restrict_unidentified: receipts.len() > 1000,
                restrict_unidentified_email: receipts.len() > 1000,
            },
            supported,
        )
    }

    pub(super) fn cursor(&self) -> MemberCursor<'_> {
        MemberCursor {
            membership: &self.membership,
            restrict_unidentified: self.restrict_unidentified,
            restrict_unidentified_email: self.restrict_unidentified_email,
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
    restrict_unidentified: bool,
    restrict_unidentified_email: bool,
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
            let restricted = match target {
                Some(LinkTarget::External { .. }) => self.restrict_unidentified,
                Some(LinkTarget::Email { .. }) => self.restrict_unidentified_email,
                _ => false,
            };
            emit(if restricted { None } else { target }, text);
            return;
        };
        if ranges.is_empty() {
            emit(None, text);
            return;
        }
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
