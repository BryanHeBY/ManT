//! Compose byte ownership with rendered text instead of recovering it from HTML.
use mant_ir::{ContentRootKey, EntryFacts, EntryOwner};
use std::{collections::HashMap, ops::Range};

use super::source_map::RenderedRootRange;

/// Keys identify borrowed facts only during one immutable render operation.
/// They are never dereferenced, serialized, or cached beyond that operation.
pub(super) type OwnerKey = *const EntryFacts;

#[derive(Default)]
pub(super) struct MappedText {
    pub(super) text: String,
    pub(super) owners: Vec<(OwnerKey, Range<usize>)>,
    pub(super) roots: Vec<RenderedRootRange>,
}

impl From<String> for MappedText {
    fn from(text: String) -> Self {
        Self {
            text,
            owners: Vec::new(),
            roots: Vec::new(),
        }
    }
}

impl MappedText {
    /// Apply the same Unicode whitespace trim as the flattened table renderer,
    /// keeping only byte ownership that remains visible after trimming.
    pub(super) fn trim(mut self) -> Self {
        let start = self.text.len() - self.text.trim_start().len();
        let end = self.text.trim_end().len();
        if start >= end {
            return Self::default();
        }
        self.text = self.text[start..end].to_owned();
        self.roots = self
            .roots
            .into_iter()
            .filter_map(|mut root| {
                root.markdown.start = root.markdown.start.max(start);
                root.markdown.end = root.markdown.end.min(end);
                if root.markdown.start >= root.markdown.end {
                    return None;
                }
                root.markdown.start -= start;
                root.markdown.end -= start;
                Some(root)
            })
            .collect();
        self
    }

    pub(super) fn nonempty(self) -> Option<Self> {
        (!self.text.is_empty()).then_some(self)
    }

    pub(super) fn append(&mut self, mut other: Self) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        for (_, range) in &mut other.owners {
            range.start += offset;
            range.end += offset;
        }
        self.owners.extend(other.owners);
        for root in &mut other.roots {
            root.markdown.start += offset;
            root.markdown.end += offset;
        }
        self.roots.extend(other.roots);
    }

    pub(super) fn join(values: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        let mut values = values.into_iter();
        let Some(mut result) = values.next() else {
            return Self::default();
        };
        for value in values {
            result.text.push_str(separator);
            result.append(value);
        }
        result
    }

    pub(super) fn insert(&mut self, offset: usize, value: &str) {
        self.text.insert_str(offset, value);
        for (_, range) in &mut self.owners {
            if range.start >= offset {
                range.start += value.len();
            }
            if range.end > offset {
                range.end += value.len();
            }
        }
        for root in &mut self.roots {
            if root.markdown.start >= offset {
                root.markdown.start += value.len();
            }
            if root.markdown.end > offset {
                root.markdown.end += value.len();
            }
        }
    }

    pub(super) fn with_owner(mut self, owner: EntryOwner<'_>, enabled: bool) -> Self {
        if enabled
            && !self.text.is_empty()
            && let Some(facts) = owner.facts()
        {
            self.owners
                .push((std::ptr::from_ref(facts), 0..self.text.len()));
        }
        self
    }

    pub(super) fn with_root(mut self, root: Option<ContentRootKey>) -> Self {
        if !self.text.is_empty()
            && let Some(root) = root
        {
            self.roots.push(RenderedRootRange {
                root,
                markdown: 0..self.text.len(),
            });
        }
        self
    }

    pub(super) fn owner_ranges(&self) -> HashMap<OwnerKey, Range<usize>> {
        self.owners.iter().cloned().collect()
    }

    /// Apply the renderer's existing list prefixes while moving every boundary
    /// through the same line transform, including CRLF and empty continuation lines.
    pub(super) fn prefix(self, marker: &str) -> Option<Self> {
        if self.text.trim().is_empty() {
            return None;
        }
        let continuation = " ".repeat(marker.chars().count());
        let mut output = String::new();
        let mut segments = Vec::new();
        let mut offset = 0;
        for (index, line) in self.text.lines().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            if index == 0 {
                output.push_str(marker);
            } else if !line.is_empty() {
                output.push_str(&continuation);
            }
            if !self.owners.is_empty() || !self.roots.is_empty() {
                segments.push((offset, line.len(), output.len()));
            }
            output.push_str(line);
            offset += line.len();
            if self.text.as_bytes().get(offset) == Some(&b'\r') {
                offset += 1;
            }
            if self.text.as_bytes().get(offset) == Some(&b'\n') {
                offset += 1;
            }
        }
        let boundary = |position: usize| {
            let index = segments
                .partition_point(|(start, _, _)| *start <= position)
                .saturating_sub(1);
            let (start, length, mapped) = segments[index];
            mapped + position.saturating_sub(start).min(length)
        };
        let owners = self
            .owners
            .into_iter()
            .filter_map(|(owner, range)| {
                let range = boundary(range.start)..boundary(range.end);
                (range.start < range.end).then_some((owner, range))
            })
            .collect();
        let roots = self
            .roots
            .into_iter()
            .filter_map(|mut root| {
                root.markdown = boundary(root.markdown.start)..boundary(root.markdown.end);
                (root.markdown.start < root.markdown.end).then_some(root)
            })
            .collect();
        Some(Self {
            text: output,
            owners,
            roots,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_preserve_nested_unicode_and_crlf_owner_boundaries() {
        // Opaque equal keys are sufficient here: no facts are dereferenced.
        let key = std::ptr::null();
        let rendered = MappedText {
            text: "α\r\n\r\n日本\r\nlast".into(),
            owners: vec![(key, 0..18), (key, 6..12)],
            roots: Vec::new(),
        }
        .prefix("12. ")
        .unwrap();
        assert_eq!(rendered.text, "12. α\n\n    日本\n    last");
        assert_eq!(
            &rendered.text[rendered.owners[0].1.clone()],
            "α\n\n    日本\n    last"
        );
        assert_eq!(&rendered.text[rendered.owners[1].1.clone()], "日本");
    }

    #[test]
    fn joining_and_inserting_do_not_claim_adjacent_unowned_text() {
        let key = std::ptr::null();
        let mut rendered = MappedText::join(
            [
                "before".to_owned().into(),
                MappedText {
                    text: "owned".into(),
                    owners: vec![(key, 0..5)],
                    roots: Vec::new(),
                },
                "after".to_owned().into(),
            ],
            " | ",
        );
        rendered.insert(0, "prefix ");
        rendered.insert(rendered.text.len(), " suffix");
        assert_eq!(&rendered.text[rendered.owners[0].1.clone()], "owned");
    }
}
