//! Project accepted native intervals through the original inline wrappers.

use std::collections::BTreeMap;

use super::super::{INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD, native_passes::OwnerAcceptance};
use super::Inline;
use crate::mandoc::inline::ownership::markers::{COVERED_ANNOTATION, is_target};

/// A cached glyph can resume an earlier owner after a later owner's marker.
/// A rejected label can retain a typed destination. Accepted glyphs covered
/// by later device writes carry a separate receipt for link publication.
pub(in crate::mandoc::inline::flow) fn retain_native_field_owners(
    nodes: &mut Vec<Inline>,
    limits: &BTreeMap<String, OwnerAcceptance>,
) -> bool {
    let mut owners = AcceptedOwners {
        limits,
        owner: None,
        prefix_printed: None,
        positions: BTreeMap::new(),
        covered: 0,
    };
    owners.retain(nodes);
    owners.owner.is_some()
}

struct AcceptedOwners<'a> {
    limits: &'a BTreeMap<String, OwnerAcceptance>,
    owner: Option<&'a str>,
    prefix_printed: Option<bool>,
    positions: BTreeMap<&'a str, (usize, usize)>,
    covered: usize,
}

impl AcceptedOwners<'_> {
    fn retain(&mut self, nodes: &mut Vec<Inline>) {
        nodes.retain_mut(|node| self.project(node));
    }

    fn project(&mut self, node: &mut Inline) -> bool {
        match node {
            Inline::Anchor { id, .. } => {
                self.enter_owner(id.as_str());
                true
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                let before = self.covered;
                self.retain(children);
                if before < self.covered
                    && matches!(target, mant_ir::LinkTarget::External { uri } if is_target(uri))
                {
                    *title = Some(COVERED_ANNOTATION.to_owned());
                }
                true
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                self.retain(children);
                !children.is_empty()
            }
            _ if self.prefix_printed.is_some() => self.prefix_printed == Some(true),
            _ if self.owner.is_none() => true,
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                self.retain_text(value)
            }
            Inline::LineBreak { .. } => self.has_native_cells(),
        }
    }

    fn enter_owner(&mut self, id: &str) {
        if let Some(serial) = id.strip_prefix(INTERNAL_FIELD_PREFIX) {
            // Generated minbl precedes the word's scalar interval.
            // term_field() prints it only with that word's first graph.
            let word = format!("{INTERNAL_FIELD_WORD}{serial}");
            self.prefix_printed = Some(
                self.limits
                    .get(&word)
                    .is_some_and(|range| range.prefix_printed),
            );
        } else if let Some((owner, _)) = self.limits.get_key_value(id) {
            self.owner = Some(owner);
            self.prefix_printed = None;
        }
    }

    fn has_native_cells(&self) -> bool {
        self.owner
            .and_then(|owner| self.limits.get(owner))
            .is_some_and(|limit| limit.native_cells)
    }

    fn retain_text(&mut self, value: &mut String) -> bool {
        if value.is_empty() {
            // A native invisible graph can own an empty physical row
            // (term.c:340-349,475-481).
            return self.has_native_cells();
        }
        let owner = self.owner.expect("native owner");
        let (position, next_omission) = self.positions.entry(owner).or_default();
        let accepted = &self.limits[owner];
        let omissions = &accepted.omitted_scalars;
        value.retain(|_| {
            let scalar = *position;
            *position = position.saturating_add(1);
            while omissions
                .get(*next_omission)
                .is_some_and(|omit| omit.range.end <= scalar)
            {
                *next_omission += 1;
            }
            if scalar >= accepted.scalars {
                return false;
            }
            if let Some(omit) = omissions
                .get(*next_omission)
                .filter(|omit| omit.range.contains(&scalar))
            {
                self.covered += usize::from(omit.covered);
                false
            } else {
                true
            }
        });
        !value.is_empty()
    }
}
