//! Pure source-maintenance reports, available without filesystem update capabilities.

use serde::Serialize;

/// Outcome for one configured repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceUpdateAction {
    /// New content was installed atomically.
    Updated,
    /// The installed revision and configuration fingerprint were current.
    Unchanged,
    /// This source failed without aborting updates for other sources.
    Failed,
}

/// Stable per-source update result printed by the native CLI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUpdateResult {
    /// Configured source name.
    pub source: String,
    /// Outcome of this source update.
    pub action: SourceUpdateAction,
    /// Installed or observed source revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Number of installed Markdown documents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    /// Human-readable failure detail for [`SourceUpdateAction::Failed`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Exact schema marker for a document-source update report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum DocumentSourcesUpdateSchema {
    /// Version 2 of the native source-update report.
    #[serde(rename = "mant.sources-update/v2")]
    V2,
}

/// Complete result of one `--update-docs` run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourcesUpdate {
    /// Exact report schema discriminator.
    pub schema: DocumentSourcesUpdateSchema,
    /// Platform-native path of the configuration used by this run.
    pub config: String,
    /// Per-source results in source-name order, independently of lookup priority.
    pub sources: Vec<SourceUpdateResult>,
    /// Updater-owned directories no longer present in configuration.
    pub orphaned: Vec<OrphanedSource>,
}

impl DocumentSourcesUpdate {
    /// Return whether at least one configured source failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.sources
            .iter()
            .any(|source| source.action == SourceUpdateAction::Failed)
    }
}

/// One updater-owned source directory absent from the active configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedSource {
    /// Installed source name derived from its directory.
    pub source: String,
    /// Platform-native source directory path.
    pub path: String,
    /// Whether ownership metadata makes automated removal safe.
    pub removable: bool,
    /// Last installed revision, when trusted metadata is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Last installed document count, when trusted metadata is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    /// Reason the candidate cannot be removed automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of one explicit orphan cleanup candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourcePruneAction {
    /// A dry run found a safe candidate that would be removed.
    WouldRemove,
    /// The updater-owned directory was removed successfully.
    Removed,
    /// Ownership or identity checks did not permit removal.
    Refused,
    /// Removal began or was attempted but could not complete safely.
    Failed,
}

/// Stable per-source result printed by `--prune-docs`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePruneResult {
    /// Installed source name.
    pub source: String,
    /// Platform-native source directory path.
    pub path: String,
    /// Outcome of this cleanup candidate.
    pub action: SourcePruneAction,
    /// Last installed revision, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Last installed document count, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    /// Refusal or failure detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Exact schema marker for an orphan cleanup report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum DocumentSourcesPruneSchema {
    /// Version 1 of the native source-prune report.
    #[serde(rename = "mant.sources-prune/v1")]
    V1,
}

/// Complete result of one explicit source prune or dry run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourcesPrune {
    /// Exact report schema discriminator.
    pub schema: DocumentSourcesPruneSchema,
    /// Platform-native path of the configuration used by this run.
    pub config: String,
    /// Whether no filesystem removals were attempted.
    pub dry_run: bool,
    /// Per-candidate results in lexical source-name order.
    pub sources: Vec<SourcePruneResult>,
}

impl DocumentSourcesPrune {
    /// Return whether any candidate was refused or failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.sources.iter().any(|source| {
            matches!(
                source.action,
                SourcePruneAction::Refused | SourcePruneAction::Failed
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_report_serialization_is_available_without_execution() {
        let report = DocumentSourcesUpdate {
            schema: DocumentSourcesUpdateSchema::V2,
            config: "config/sources.toml".into(),
            sources: vec![SourceUpdateResult {
                source: "sample".into(),
                action: SourceUpdateAction::Failed,
                revision: None,
                documents: None,
                error: Some("offline".into()),
            }],
            orphaned: vec![OrphanedSource {
                source: "old".into(),
                path: "sources/old".into(),
                removable: false,
                revision: None,
                documents: None,
                error: None,
            }],
        };
        // TOML exercises serde field names and omission with the crate's
        // existing serializer dependency, without enabling update or doing I/O.
        let expected: toml::Value = toml::from_str(
            r#"
schema = "mant.sources-update/v2"
config = "config/sources.toml"
[[sources]]
source = "sample"
action = "failed"
error = "offline"
[[orphaned]]
source = "old"
path = "sources/old"
removable = false
"#,
        )
        .unwrap();
        assert_eq!(toml::Value::try_from(&report).unwrap(), expected);
        for (action, failed) in [
            (SourceUpdateAction::Updated, false),
            (SourceUpdateAction::Unchanged, false),
            (SourceUpdateAction::Failed, true),
        ] {
            let mut report = report.clone();
            report.sources[0].action = action;
            assert_eq!(report.has_failures(), failed);
        }
    }

    #[test]
    fn prune_report_serialization_and_failure_policy_are_pure() {
        let mut report = DocumentSourcesPrune {
            schema: DocumentSourcesPruneSchema::V1,
            config: "sources.toml".into(),
            dry_run: true,
            sources: vec![SourcePruneResult {
                source: "old".into(),
                path: "sources/old".into(),
                action: SourcePruneAction::WouldRemove,
                revision: Some("abc".into()),
                documents: Some(3),
                error: None,
            }],
        };
        let expected: toml::Value = toml::from_str(
            r#"
schema = "mant.sources-prune/v1"
config = "sources.toml"
dryRun = true
[[sources]]
source = "old"
path = "sources/old"
action = "would-remove"
revision = "abc"
documents = 3
"#,
        )
        .unwrap();
        assert_eq!(toml::Value::try_from(&report).unwrap(), expected);
        for (action, failed) in [
            (SourcePruneAction::WouldRemove, false),
            (SourcePruneAction::Removed, false),
            (SourcePruneAction::Refused, true),
            (SourcePruneAction::Failed, true),
        ] {
            report.sources[0].action = action;
            assert_eq!(report.has_failures(), failed);
        }
        report.sources.clear();
        assert!(!report.has_failures());
    }
}
