//! Bounded in-process execution of read-only engine queries.

use std::sync::Arc;

use mant_engine::{QueryPolicy, QueryViewResult};
use mant_protocol::{CatalogQuery, DocumentCatalog, QueryRequest};
use tokio::{sync::Semaphore, task};

/// Serializes synchronous parser and filesystem work away from the protocol loop.
#[derive(Debug, Clone)]
pub(super) struct QueryService {
    gate: Arc<Semaphore>,
}

impl QueryService {
    pub(super) fn new() -> Self {
        Self {
            gate: Arc::new(Semaphore::new(1)),
        }
    }

    pub(super) async fn query(&self, request: QueryRequest) -> Result<QueryViewResult, String> {
        let permit = Arc::clone(&self.gate)
            .acquire_owned()
            .await
            .map_err(|_| "MCP query service is shutting down".to_owned())?;
        task::spawn_blocking(move || {
            let _permit = permit;
            mant_engine::execute_query(&request, QueryPolicy::default())
                .map_err(query_error_for_mcp)
        })
        .await
        .map_err(|error| format!("MCP query worker failed: {error}"))?
    }

    pub(super) async fn discover(&self, query: CatalogQuery) -> Result<DocumentCatalog, String> {
        let permit = Arc::clone(&self.gate)
            .acquire_owned()
            .await
            .map_err(|_| "MCP query service is shutting down".to_owned())?;
        task::spawn_blocking(move || {
            let _permit = permit;
            mant_engine::discover_documents(&query)
        })
        .await
        .map_err(|error| format!("MCP document discovery worker failed: {error}"))?
    }
}

pub(super) fn query_error_for_mcp(error: mant_engine::QueryExecutionError) -> String {
    use mant_engine::{ManualLoadError, QueryError, QueryExecutionError};

    fn manual_error_for_mcp(error: &ManualLoadError) -> String {
        match error {
            ManualLoadError::NotFound { name, .. } => format!("manual '{name}' was not found"),
            ManualLoadError::Parse { name, .. } => format!("could not parse manual '{name}'"),
            ManualLoadError::Empty { name, .. } => {
                format!("manual '{name}' contained no readable sections")
            }
        }
    }

    let QueryExecutionError::Query(error) = error else {
        return error.to_string();
    };
    match error {
        QueryError::Markdown { .. } => {
            "could not load or parse the selected Markdown document".to_owned()
        }
        QueryError::EmptyMarkdown { .. } => {
            "the selected Markdown document has no readable content".to_owned()
        }
        QueryError::Registry { .. } => "registered document discovery failed".to_owned(),
        QueryError::Manual(error) => manual_error_for_mcp(&error),
        QueryError::ManualWithTldr { error, topic } => format!(
            "{}; a tldr entry is available for '{topic}'",
            manual_error_for_mcp(&error)
        ),
        QueryError::Tldr { topic, .. } => {
            format!("could not load the tldr entry for '{topic}'")
        }
        other => other.to_string(),
    }
}
