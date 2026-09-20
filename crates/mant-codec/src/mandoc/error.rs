//! Controlled failures for the complete roff-to-IR boundary.

use std::{fmt, path::Path, path::PathBuf, sync::Arc};

use libmandoc_rs::ExecutionError;

/// Stage of the source-neutral projection pipeline that rejected a native
/// execution report.
///
/// Native parsing and execution failures are represented separately by
/// [`RoffError::Native`].  These stages describe failures after that atomic
/// native boundary has successfully produced its owned report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoffProjectionStage {
    /// Native records could not be assigned to one total ownership plan.
    Ownership,
    /// A sealed ownership plan could not be materialized as semantic IR.
    Materialization,
    /// The materialized document failed a projection-specific invariant.
    Validation,
}

impl fmt::Display for RoffProjectionStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ownership => "ownership",
            Self::Materialization => "materialization",
            Self::Validation => "validation",
        })
    }
}

/// Controlled failure while projecting one successful native execution into
/// ManT's source-neutral document model.
#[derive(Clone, Debug)]
pub struct RoffProjectionError {
    path: PathBuf,
    stage: RoffProjectionStage,
    message: String,
    source: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

impl PartialEq for RoffProjectionError {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.stage == other.stage && self.message == other.message
    }
}

impl Eq for RoffProjectionError {}

impl RoffProjectionError {
    /// Construct a projection failure at one explicit pipeline stage.
    #[must_use]
    pub fn new(
        path: impl Into<PathBuf>,
        stage: RoffProjectionStage,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            stage,
            message: message.into(),
            source: None,
        }
    }

    /// Construct a projection failure while retaining the internal failure as
    /// the next link in the standard error chain.
    ///
    /// This is the adapter used by ownership, materialization and validation
    /// boundaries when their checked APIs are connected to the public codec.
    #[must_use]
    pub fn with_source<E>(path: impl Into<PathBuf>, stage: RoffProjectionStage, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self {
            path: path.into(),
            stage,
            message: source.to_string(),
            source: Some(Arc::new(source)),
        }
    }

    /// Return the logical source path supplied by the caller.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the projection stage that rejected the report.
    #[must_use]
    pub const fn stage(&self) -> RoffProjectionStage {
        self.stage
    }

    /// Return the stable human-readable detail.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for RoffProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: roff {} failed: {}",
            self.path.display(),
            self.stage,
            self.message
        )
    }
}

impl std::error::Error for RoffProjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

/// Failure from the complete roff-to-IR boundary.
///
/// Keeping native execution and projection failures distinct prevents a
/// rejected ownership/materialization plan from being reported as a parser
/// failure or silently falling back to a second lowering path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoffError {
    /// Failure from libmandoc's atomic parse/execute boundary.
    Native(ExecutionError),
    /// Failure while projecting a successful native report into stable IR.
    Projection(RoffProjectionError),
}

impl RoffError {
    /// Return the logical caller-facing source path.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Native(error) => &error.path,
            Self::Projection(error) => error.path(),
        }
    }

    /// Return the stable human-readable detail without the source path.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Native(error) => &error.message,
            Self::Projection(error) => error.message(),
        }
    }

    /// Return the projection stage, or `None` for native execution failures.
    #[must_use]
    pub const fn projection_stage(&self) -> Option<RoffProjectionStage> {
        match self {
            Self::Native(_) => None,
            Self::Projection(error) => Some(error.stage()),
        }
    }
}

impl From<ExecutionError> for RoffError {
    fn from(error: ExecutionError) -> Self {
        Self::Native(error)
    }
}

impl From<RoffProjectionError> for RoffError {
    fn from(error: RoffProjectionError) -> Self {
        Self::Projection(error)
    }
}

impl fmt::Display for RoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(error) => fmt::Display::fmt(error, formatter),
            Self::Projection(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RoffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Native(error) => Some(error),
            Self::Projection(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libmandoc_rs::ExecutionErrorKind;

    #[test]
    fn preserves_native_error_as_its_source() {
        let error = RoffError::from(ExecutionError {
            path: PathBuf::from("probe.1"),
            kind: ExecutionErrorKind::Transfer,
            message: "invalid transfer seal".to_owned(),
        });

        assert_eq!(error.path(), Path::new("probe.1"));
        assert_eq!(error.message(), "invalid transfer seal");
        assert_eq!(error.projection_stage(), None);
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn preserves_projection_stage_message_and_source() {
        let error = RoffError::from(RoffProjectionError::new(
            "probe.1",
            RoffProjectionStage::Ownership,
            "atom has no final consumer",
        ));

        assert_eq!(error.path(), Path::new("probe.1"));
        assert_eq!(error.message(), "atom has no final consumer");
        assert_eq!(
            error.projection_stage(),
            Some(RoffProjectionStage::Ownership)
        );
        assert!(error.to_string().contains("roff ownership failed"));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn retains_internal_projection_source_chain() {
        #[derive(Debug)]
        struct PlanningFailure;

        impl fmt::Display for PlanningFailure {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("record has no unique owner")
            }
        }

        impl std::error::Error for PlanningFailure {}

        let projection = RoffProjectionError::with_source(
            "probe.1",
            RoffProjectionStage::Ownership,
            PlanningFailure,
        );
        assert_eq!(projection.message(), "record has no unique owner");
        assert_eq!(
            std::error::Error::source(&projection)
                .expect("internal source")
                .to_string(),
            "record has no unique owner"
        );
    }
}
