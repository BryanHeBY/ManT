//! Exact, explicitly namespaced local path and identity lookup.
use super::{LocatedNode, ProjectionError, ambiguous_selector};
use mant_protocol::ContentSelector;
use std::collections::{BTreeMap, HashMap};

/// Content addressing deliberately excludes semantic names and link activation.
pub(crate) struct DocumentSelectorIndex<'a> {
    paths: HashMap<String, &'a LocatedNode<'a>>,
    pub(super) ids: BTreeMap<&'a str, Vec<&'a LocatedNode<'a>>>,
}

impl<'a> DocumentSelectorIndex<'a> {
    pub(crate) fn new(located: &'a [LocatedNode<'a>]) -> Self {
        let mut index = Self {
            paths: HashMap::new(),
            ids: BTreeMap::new(),
        };
        for candidate in located {
            index.paths.insert(candidate.path().to_string(), candidate);
            index.ids.entry(candidate.id()).or_default().push(candidate);
        }
        index
    }

    /// Synthetic roots must not silently shadow malformed public IR owners.
    pub(crate) fn validate_synthetic_identity(
        &self,
        document: &str,
        selector: &ContentSelector,
        synthetic_id: &str,
        synthetic_path: &str,
    ) -> Result<(), ProjectionError> {
        let ContentSelector::Id { id } = selector else {
            return Ok(());
        };
        if id.as_str() != synthetic_id {
            return Ok(());
        }
        let Some(real_owners) = self.ids.get(synthetic_id) else {
            return Ok(());
        };
        let mut candidates = vec![super::SelectorCandidate {
            path: synthetic_path.to_owned(),
            id: synthetic_id.to_owned(),
        }];
        candidates.extend(real_owners.iter().map(|owner| super::SelectorCandidate {
            path: owner.path().to_string(),
            id: owner.id().to_owned(),
        }));
        Err(ProjectionError::AmbiguousSelector {
            document: document.to_owned(),
            selector: selector.to_string(),
            candidates,
        })
    }

    /// The overview can also collide with an inline anchor that is not an
    /// outline node. A path remains an exact way to read the overview.
    pub(crate) fn validate_overview_identity(
        &self,
        document: &str,
        selector: &ContentSelector,
        has_overview_conflict: impl FnOnce() -> bool,
    ) -> Result<(), ProjectionError> {
        let root = mant_ir::DOCUMENT_ROOT_ID;
        self.validate_synthetic_identity(document, selector, root, "root")?;
        if matches!(selector, ContentSelector::Id { id } if id.as_str() == root)
            && has_overview_conflict()
        {
            return Err(ProjectionError::AmbiguousSelector {
                document: document.to_owned(),
                selector: selector.to_string(),
                candidates: vec![super::SelectorCandidate {
                    path: "root".into(),
                    id: root.into(),
                }],
            });
        }
        Ok(())
    }

    pub(crate) fn resolve(
        &self,
        document: &str,
        selector: &ContentSelector,
    ) -> Result<&'a LocatedNode<'a>, ProjectionError> {
        selector
            .validate()
            .map_err(|_| ProjectionError::InvalidSelector)?;
        let candidate = match selector {
            ContentSelector::Path { path } => self.paths.get(path.as_str()).copied(),
            ContentSelector::Id { id } => match self.ids.get(id.as_str()).map(Vec::as_slice) {
                Some([candidate]) => Some(*candidate),
                Some(candidates) if candidates.len() > 1 => {
                    return Err(ambiguous_selector(
                        document,
                        &selector.to_string(),
                        candidates.to_vec(),
                    ));
                }
                _ => None,
            },
        };
        candidate.ok_or_else(|| ProjectionError::UnknownSelector {
            document: document.to_owned(),
            selector: selector.to_string(),
        })
    }
}
