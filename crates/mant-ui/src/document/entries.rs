//! Source-bound signature accents: never search names in unrelated body text.

use std::collections::{HashMap, HashSet};

use mant_ir::{
    Document, EntryForm, EntryKind, EntryOwner, Inline,
    visit::{self, Visit},
};

use crate::{
    code::source::Source,
    entry::{self, Accent, EntryLabels, LabelStyles},
};

#[derive(Default)]
pub(super) struct EntryStyles<'a> {
    names: mant_render::EntryStyleMap<'a>,
    roots: HashMap<usize, Vec<Accent>>,
    pub(super) labels: EntryLabels,
    label_ids: HashSet<String>,
}

struct PreparedForm {
    text: String,
    scalars: usize,
    accents: Vec<Accent>,
    projected: Vec<(usize, Accent)>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct InlineStyles<'a> {
    pub(super) names: &'a [mant_render::InlineNameRange],
    pub(super) accents: &'a [Accent],
}

impl<'a> From<&'a [mant_render::InlineNameRange]> for InlineStyles<'a> {
    fn from(names: &'a [mant_render::InlineNameRange]) -> Self {
        Self {
            names,
            accents: &[],
        }
    }
}

impl<'a> EntryStyles<'a> {
    pub(super) fn for_document(document: &'a Document) -> Self {
        let mut styles = Self {
            names: mant_render::EntryStyleMap::for_document(document),
            ..Self::default()
        };
        styles.visit_document(document);
        for accents in styles.roots.values_mut() {
            accents.sort_by_key(|accent| accent.range.start);
            accents.dedup();
        }
        styles
    }

    pub(super) fn ranges(&self, nodes: &[Inline]) -> InlineStyles<'_> {
        InlineStyles {
            names: self.names.ranges(nodes),
            accents: self
                .roots
                .get(&(nodes.as_ptr() as usize))
                .map_or(&[], Vec::as_slice),
        }
    }

    fn owner(&mut self, owner: EntryOwner<'a>) {
        let Some(facts) = owner.facts() else { return };
        if !matches!(
            facts.kind,
            EntryKind::Parameter { .. } | EntryKind::Command | EntryKind::ConfigurationKey
        ) {
            return;
        }
        if owner.validated_form_count().is_none() {
            return;
        }
        let mut full = String::new();
        let mut full_scalars = 0;
        let mut full_accents = Vec::new();
        let mut pending = Vec::new();
        for form in &facts.forms {
            let Some(prepared) = self.form(owner, form) else {
                return;
            };
            let PreparedForm {
                text,
                scalars,
                accents,
                projected,
            } = prepared;
            pending.extend(projected);
            if text.trim().is_empty() {
                continue;
            }
            let separator = usize::from(!full.is_empty()) * 3;
            if full.len() + text.len() + separator > entry::MAX_FORM_BYTES {
                return;
            }
            if separator > 0 {
                let position = full_scalars + 1;
                full.push_str(" | ");
                full_scalars += 3;
                full_accents.push(Accent {
                    range: position..position + 1,
                    role: crate::theme::StyleRole::EntrySeparator,
                });
            }
            let start = full_scalars;
            full_accents.extend(accents.into_iter().map(|accent| Accent {
                range: start + accent.range.start..start + accent.range.end,
                role: accent.role,
            }));
            full.push_str(&text);
            full_scalars += scalars;
        }
        for (root, accent) in pending {
            self.roots.entry(root).or_default().push(accent);
        }
        // Duplicate identities cannot safely bind a cached outline label.
        let id = facts.id.to_string();
        if !self.label_ids.insert(id.clone()) {
            self.labels.remove(&id);
        } else if !full_accents.is_empty() {
            self.labels
                .insert(id, LabelStyles::new(&full, full_accents));
        }
    }

    fn form(&self, owner: EntryOwner<'a>, form: &EntryForm) -> Option<PreparedForm> {
        let mut source = Source::default();
        let mut names = Vec::new();
        let mut parts = Vec::new();
        let mut scalar = 0;
        for group in form.parts.chunk_by(|left, right| left.root == right.root) {
            let nodes = owner.inline_root(&group[0].root)?;
            let root = nodes.as_ptr() as usize;
            let mut names_in_root = self.names.ranges(nodes);
            for range in source.append_slices(nodes, group, entry::MAX_FORM_BYTES)? {
                let first = names_in_root.partition_point(|name| name.chars.end <= range.start);
                names_in_root = &names_in_root[first..];
                for name in names_in_root
                    .iter()
                    .take_while(|name| name.chars.start < range.end)
                {
                    let start = name.chars.start.max(range.start);
                    let end = name.chars.end.min(range.end);
                    if start < end {
                        names.push(scalar + start - range.start..scalar + end - range.start);
                    }
                }
                let count = range.len();
                parts.push((root, range, scalar));
                scalar += count;
            }
        }
        let accents =
            entry::form_accents(&source.text, &names, &source.emphasis, &source.protected);
        let mut pending = Vec::new();
        let mut part_index = 0;
        for accent in &accents {
            while parts
                .get(part_index)
                .is_some_and(|(_, range, start)| *start + range.len() <= accent.range.start)
            {
                part_index += 1;
            }
            for (root, range, start) in parts[part_index..]
                .iter()
                .take_while(|(_, _, start)| *start < accent.range.end)
            {
                let overlap =
                    accent.range.start.max(*start)..accent.range.end.min(*start + range.len());
                if overlap.is_empty() {
                    continue;
                }
                pending.push((
                    *root,
                    Accent {
                        range: range.start + overlap.start - start
                            ..range.start + overlap.end - start,
                        role: accent.role,
                    },
                ));
            }
        }
        Some(PreparedForm {
            text: source.text,
            scalars: scalar,
            accents,
            projected: pending,
        })
    }
}

impl<'a> Visit<'a> for EntryStyles<'a> {
    fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
        self.owner(EntryOwner::Definition(item));
        visit::walk_definition_item(self, item);
    }
    fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
        self.owner(EntryOwner::List(item));
        visit::walk_list_item(self, item);
    }
}
