//! Read-only selectors over the single ordered registry snapshot.

use super::{
    BUILTIN_CONTENT_PRIORITY, RegisteredDocument, RegisteredDocumentIndex, RegisteredDocumentMatch,
    RegisteredDocumentOrigin,
};
use crate::document_path::normalize_document_path;
use crate::{SourceConfigError, is_source_name};

struct RegisteredDocumentMatchRef<'a> {
    origin: &'a RegisteredDocumentOrigin,
    selector: String,
    documents: Vec<&'a RegisteredDocument>,
}

impl RegisteredDocumentMatchRef<'_> {
    fn into_owned(self) -> RegisteredDocumentMatch {
        RegisteredDocumentMatch {
            origin: self.origin.clone(),
            selector: self.selector,
            documents: self.documents.into_iter().cloned().collect(),
        }
    }
}

impl RegisteredDocumentIndex {
    /// Resolve ordered path or component-suffix candidates using origin precedence.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicit source is not configured.
    pub fn find(
        &self,
        candidates: &[String],
        source: Option<&str>,
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        if let Some(source) = source
            && (!is_source_name(source) || self.config.get(source).is_none())
        {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not configured"
            )));
        }
        if let Some(source) = source
            && !self.ready_sources.contains(source)
        {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not installed; run 'mant --update-docs' first"
            )));
        }
        self.find_matching(candidates, |document| {
            source.is_none_or(|source| {
                matches!(
                    &document.origin,
                    RegisteredDocumentOrigin::Source(candidate) if candidate == source
                )
            })
        })
    }

    /// Resolve personal documents and positive-priority configured sources.
    ///
    /// This is the portion of the namespace that precedes built-in content.
    /// Lower-priority ambiguities are deliberately not observed in this phase.
    ///
    /// # Errors
    ///
    /// Returns an error when a matching origin contains an ambiguous suffix.
    pub fn find_before_builtin(
        &self,
        candidates: &[String],
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        self.find_matching(candidates, |document| {
            document
                .source_priority
                .is_none_or(|priority| priority > BUILTIN_CONTENT_PRIORITY)
        })
    }

    /// Resolve configured sources at priority zero or below.
    ///
    /// This phase is consulted only after the built-in priority-zero candidate.
    ///
    /// # Errors
    ///
    /// Returns an error when a matching origin contains an ambiguous suffix.
    pub fn find_after_builtin(
        &self,
        candidates: &[String],
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        self.find_matching(candidates, |document| {
            document
                .source_priority
                .is_some_and(|priority| priority <= BUILTIN_CONTENT_PRIORITY)
        })
    }

    /// Return every matching origin before the built-in priority baseline.
    ///
    /// Exact paths suppress suffix matches inside the same origin. Ambiguous
    /// suffixes remain grouped so a content-specific resolver can inspect them
    /// before deciding whether the ambiguity is relevant.
    #[must_use]
    pub fn matches_before_builtin(&self, candidates: &[String]) -> Vec<RegisteredDocumentMatch> {
        self.matching_group_refs(candidates, |document| {
            document
                .source_priority
                .is_none_or(|priority| priority > BUILTIN_CONTENT_PRIORITY)
        })
        .into_iter()
        .map(RegisteredDocumentMatchRef::into_owned)
        .collect()
    }

    /// Return every matching origin at or below the built-in priority baseline.
    #[must_use]
    pub fn matches_after_builtin(&self, candidates: &[String]) -> Vec<RegisteredDocumentMatch> {
        self.matching_group_refs(candidates, |document| {
            document
                .source_priority
                .is_some_and(|priority| priority <= BUILTIN_CONTENT_PRIORITY)
        })
        .into_iter()
        .map(RegisteredDocumentMatchRef::into_owned)
        .collect()
    }

    /// Return matches from one explicitly selected installed source.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is invalid, unconfigured, or not installed.
    pub fn matches_in_source(
        &self,
        candidates: &[String],
        source: &str,
    ) -> Result<Vec<RegisteredDocumentMatch>, SourceConfigError> {
        if !is_source_name(source) || self.config.get(source).is_none() {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not configured"
            )));
        }
        if !self.ready_sources.contains(source) {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not installed; run 'mant --update-docs' first"
            )));
        }
        Ok(self
            .matching_group_refs(candidates, |document| {
                matches!(
                    &document.origin,
                    RegisteredDocumentOrigin::Source(candidate) if candidate == source
                )
            })
            .into_iter()
            .map(RegisteredDocumentMatchRef::into_owned)
            .collect())
    }

    fn find_matching(
        &self,
        candidates: &[String],
        include: impl Fn(&RegisteredDocument) -> bool,
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        let Some(group) = self
            .matching_group_refs(candidates, include)
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        match group.documents.as_slice() {
            [document] => Ok(Some(*document)),
            documents => {
                let choices = documents
                    .iter()
                    .map(|document| document.logical_path.as_str())
                    .collect::<Vec<_>>()
                    .join("', '");
                Err(SourceConfigError::new(format!(
                    "document selector '{}' is ambiguous in {}: '{choices}'",
                    group.selector,
                    origin_label(group.origin)
                )))
            }
        }
    }

    fn matching_group_refs<'a>(
        &'a self,
        candidates: &[String],
        include: impl Fn(&RegisteredDocument) -> bool,
    ) -> Vec<RegisteredDocumentMatchRef<'a>> {
        let origins = self
            .documents
            .iter()
            .filter(|document| include(document))
            .fold(
                Vec::<&RegisteredDocumentOrigin>::new(),
                |mut origins, document| {
                    if !origins.contains(&&document.origin) {
                        origins.push(&document.origin);
                    }
                    origins
                },
            );
        let mut groups = Vec::new();
        for origin in origins {
            for candidate in candidates
                .iter()
                .filter_map(|value| normalize_document_path(value))
            {
                let in_origin = self
                    .documents
                    .iter()
                    .filter(|document| include(document) && &document.origin == origin);
                if let Some(exact) = in_origin
                    .clone()
                    .find(|document| document_paths_equal(&document.logical_path, &candidate))
                {
                    groups.push(RegisteredDocumentMatchRef {
                        origin,
                        selector: candidate,
                        documents: vec![exact],
                    });
                    break;
                }
                let suffix = in_origin
                    .filter(|document| component_suffix_matches(&document.logical_path, &candidate))
                    .collect::<Vec<_>>();
                if !suffix.is_empty() {
                    groups.push(RegisteredDocumentMatchRef {
                        origin,
                        selector: candidate,
                        documents: suffix,
                    });
                    break;
                }
            }
        }
        groups
    }

    /// Resolve one complete address without component-suffix fallback.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicitly addressed source is not configured
    /// or has not been installed yet.
    pub fn find_address(
        &self,
        logical_path: &str,
        origin: &RegisteredDocumentOrigin,
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        if let RegisteredDocumentOrigin::Source(source) = origin {
            if !is_source_name(source) || self.config.get(source).is_none() {
                return Err(SourceConfigError::new(format!(
                    "document source '{source}' is not configured"
                )));
            }
            if !self.ready_sources.contains(source) {
                return Err(SourceConfigError::new(format!(
                    "document source '{source}' is not installed; run 'mant --update-docs' first"
                )));
            }
        }
        let Some(logical_path) = normalize_document_path(logical_path) else {
            return Ok(None);
        };
        Ok(self.documents.iter().find(|document| {
            &document.origin == origin
                && document_paths_equal(&document.logical_path, &logical_path)
        }))
    }
}

fn document_paths_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn component_suffix_matches(path: &str, selector: &str) -> bool {
    document_paths_equal(path, selector)
        || path.len() > selector.len()
            && path.as_bytes().get(path.len() - selector.len() - 1) == Some(&b'/')
            && document_paths_equal(&path[path.len() - selector.len()..], selector)
}

fn origin_label(origin: &RegisteredDocumentOrigin) -> String {
    match origin {
        RegisteredDocumentOrigin::Documents => "personal documents".to_owned(),
        RegisteredDocumentOrigin::Source(name) => format!("source '{name}'"),
    }
}

#[cfg(test)]
mod tests;
