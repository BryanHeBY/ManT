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
    pub(super) contribution: Contribution,
    pub(super) syntax: BlockSyntax,
    pub(super) tail: TailSyntax,
    /// The first source root consists entirely of executed hard rows.
    pub(super) hard_rows: bool,
}

/// Last physical leaf's framing, carried through containers rather than
/// inferred from their closing delimiters. This is not formatter line state.
#[derive(Default, Clone, Copy)]
pub(super) struct TailSyntax {
    pub(super) grammar: BlockSyntax,
    pub(super) columns: usize,
    pub(super) open_row: bool,
    pub(super) hard_rows: bool,
    /// A final definition term owns its tail until the next physical boundary.
    /// Paragraph closing alone must not discard that row before a successor.
    pub(super) term_tail: bool,
}

/// Source contribution and exported grammar are independent from navigation.
#[derive(Default, Clone, Copy)]
pub(super) struct Contribution {
    /// Authored rows or structural data, never generated navigation syntax.
    pub(super) rows: bool,
    /// A resolved positive gap before the first physical row, or a gap-only root.
    pub(super) before: bool,
    /// A resolved positive gap after the last physical row.
    pub(super) after: bool,
}

impl Contribution {
    pub(super) fn spacing(self) -> bool {
        self.before || self.after
    }

    fn append(&mut self, other: Self) {
        if !self.rows {
            self.before |= self.after || other.before;
            self.after = other.after;
        } else if other.rows {
            self.after = other.after;
        } else {
            self.after |= other.spacing();
        }
        self.rows |= other.rows;
    }
}

/// First exported grammar, which may differ from the source block's variant.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockSyntax {
    #[default]
    Phrasing,
    Fence,
    List {
        needs_blank: bool,
    },
    Rule,
}

/// The first accepted block's syntax receiver, before its body scalars.
/// This is private export framing, not an IR coordinate or execution state.
#[derive(Clone, Copy)]
pub(super) struct NavigationSite {
    offset: usize,
    tail: usize,
    block: bool,
    continuation_columns: usize,
    /// Navigation inserted a syntax line before this structural receiver.
    preamble: bool,
}

impl From<String> for MappedText {
    fn from(text: String) -> Self {
        let rows = !text.is_empty();
        Self {
            text,
            owners: Vec::new(),
            navigation: None,
            contribution: Contribution {
                rows,
                ..Default::default()
            },
            syntax: BlockSyntax::Phrasing,
            tail: TailSyntax::default(),
            hard_rows: false,
        }
    }
}

impl MappedText {
    pub(super) fn nonempty(self) -> Option<Self> {
        (!self.text.is_empty() || self.contribution.spacing()).then_some(self)
    }

    pub(super) fn append(&mut self, mut other: Self) {
        let offset = self.text.len();
        if other.contribution.rows {
            self.tail = other.tail;
        }
        self.contribution.append(other.contribution);
        if offset == 0 {
            self.syntax = other.syntax;
            self.hard_rows = other.hard_rows;
        }
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

    pub(super) fn syntax_site(mut self, syntax: BlockSyntax) -> Self {
        self.syntax = syntax;
        self.navigation = Some(NavigationSite {
            offset: 0,
            tail: 0,
            block: syntax != BlockSyntax::Phrasing,
            continuation_columns: 0,
            preamble: false,
        });
        self
    }

    pub(super) fn tail_grammar(mut self, grammar: BlockSyntax, open_row: bool) -> Self {
        self.tail.grammar = grammar;
        self.tail.open_row = open_row;
        self
    }

    pub(super) fn hard_rows(mut self, only_breaks: bool) -> Self {
        self.hard_rows = only_breaks;
        self.tail.hard_rows = only_breaks;
        self
    }

    /// Block navigation has its own syntax line before the actual receiver.
    /// It may join preceding phrasing without a soft-break separator cell.
    pub(super) fn has_navigation_preamble(&self) -> bool {
        self.navigation
            .is_some_and(|site| site.preamble && site.offset == 0)
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
            preamble: site.preamble || site.block,
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
            contribution: self.contribution,
            syntax: BlockSyntax::List {
                needs_blank: marker.ends_with(". ") && marker != "1. ",
            },
            tail: TailSyntax {
                columns: self.tail.columns.saturating_add(marker.chars().count()),
                ..self.tail
            },
            hard_rows: false,
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
            .syntax_site(BlockSyntax::Fence)
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
        let mut rendered = MappedText::from("α".to_owned()).syntax_site(BlockSyntax::Phrasing);
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
            contribution: Contribution::default(),
            ..Default::default()
        };
        let mut rendered = MappedText::from("BODY".to_owned()).syntax_site(BlockSyntax::Phrasing);
        rendered.attach_navigation_text(navigation, false);
        assert_eq!(rendered.text, "[](uri)BODY");
        assert_eq!(&rendered.text[rendered.owners[0].1.clone()], "[](uri)");
    }
}
