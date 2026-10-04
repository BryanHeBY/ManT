//! Compose byte ownership with rendered text instead of recovering it from HTML.
use mant_ir::{EntryFacts, EntryOwner};
use std::{collections::HashMap, ops::Range};

/// Keys identify borrowed facts only during one immutable render operation.
/// They are never dereferenced, serialized, or cached beyond that operation.
pub(super) type OwnerKey = *const EntryFacts;

#[derive(Default)]
pub(super) struct MappedText {
    pub(super) text: String,
    pub(super) owners: Vec<(OwnerKey, Range<usize>)>,
    pub(super) navigation: Option<NavigationSite>,
    pub(super) navigation_only: bool,
}

/// The first accepted block's syntax receiver, before its body scalars.
/// This is private export framing, not an IR coordinate or execution state.
#[derive(Clone, Copy)]
pub(super) struct NavigationSite {
    offset: usize,
    tail: usize,
    block: bool,
    continuation_columns: usize,
}

impl From<String> for MappedText {
    fn from(text: String) -> Self {
        Self {
            text,
            owners: Vec::new(),
            navigation: None,
            navigation_only: false,
        }
    }
}

impl MappedText {
    pub(super) fn nonempty(self) -> Option<Self> {
        (!self.text.is_empty()).then_some(self)
    }

    pub(super) fn append(&mut self, mut other: Self) {
        let offset = self.text.len();
        self.navigation_only = if offset == 0 {
            other.navigation_only
        } else {
            self.navigation_only && other.navigation_only
        };
        if self.navigation.is_none() {
            self.navigation = other.navigation.map(|mut site| {
                site.offset += offset;
                site.tail += offset;
                site
            });
        }
        self.text.push_str(&other.text);
        for (_, range) in &mut other.owners {
            range.start += offset;
            range.end += offset;
        }
        self.owners.extend(other.owners);
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
        if let Some(site) = &mut self.navigation {
            if site.offset >= offset {
                site.offset += value.len();
            }
            if site.tail >= offset {
                site.tail += value.len();
            }
        }
        for (_, range) in &mut self.owners {
            if range.start >= offset {
                range.start += value.len();
            }
            if range.end > offset {
                range.end += value.len();
            }
        }
    }

    pub(super) fn navigation_site(mut self, block: bool) -> Self {
        self.navigation = Some(NavigationSite {
            offset: 0,
            tail: 0,
            block,
            continuation_columns: 0,
        });
        self
    }

    pub(super) fn attach_navigation(&mut self, navigation: &str) {
        self.navigation_at(navigation, false);
    }

    pub(super) fn append_navigation(&mut self, navigation: &str) {
        self.navigation_at(navigation, true);
    }

    pub(super) fn attach_navigation_text(&mut self, navigation: Self, append: bool) {
        if navigation.text.is_empty() {
            return;
        }
        let offset = self.navigation_at(&navigation.text, append);
        self.owners.extend(
            navigation
                .owners
                .into_iter()
                .map(|(owner, range)| (owner, range.start + offset..range.end + offset)),
        );
    }

    fn navigation_at(&mut self, navigation: &str, append: bool) -> usize {
        if navigation.is_empty() {
            return 0;
        }
        let site = self.navigation.expect("a rendered block's syntax receiver");
        let offset = if append { site.tail } else { site.offset };
        let mut syntax = navigation.to_owned();
        if site.block {
            syntax.push('\n');
            syntax.push_str(&" ".repeat(site.continuation_columns));
        }
        self.insert(offset, &syntax);
        // The receiver precedes the first accepted child content. Keep a
        // parent's navigation outside nested entry ranges, including their
        // generated list prefixes; their own names and payload remain inside.
        for (_, range) in &mut self.owners {
            if range.start < offset && range.end > offset {
                range.start = offset + syntax.len();
            }
        }
        // Later annotations join this same zero-width phrasing, without
        // creating another syntax line or another paragraph boundary.
        self.navigation = Some(NavigationSite {
            tail: site.tail + navigation.len(),
            block: false,
            ..site
        });
        offset
    }

