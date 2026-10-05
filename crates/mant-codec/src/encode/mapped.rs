//! Compose byte ownership with rendered text instead of recovering it from HTML.
mod navigation;
mod prefix;

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
}

#[cfg(test)]
mod tests;
