//! Shared exact path, ID, alias and shorthand resolution.
use super::{LocatedNode, ProjectionError, ambiguous_selector, semantic_name_shorthand};
use mant_ir::{DefinitionCase, OutlinePath};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy)]
pub(super) enum AliasMatchKind {
    Exact,
    Shorthand,
}

impl AliasMatchKind {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact alias",
            Self::Shorthand => "normalized shorthand",
        }
    }
}

#[derive(Default)]
struct AliasIndex<'a> {
    sensitive: HashMap<&'a str, Vec<&'a LocatedNode<'a>>>,
    insensitive: HashMap<String, Vec<&'a LocatedNode<'a>>>,
}

impl<'a> AliasIndex<'a> {
    fn insert(&mut self, case: DefinitionCase, alias: &'a str, candidate: &'a LocatedNode<'a>) {
        let bucket = match case {
            DefinitionCase::Sensitive => self.sensitive.entry(alias).or_default(),
            DefinitionCase::Insensitive => self
                .insensitive
                .entry(alias.to_ascii_lowercase())
                .or_default(),
        };
        if bucket
            .last()
            .is_none_or(|existing| existing.order() != candidate.order())
        {
            bucket.push(candidate);
        }
    }

    fn matches(&self, selector: &str) -> Vec<&'a LocatedNode<'a>> {
        let mut matches = self.sensitive.get(selector).cloned().unwrap_or_default();
        if let Some(insensitive) = self.insensitive.get(&selector.to_ascii_lowercase()) {
            matches.extend(insensitive.iter().copied());
        }
        matches.sort_unstable_by_key(|candidate| candidate.order());
        matches.dedup_by_key(|candidate| candidate.order());
        matches
    }
}

/// One immutable lookup policy shared by excerpts, explanations, outline-root
/// selection, and producer diagnostics.
///
/// Keeping path, ID, exact-alias, and shorthand precedence in this one index
/// prevents a diagnostic from promising a selector that a query surface
/// resolves differently.
pub(crate) struct DocumentSelectorIndex<'a> {
    paths: HashMap<String, &'a LocatedNode<'a>>,
    exact_aliases: AliasIndex<'a>,
    shorthand_aliases: AliasIndex<'a>,
    pub(super) ids: BTreeMap<&'a str, Vec<&'a LocatedNode<'a>>>,
}

impl<'a> DocumentSelectorIndex<'a> {
    pub(crate) fn new(located: &'a [LocatedNode<'a>]) -> Self {
        let mut index = Self {
            paths: HashMap::new(),
            exact_aliases: AliasIndex::default(),
            shorthand_aliases: AliasIndex::default(),
            ids: BTreeMap::new(),
        };
        for candidate in located {
            index.paths.insert(candidate.path().to_string(), candidate);
            index.ids.entry(candidate.id()).or_default().push(candidate);
            let Some(identity) = candidate.identity() else {
                continue;
            };
            for name in &identity.names {
                index.exact_aliases.insert(identity.case, name, candidate);
                if let Some(shorthand) = semantic_name_shorthand(identity.role, name) {
                    index
                        .shorthand_aliases
                        .insert(identity.case, shorthand, candidate);
                }
            }
        }
        index
    }

    pub(crate) fn resolve(
        &self,
        document: &str,
        selector: &str,
    ) -> Result<&'a LocatedNode<'a>, ProjectionError> {
        if let Ok(path) = selector.parse::<OutlinePath>()
            && let Some(candidate) = self.paths.get(&path.to_string())
        {
            return Ok(candidate);
        }
        let ids = self.ids.get(selector).cloned().unwrap_or_default();
        match ids.as_slice() {
            [candidate] => return Ok(candidate),
            [] => {}
            _ => return Err(ambiguous_selector(document, selector, ids)),
        }

        let matches = self.matching_aliases(selector).1;
        match matches.as_slice() {
            [] => Err(ProjectionError::UnknownSelector {
                document: document.to_owned(),
                selector: selector.to_owned(),
            }),
            [candidate] => Ok(candidate),
            _ => Err(ambiguous_selector(document, selector, matches)),
        }
    }

    pub(super) fn matching_aliases(
        &self,
        selector: &str,
    ) -> (AliasMatchKind, Vec<&'a LocatedNode<'a>>) {
        let exact = self.exact_aliases.matches(selector);
        if !exact.is_empty() {
            return (AliasMatchKind::Exact, exact);
        }
        (
            AliasMatchKind::Shorthand,
            self.shorthand_aliases.matches(selector),
        )
    }
}
