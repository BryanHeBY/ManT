//! Apply already consumed native TAB advances at stable word coordinates.
//! No tab stops, formatter flags or width decisions are executed here.

use std::collections::BTreeMap;

use super::super::{
    NativeWordAnchor,
    field_buffer::{FieldBuffer, FieldCell},
};
use super::{Inline, native_passes::native_word_marker};

#[cfg(test)]
std::thread_local! {
    static NODES_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone)]
pub(in crate::mandoc::inline::flow) struct TabExpansion {
    pub(in crate::mandoc::inline::flow) owner: String,
    pub(in crate::mandoc::inline::flow) scalar: usize,
    pub(in crate::mandoc::inline::flow) spaces: usize,
}

pub(in crate::mandoc::inline::flow) fn capture(
    buffer: &FieldBuffer,
    anchors: &[NativeWordAnchor],
    printed: &[(usize, usize)],
) -> Vec<TabExpansion> {
    let mut edits = Vec::new();
    let mut owner = 0;
    let mut advance = 0;
    for (cell, value) in buffer.cells().iter().enumerate() {
        if !matches!(value, FieldCell::Tab) {
            continue;
        }
        while anchors
            .get(owner)
            .is_some_and(|anchor| anchor.content <= cell)
        {
            owner += 1;
        }
        let Some(anchor) = owner.checked_sub(1).and_then(|owner| anchors.get(owner)) else {
            continue;
        };
        while printed.get(advance).is_some_and(|(index, _)| *index < cell) {
            advance += 1;
        }
        let spaces = printed
            .get(advance)
            .filter(|(index, _)| *index == cell)
            .map_or(0, |(_, spaces)| *spaces);
        edits.push(TabExpansion {
            owner: anchor.owner.clone(),
            scalar: buffer.projection_length(anchor.content, cell),
            spaces,
        });
    }
    edits
}

pub(in crate::mandoc::inline::flow) fn apply(terms: &mut [Vec<Inline>], edits: &[TabExpansion]) {
    apply_slices(terms.iter_mut().map(Vec::as_mut_slice), edits);
}

pub(in crate::mandoc::inline::flow) fn apply_from(
    nodes: &mut [Inline],
    output_start: usize,
    edits: &[TabExpansion],
) {
    let start = output_start.min(nodes.len());
    apply_slices(std::iter::once(&mut nodes[start..]), edits);
}

fn apply_slices<'a>(terms: impl IntoIterator<Item = &'a mut [Inline]>, edits: &[TabExpansion]) {
    if edits.is_empty() {
        return;
    }
    let mut positions = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    for edit in edits {
        positions
            .entry(&edit.owner)
            .or_default()
            .push((edit.scalar, edit.spaces));
    }
    for values in positions.values_mut() {
        values.sort_unstable_by_key(|(scalar, _)| *scalar);
    }
    let mut cursor = Cursor {
        positions: &positions,
        owner: None,
        scalar: 0,
        next: 0,
        previous: BTreeMap::new(),
    };
    for term in terms {
        cursor.visit(term);
    }
}

struct Cursor<'a> {
    positions: &'a BTreeMap<&'a str, Vec<(usize, usize)>>,
    owner: Option<String>,
    scalar: usize,
    next: usize,
    previous: BTreeMap<String, (usize, usize)>,
}

impl Cursor<'_> {
    fn visit(&mut self, nodes: &mut [Inline]) {
        for node in nodes {
            #[cfg(test)]
            NODES_VISITED.with(|count| count.set(count.get() + 1));
            if let Some(marker) = native_word_marker(node) {
                if let Some(owner) = self.owner.take() {
                    self.previous.insert(owner, (self.scalar, self.next));
                }
                self.owner = self
                    .positions
                    .contains_key(marker)
                    .then(|| marker.to_owned());
                (self.scalar, self.next) = self
                    .owner
                    .as_ref()
                    .and_then(|owner| self.previous.get(owner))
                    .copied()
                    .unwrap_or_default();
                continue;
            }
            match node {
                Inline::Text { value } | Inline::Code { value } if self.owner.is_some() => {
                    self.text(value);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.visit(children),
                // Structured equations retain their shared readable value
                // and expression together; native text edits never rewrite
                // only one half of that invariant.
                _ => {}
            }
        }
    }

    fn text(&mut self, value: &mut String) {
        let positions = &self.positions[self.owner.as_deref().expect("selected word")];
        let mut changed = None;
        for (byte, character) in value.char_indices() {
            while positions
                .get(self.next)
                .is_some_and(|(scalar, _)| *scalar < self.scalar)
            {
                self.next += 1;
            }
            let expansion = positions
                .get(self.next)
                .filter(|(scalar, _)| *scalar == self.scalar);
            if character == '\t'
                && let Some((_, spaces)) = expansion
            {
                let output = changed.get_or_insert_with(|| value[..byte].to_owned());
                output.extend(std::iter::repeat_n(' ', *spaces));
                self.next += 1;
            } else if let Some(output) = &mut changed {
                output.push(character);
            }
            // Coordinates belong to the original consumed scalar stream;
            // replacing one TAB must not shift the next edit or annotation.
            self.scalar += 1;
        }
        if let Some(output) = changed {
            *value = output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_tab_projection_visits_only_the_consumed_field_suffix() {
        for historical in [64, 4096] {
            let owner = format!("{}current", super::super::INTERNAL_FIELD_WORD);
            let mut nodes = vec![
                Inline::Text {
                    value: "history".into()
                };
                historical
            ];
            nodes.push(Inline::anchor(owner.as_str()));
            nodes.push(Inline::Text {
                value: "\tX".into(),
            });
            let edits = [TabExpansion {
                owner,
                scalar: 0,
                spaces: 3,
            }];
            NODES_VISITED.with(|count| count.set(0));
            apply_from(&mut nodes, historical, &edits);
            assert_eq!(NODES_VISITED.with(std::cell::Cell::get), 2);
            assert!(matches!(&nodes[historical - 1], Inline::Text { value } if value == "history"));
            assert!(matches!(nodes.last(), Some(Inline::Text { value }) if value == "   X"));
        }
    }
}
