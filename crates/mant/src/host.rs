//! Explicit process capabilities and one application document snapshot.
use crate::{
    doctor,
    error::{self, Failure, query_execution_failure},
};
use mant_engine::{DocumentResolver, PreparedQueryRequest, PreparedScopeQuery, QueryViewResult};
use mant_ir::ResolvedContent;
use mant_protocol::{
    CatalogQuery, DoctorReport, DocumentCatalog, ScopeQueryResponse, TldrCacheUpdate,
};
use mant_sources::{DocumentSourcesPrune, DocumentSourcesUpdate};
use std::sync::OnceLock;

pub(crate) mod maintenance;

#[cfg(test)]
mod tests;

pub(crate) trait CliHost {
    fn doctor(&self) -> Result<DoctorReport, Failure>;
    fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, Failure>;
    fn query(&self, request: &PreparedQueryRequest<'_>) -> Result<QueryViewResult, Failure>;
    fn query_markdown(&self, source: &str) -> Result<ResolvedContent, Failure>;
    fn resolve_scope(
        &self,
        _scope: &mant_protocol::DocumentScope,
    ) -> Result<mant_engine::LoadedDocumentScope, Failure> {
        Err(Failure::operational(
            "document scopes are unavailable in this host",
        ))
    }
    fn query_scope(
        &self,
        _request: &PreparedScopeQuery<'_>,
    ) -> Result<ScopeQueryResponse, Failure> {
        Err(Failure::operational(
            "document scope queries are unavailable in this host",
        ))
    }
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure>;
    fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure>;
}

#[derive(Default)]
pub(crate) struct SystemHost {
    resolver: OnceLock<DocumentResolver>,
}

impl SystemHost {
    // A host is per invocation/session, not a process-global document cache.
    // Only callers with validated requests may initialize its environment.
    fn resolver(&self) -> &DocumentResolver {
        self.resolver.get_or_init(DocumentResolver::from_system)
    }
}

impl CliHost for SystemHost {
    fn doctor(&self) -> Result<DoctorReport, Failure> {
        Ok(doctor::inspect_system())
    }

    fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, Failure> {
        let prepared =
            mant_loader::PreparedCatalogQuery::new(query).map_err(Failure::operational)?;
        self.resolver()
            .discover_prepared(&prepared)
            .map_err(Failure::operational)
    }

    fn query(&self, request: &PreparedQueryRequest<'_>) -> Result<QueryViewResult, Failure> {
        request
            .execute(self.resolver())
            .map_err(query_execution_failure)
    }

    fn query_markdown(&self, source: &str) -> Result<ResolvedContent, Failure> {
        mant_engine::query_markdown_text(source, None).map_err(Failure::operational)
    }

    fn resolve_scope(
        &self,
        scope: &mant_protocol::DocumentScope,
    ) -> Result<mant_engine::LoadedDocumentScope, Failure> {
        mant_engine::validate_document_scope(scope).map_err(|error| {
            error::scope_query_failure(mant_engine::ScopeQueryError::Load(error))
        })?;
        self.resolver()
            .resolve_scope(scope)
            .map_err(error::scope_query_failure)
    }

    fn query_scope(&self, request: &PreparedScopeQuery<'_>) -> Result<ScopeQueryResponse, Failure> {
        request
            .execute(self.resolver())
            .map_err(error::scope_query_failure)
    }

    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
        maintenance::tldr::update_tldr_cache().map_err(Failure::operational)
    }

    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure> {
        mant_sources::update_document_sources().map_err(Failure::operational)
    }

    fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure> {
        mant_sources::prune_document_sources(dry_run).map_err(Failure::operational)
    }
}
