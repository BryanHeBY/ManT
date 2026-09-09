//! Explicit process capabilities and one application document snapshot.
use crate::{
    doctor,
    error::{self, Failure, query_failure},
};
use mant_engine::LoadPolicy;
use mant_ir::ResolvedContent;
use mant_protocol::{
    CatalogQuery, DoctorReport, DocumentCatalog, QueryRequest, ScopeQueryRequest,
    ScopeQueryResponse, TldrCacheUpdate,
};
use mant_sources::{DocumentSourcesPrune, DocumentSourcesUpdate};

pub(crate) mod maintenance;

pub(crate) trait CliHost {
    fn doctor(&self) -> Result<DoctorReport, Failure>;
    fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, Failure>;
    fn query(&self, request: &QueryRequest, policy: LoadPolicy)
    -> Result<ResolvedContent, Failure>;
    fn query_markdown(&self, source: &str) -> Result<ResolvedContent, Failure>;
    fn resolve_scope(
        &self,
        _scope: &mant_protocol::DocumentScope,
    ) -> Result<mant_engine::LoadedDocumentScope, Failure> {
        Err(Failure::operational(
            "document scopes are unavailable in this host",
        ))
    }
    fn query_scope(&self, _request: &ScopeQueryRequest) -> Result<ScopeQueryResponse, Failure> {
        Err(Failure::operational(
            "document scope queries are unavailable in this host",
        ))
    }
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure>;
    fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure>;
}

pub(crate) struct SystemHost {
    resolver: mant_engine::DocumentResolver,
}

impl Default for SystemHost {
    fn default() -> Self {
        Self {
            resolver: mant_engine::DocumentResolver::from_system(),
        }
    }
}

impl CliHost for SystemHost {
    fn doctor(&self) -> Result<DoctorReport, Failure> {
        Ok(doctor::inspect_system())
    }

    fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, Failure> {
        self.resolver.discover(query).map_err(Failure::operational)
    }

    fn query(
        &self,
        request: &QueryRequest,
        policy: LoadPolicy,
    ) -> Result<ResolvedContent, Failure> {
        self.resolver
            .resolve(request, policy)
            .map_err(query_failure)
    }

    fn query_markdown(&self, source: &str) -> Result<ResolvedContent, Failure> {
        mant_engine::query_markdown_text(source, None).map_err(Failure::operational)
    }

    fn resolve_scope(
        &self,
        scope: &mant_protocol::DocumentScope,
    ) -> Result<mant_engine::LoadedDocumentScope, Failure> {
        self.resolver
            .resolve_scope(scope)
            .map_err(error::scope_query_failure)
    }

    fn query_scope(&self, request: &ScopeQueryRequest) -> Result<ScopeQueryResponse, Failure> {
        self.resolver
            .execute_scope_query(request)
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
