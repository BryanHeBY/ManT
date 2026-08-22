//! Versioned report contract for read-only installation diagnostics.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Producer;

/// Exact schema marker for an installation health report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum DoctorSchema {
    /// Version 1 of the doctor report protocol.
    #[serde(rename = "mant.doctor/v1")]
    V1,
}

/// Aggregate health derived from every doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DoctorOutcome {
    /// No warning or error was detected.
    Healthy,
    /// At least one non-fatal condition deserves attention.
    Warning,
    /// At least one promised local capability is broken.
    Error,
}

/// Severity of one stable doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DoctorCheckStatus {
    /// The checked capability is available and healthy.
    Ok,
    /// The check records useful context without requiring action.
    Info,
    /// An optional or recoverable capability needs attention.
    Warning,
    /// A promised local capability is unusable.
    Error,
}

/// Effective local paths and host identity inspected by `mant --doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorEnvironment {
    /// Rust host operating-system family.
    pub os: String,
    /// Rust host processor architecture.
    pub arch: String,
    /// Platform-native `ManT` data root, when it could be derived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_root: Option<String>,
    /// Effective `sources.toml` path, when the data root is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    /// Personal Markdown root, when the data root is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents_root: Option<String>,
    /// Managed source root, when the data root is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources_root: Option<String>,
    /// Native manual roots in effective precedence order.
    pub manual_roots: Vec<String>,
    /// tldr cache roots in effective read order.
    pub tldr_roots: Vec<String>,
}

/// One independently actionable installation check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    /// Stable machine-readable check identifier.
    pub code: String,
    /// Optional configured source or other logical subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Check severity.
    pub status: DoctorCheckStatus,
    /// Concise human-readable result.
    pub message: String,
    /// Additional bounded evidence, when useful.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    /// Explicit next command or corrective action, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// Stable counts for one complete doctor run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSummary {
    /// Successful checks.
    pub ok: u32,
    /// Informational checks.
    pub info: u32,
    /// Non-fatal warnings.
    pub warnings: u32,
    /// Failed capabilities.
    pub errors: u32,
}

/// Complete read-only installation health report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:doctor:v1"))]
pub struct DoctorReport {
    /// Exact response schema discriminator.
    pub schema: DoctorSchema,
    /// Process provenance.
    pub producer: Producer,
    /// Aggregate health derived from [`Self::checks`].
    pub outcome: DoctorOutcome,
    /// Effective host and storage paths.
    pub environment: DoctorEnvironment,
    /// Deterministically ordered checks.
    pub checks: Vec<DoctorCheck>,
    /// Counts derived from [`Self::checks`].
    pub summary: DoctorSummary,
}

impl DoctorReport {
    /// Build a report whose summary and aggregate outcome cannot disagree with
    /// its checks.
    #[must_use]
    pub fn new(
        producer: Producer,
        environment: DoctorEnvironment,
        checks: Vec<DoctorCheck>,
    ) -> Self {
        let mut summary = DoctorSummary::default();
        for check in &checks {
            match check.status {
                DoctorCheckStatus::Ok => summary.ok += 1,
                DoctorCheckStatus::Info => summary.info += 1,
                DoctorCheckStatus::Warning => summary.warnings += 1,
                DoctorCheckStatus::Error => summary.errors += 1,
            }
        }
        let outcome = if summary.errors > 0 {
            DoctorOutcome::Error
        } else if summary.warnings > 0 {
            DoctorOutcome::Warning
        } else {
            DoctorOutcome::Healthy
        };
        Self {
            schema: DoctorSchema::V1,
            producer,
            outcome,
            environment,
            checks,
            summary,
        }
    }

    /// Return whether a promised capability failed.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        self.summary.errors > 0
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DoctorCheck, DoctorCheckStatus, DoctorEnvironment, DoctorOutcome, DoctorReport};
    use crate::Producer;

    fn environment() -> DoctorEnvironment {
        DoctorEnvironment {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            data_root: Some("/data/mant".to_owned()),
            config_path: Some("/data/mant/sources.toml".to_owned()),
            documents_root: Some("/data/mant/documents".to_owned()),
            sources_root: Some("/data/mant/sources".to_owned()),
            manual_roots: vec!["/usr/share/man".to_owned()],
            tldr_roots: Vec::new(),
        }
    }

    fn producer() -> Producer {
        Producer {
            name: "mant".to_owned(),
            version: "0.9.0".to_owned(),
            engine: None,
        }
    }

    #[test]
    fn report_derives_warning_outcome_and_counts() {
        let report = DoctorReport::new(
            producer(),
            environment(),
            vec![
                DoctorCheck {
                    code: "runtime.libmandoc".to_owned(),
                    subject: None,
                    status: DoctorCheckStatus::Ok,
                    message: "parser probe succeeded".to_owned(),
                    details: Vec::new(),
                    remediation: None,
                },
                DoctorCheck {
                    code: "sources.not-installed".to_owned(),
                    subject: Some("team".to_owned()),
                    status: DoctorCheckStatus::Warning,
                    message: "configured source is not installed".to_owned(),
                    details: Vec::new(),
                    remediation: Some("mant --update-docs".to_owned()),
                },
            ],
        );

        assert_eq!(report.outcome, DoctorOutcome::Warning);
        assert_eq!(report.summary.ok, 1);
        assert_eq!(report.summary.warnings, 1);
        assert!(!report.has_errors());
        assert_eq!(
            serde_json::to_value(report).expect("doctor report"),
            json!({
                "schema": "mant.doctor/v1",
                "producer": { "name": "mant", "version": "0.9.0" },
                "outcome": "warning",
                "environment": {
                    "os": "linux",
                    "arch": "x86_64",
                    "dataRoot": "/data/mant",
                    "configPath": "/data/mant/sources.toml",
                    "documentsRoot": "/data/mant/documents",
                    "sourcesRoot": "/data/mant/sources",
                    "manualRoots": ["/usr/share/man"],
                    "tldrRoots": []
                },
                "checks": [
                    {
                        "code": "runtime.libmandoc",
                        "status": "ok",
                        "message": "parser probe succeeded"
                    },
                    {
                        "code": "sources.not-installed",
                        "subject": "team",
                        "status": "warning",
                        "message": "configured source is not installed",
                        "remediation": "mant --update-docs"
                    }
                ],
                "summary": { "ok": 1, "info": 0, "warnings": 1, "errors": 0 }
            })
        );
    }

    #[test]
    fn any_error_makes_the_report_fail() {
        let report = DoctorReport::new(
            producer(),
            environment(),
            vec![DoctorCheck {
                code: "paths.data-root".to_owned(),
                subject: None,
                status: DoctorCheckStatus::Error,
                message: "data root is unavailable".to_owned(),
                details: Vec::new(),
                remediation: Some("set HOME".to_owned()),
            }],
        );

        assert_eq!(report.outcome, DoctorOutcome::Error);
        assert!(report.has_errors());
    }
}
