//! Allocate unique native inline anchors within established section IDs.
use std::collections::{HashMap, HashSet};

use mant_ir::{
    Block, Inline, Section,
    visit::{self, VisitMut},
};

/// Normalize and uniquely allocate formatter-generated native anchors.
///
/// Explicit `.Tg` identities remain source-authored destinations. Other man
/// and mdoc tags are formatter conveniences, so expose them through the same
/// document-local slug contract as semantic entries and disambiguate repeated
/// tags before IR validation observes them.
pub(in crate::mandoc) fn normalize_generated_anchors(
    blocks: &mut [Block],
    sections: &mut [Section],
    explicit_targets: &HashSet<String>,
) {
    struct Normalizer<'targets> {
        explicit_targets: &'targets HashSet<String>,
        used: HashSet<String>,
        conflicting_canonicals: HashSet<String>,
        next_suffix: HashMap<String, usize>,
    }

    impl Normalizer<'_> {
        fn allocate(&mut self, base: &str, original: &str) -> String {
            let candidate = base.to_owned();
            if (base == original || !self.explicit_targets.contains(base))
                && self.used.insert(candidate.clone())
            {
                return candidate;
            }
            let suffix = self.next_suffix.entry(candidate).or_insert(2);
            loop {
                let candidate = format!("{base}-{suffix}");
                *suffix += 1;
                // Occupied IDs and authored reservations only accumulate.
                // An exact reserved suffix is checked as its own slug above.
                if !self.explicit_targets.contains(&candidate)
                    && self.used.insert(candidate.clone())
                {
                    return candidate;
                }
            }
        }
    }

    impl VisitMut for Normalizer<'_> {
        fn visit_inline_mut(&mut self, inline: &mut Inline) {
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
            {
                if crate::definitions::is_internal_definition_owner_marker(id.as_str()) {
                    return;
                }
                let original = id.to_string();
                let base = crate::definitions::document_id_slug(&original);
                let candidate = self.allocate(&base, &original);
                if candidate != original && self.explicit_targets.contains(&original) {
                    if base == original {
                        self.conflicting_canonicals.insert(original.clone());
                    }
                    fragment_aliases.push(original.into());
                }
                *id = candidate.into();
                return;
            }
            visit::walk_inline_mut(self, inline);
        }
    }

    struct ReassignAmbiguousCanonical<'a, 'targets>(&'a mut Normalizer<'targets>);

    impl VisitMut for ReassignAmbiguousCanonical<'_, '_> {
        fn visit_inline_mut(&mut self, inline: &mut Inline) {
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
                && self.0.conflicting_canonicals.remove(id.as_str())
            {
                let original = id.as_str();
                let canonical = self.0.allocate(original, original);
                fragment_aliases.push(original.into());
                *id = canonical.into();
                return;
            }
            visit::walk_inline_mut(self, inline);
        }
    }

    let mut used = HashSet::new();
    reserve_section_targets(sections, &mut used);
    let mut normalizer = Normalizer {
        explicit_targets,
        used,
        conflicting_canonicals: HashSet::new(),
        next_suffix: HashMap::new(),
    };
    for block in &mut *blocks {
        normalizer.visit_block_mut(block);
    }
    for section in &mut *sections {
        normalizer.visit_section_mut(section);
    }
    // The shared author spelling must be ambiguous, while each canonical
    // stays addressable. Move only the shadowed canonical after allocating
    // the other IDs, so links to existing suffixed IDs keep their owners.
    if normalizer.conflicting_canonicals.is_empty() {
        return;
    }
    let mut aliases = ReassignAmbiguousCanonical(&mut normalizer);
    for block in blocks {
        aliases.visit_block_mut(block);
    }
    for section in sections {
        aliases.visit_section_mut(section);
    }
}

fn reserve_section_targets(sections: &[Section], used: &mut HashSet<String>) {
    for section in sections {
        used.insert(section.id.to_string());
        used.extend(section.fragment_aliases.iter().map(ToString::to_string));
        reserve_section_targets(&section.children, used);
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_generated_anchors;
    use mant_ir::{Block, Inline, InlineLayout, LayoutHint};
    use std::collections::HashSet;

    #[test]
    fn anchor_suffix_cursor_keeps_source_order_and_authored_reservations() {
        // The uppercase and dotted spellings use the same slug namespace.
        // The reserved base can still be used later by its exact owner.
        let mut blocks = [Block::Paragraph {
            children: ["TAG", "tag", "tag-2", "tag", "TAG", "Tag.2", "tag"]
                .into_iter()
                .map(Inline::anchor)
                .collect(),
            layout: LayoutHint::default(),
            inline_layout: InlineLayout::default(),
            source: None,
        }];
        let reserved = HashSet::from(["tag".to_owned(), "tag-2".to_owned()]);
        normalize_generated_anchors(&mut blocks, &mut [], &reserved);
        let [Block::Paragraph { children, .. }] = &blocks else {
            panic!("one anchor carrier");
        };
        let projected = children
            .iter()
            .map(|child| {
                let Inline::Anchor {
                    id,
                    fragment_aliases,
                    ..
                } = child
                else {
                    panic!("expected one anchor: {child:?}");
                };
                (
                    id.as_str(),
                    fragment_aliases.first().map(mant_ir::FragmentAlias::as_str),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            [
                ("tag-3", None),
                ("tag-7", Some("tag")),
                ("tag-2", None),
                ("tag-4", Some("tag")),
                ("tag-5", None),
                ("tag-2-2", None),
                ("tag-6", Some("tag")),
            ]
        );
    }
}