    pub(super) fn with_owner(mut self, owner: EntryOwner<'_>, enabled: bool) -> Self {
        // An ancestor's annotations precede this owner's own destinations.
        // The child's existing zero-width prefix stays inside its range.
        if let Some(site) = &mut self.navigation {
            site.tail = site.offset;
        }
        if enabled
            && !self.text.is_empty()
            && let Some(facts) = owner.facts()
        {
            self.owners
                .push((std::ptr::from_ref(facts), 0..self.text.len()));
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
        let mut navigation = self.navigation.map(|mut site| {
            site.continuation_columns += marker.chars().count();
            site
        });
        for (index, line) in self.text.lines().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            if index == 0 {
                output.push_str(marker);
            } else if !line.is_empty() {
                output.push_str(&continuation);
            }
            if !self.owners.is_empty() {
                segments.push((offset, line.len(), output.len()));
            }
            if let (Some(site), Some(mapped)) = (self.navigation, &mut navigation) {
                for (position, destination) in [
                    (site.offset, &mut mapped.offset),
                    (site.tail, &mut mapped.tail),
                ] {
                    if position >= offset && position <= offset + line.len() {
                        *destination = output.len() + position - offset;
                    }
                }
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
        Some(Self {
            text: output,
            owners,
            navigation,
            navigation_only: self.navigation_only,
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
            ..Default::default()
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
                    ..Default::default()
                },
                "after".to_owned().into(),
            ],
            " | ",
        );
        rendered.insert(0, "prefix ");
        rendered.insert(rendered.text.len(), " suffix");
        assert_eq!(&rendered.text[rendered.owners[0].1.clone()], "owned");
    }

    #[test]
    fn navigation_receivers_follow_unicode_prefixes_without_new_paragraphs() {
        let mut rendered = MappedText::from("```txt\nα\n```".to_owned())
            .navigation_site(true)
            .prefix("12. ")
            .unwrap();
        rendered.attach_navigation("[](first)");
        rendered.append_navigation("[](second)");
        assert_eq!(
            rendered.text,
            "12. [](first)[](second)\n    ```txt\n    α\n    ```"
        );
        assert_eq!(rendered.text.matches("\n\n").count(), 0);
    }

    #[test]
    fn parent_navigation_preserves_a_childs_own_anchor_and_unicode_range() {
        let item: mant_ir::ListItem = serde_json::from_value(serde_json::json!({
            "blocks": [],
            "entry": { "id": "own", "kind": {"kind": "term"}, "case": "sensitive", "names": ["α"] }
        }))
        .unwrap();
        let mut rendered = MappedText::from("α".to_owned()).navigation_site(false);
        rendered.attach_navigation("<a id=\"own\"></a>");
        let mut rendered = rendered
            .with_owner(EntryOwner::List(&item), true)
            .prefix("- ")
            .unwrap();
        rendered.append_navigation("[](parent)");
        assert_eq!(rendered.text, "- [](parent)<a id=\"own\"></a>α");
        assert_eq!(
            &rendered.text[rendered.owners[0].1.clone()],
            "<a id=\"own\"></a>α"
        );
    }

    #[test]
    fn navigation_only_owners_transfer_without_claiming_the_receiver() {
        let key = std::ptr::null();
        let navigation = MappedText {
            text: "[](uri)".into(),
            owners: vec![(key, 0..7)],
            navigation_only: true,
            ..Default::default()
        };
        let mut rendered = MappedText::from("BODY".to_owned()).navigation_site(false);
        rendered.attach_navigation_text(navigation, false);
        assert_eq!(rendered.text, "[](uri)BODY");
        assert_eq!(&rendered.text[rendered.owners[0].1.clone()], "[](uri)");
    }
}
