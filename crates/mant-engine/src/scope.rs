//! Resolves typed document links into bounded, deterministic query scopes.

use std::collections::{BTreeMap, VecDeque};
use std::{error::Error, fmt, io::Write};

use mant_ir::visit::{Visit, walk_block, walk_definition_item, walk_inline};
use mant_ir::{
    Block, DefinitionItem, DocumentAddress, Inline, ResolvedContent, SemanticDocumentReference,
    ValueDomain,
};
use mant_protocol::{
    DocumentEdge, DocumentEdgeKind, DocumentFrontier, DocumentScope, DocumentSelector,
    MAX_DOCUMENT_SELECTOR_CHARS, MAX_SCOPE_CONTENT_BYTES, MAX_SCOPE_DEPTH,
    MAX_SCOPE_DOCUMENT_LIMIT, MAX_SCOPE_DOCUMENTS, MAX_SEMANTIC_ENTRY_CHARS, QueryInput,
    QueryRequest, RequestSchema, ResolvedDocumentScope, ScopeQueryRequest, ScopeQueryResponse,
    ScopeQueryResult, ScopeQuerySchema, ScopeQueryView, ScopeSearch, ScopeTextError,
    ScopedDocument, ScopedExplanation, ScopedQueryFailure, ScopedSearchDocument, SearchQuery,
    TraversalLimit, UnresolvedDocument, validate_scope_text,
};

use crate::{
    DocumentResolver, ProjectionError, QueryError, QueryPolicy,
    query::select_explanation_with_text_hint, search_query, validate_search_query,
};

/// A logical scope together with the loaded documents in matching order.
#[derive(Debug, Clone)]
pub struct LoadedDocumentScope {
    /// Transport-neutral logical graph.
    pub scope: ResolvedDocumentScope,
    /// Loaded documents in the same order as [`ResolvedDocumentScope::documents`].
    pub documents: Vec<ResolvedContent>,
}

/// Invalid scope configuration or failure to resolve any initial document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeQueryError {
    /// No initial document was supplied.
    EmptyScope,
    /// The initial document count exceeded the native bound.
    TooManyDocuments,
    /// Traversal depth exceeded the native bound.
    DepthLimit,
    /// The document budget was zero, too large, or smaller than the root set.
    DocumentLimit,
    /// Traversal limits were supplied while link following was disabled.
    TraversalLimitsRequireLinks,
    /// A logical document selector violated its native bound.
    DocumentSelector(ScopeTextError),
    /// A semantic-entry selector violated its native bound.
    EntrySelector(ScopeTextError),
    /// Search configuration was invalid.
    Search(crate::SearchError),
    /// No initial document could be loaded.
    NoResolvedDocuments {
        /// Compact seed-resolution diagnostics.
        reasons: Vec<String>,
    },
}

impl fmt::Display for ScopeQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyScope => formatter.write_str("at least one document is required"),
            Self::TooManyDocuments => write!(
                formatter,
                "at most {MAX_SCOPE_DOCUMENTS} initial documents are allowed"
            ),
            Self::DepthLimit => write!(
                formatter,
                "maximum link depth must not exceed {MAX_SCOPE_DEPTH}"
            ),
            Self::DocumentLimit => write!(
                formatter,
                "document limit must include every initial document and not exceed {MAX_SCOPE_DOCUMENT_LIMIT}"
            ),
            Self::TraversalLimitsRequireLinks => {
                formatter.write_str("maxDepth and maxDocuments require followLinks=true")
            }
            Self::DocumentSelector(error) => {
                write!(
                    formatter,
                    "document selector {}",
                    scope_text_error_message(*error)
                )
            }
            Self::EntrySelector(error) => {
                write!(
                    formatter,
                    "semantic entry {}",
                    scope_text_error_message(*error)
                )
            }
            Self::Search(error) => error.fmt(formatter),
            Self::NoResolvedDocuments { reasons } => {
                formatter.write_str("none of the initial documents could be resolved")?;
                if !reasons.is_empty() {
                    write!(formatter, ": {}", reasons.join("; "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ScopeQueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Search(error) => Some(error),
            Self::EmptyScope
            | Self::TooManyDocuments
            | Self::DepthLimit
            | Self::DocumentLimit
            | Self::TraversalLimitsRequireLinks
            | Self::DocumentSelector(_)
            | Self::EntrySelector(_)
            | Self::NoResolvedDocuments { .. } => None,
        }
    }
}

/// Validate the closed scope-query contract before document I/O.
///
/// # Errors
///
/// Returns the first violated bound or projection invariant.
pub fn validate_scope_query_request(request: &ScopeQueryRequest) -> Result<(), ScopeQueryError> {
    validate_document_scope(&request.scope)?;
    match &request.view {
        ScopeQueryView::Explain { entry } => validate_scope_text(entry, MAX_SEMANTIC_ENTRY_CHARS)
            .map_err(ScopeQueryError::EntrySelector),
        ScopeQueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        } => validate_search_query(&SearchQuery {
            pattern: pattern.clone(),
            syntax: *syntax,
            case: *case,
            scope: *scope,
            word: *word,
            context_lines: *context_lines,
            limit: *limit,
            offset: *offset,
        })
        .map_err(ScopeQueryError::Search),
    }
}

fn validate_document_scope(scope: &DocumentScope) -> Result<(), ScopeQueryError> {
    if scope.documents.is_empty() {
        return Err(ScopeQueryError::EmptyScope);
    }
    if scope.documents.len() > MAX_SCOPE_DOCUMENTS {
        return Err(ScopeQueryError::TooManyDocuments);
    }
    for selector in &scope.documents {
        validate_scope_text(&selector.selector, MAX_DOCUMENT_SELECTOR_CHARS)
            .map_err(ScopeQueryError::DocumentSelector)?;
    }
    if !scope.traversal.follow_links
        && (scope.traversal.max_depth.is_some() || scope.traversal.max_documents.is_some())
    {
        return Err(ScopeQueryError::TraversalLimitsRequireLinks);
    }
    if scope.traversal.effective_max_depth() > MAX_SCOPE_DEPTH {
        return Err(ScopeQueryError::DepthLimit);
    }
    let root_count = u32::try_from(scope.documents.len()).unwrap_or(u32::MAX);
    if scope.traversal.effective_max_documents() < root_count
        || scope.traversal.effective_max_documents() > MAX_SCOPE_DOCUMENT_LIMIT
    {
        return Err(ScopeQueryError::DocumentLimit);
    }
    Ok(())
}

fn scope_text_error_message(error: ScopeTextError) -> String {
    match error {
        ScopeTextError::Empty => "must not be empty".to_owned(),
        ScopeTextError::ControlCharacter => "must not contain control characters".to_owned(),
        ScopeTextError::TooLong { maximum } => {
            format!("must not exceed {maximum} Unicode scalar values")
        }
    }
}

impl DocumentResolver {
    /// Resolve initial documents and their typed outbound links breadth-first.
    ///
    /// # Errors
    ///
    /// Returns an invalid-scope error, or an aggregate error when no initial
    /// document is readable. Individual missing links remain in the result.
    pub fn resolve_scope(
        &self,
        query: &DocumentScope,
    ) -> Result<LoadedDocumentScope, ScopeQueryError> {
        validate_document_scope(query)?;
        let mut resolution = ScopeResolution::new(query);
        resolution.resolve_roots(self);
        if resolution.documents.is_empty() {
            return Err(ScopeQueryError::NoResolvedDocuments {
                reasons: resolution
                    .graph
                    .unresolved
                    .iter()
                    .map(|failure| failure.reason.clone())
                    .collect(),
            });
        }
        if query.traversal.follow_links {
            resolution.follow_links(self);
        }
        Ok(resolution.finish())
    }

    /// Resolve a scope and apply its closed multi-document projection.
    ///
    /// # Errors
    ///
    /// Returns request validation, resolution, or search errors. Ordinary
    /// per-document explanation misses do not fail the aggregate query.
    pub fn execute_scope_query(
        &self,
        request: &ScopeQueryRequest,
    ) -> Result<ScopeQueryResponse, ScopeQueryError> {
        validate_scope_query_request(request)?;
        let loaded = self.resolve_scope(&request.scope)?;
        let result = match &request.view {
            ScopeQueryView::Explain { entry } => execute_scope_explain(&loaded, entry),
            ScopeQueryView::Search {
                pattern,
                syntax,
                case,
                scope,
                word,
                context_lines,
                limit,
                offset,
            } => execute_scope_search(
                &loaded,
                &SearchQuery {
                    pattern: pattern.clone(),
                    syntax: *syntax,
                    case: *case,
                    scope: *scope,
                    word: *word,
                    context_lines: *context_lines,
                    limit: *limit,
                    offset: *offset,
                },
            )?,
        };
        Ok(ScopeQueryResponse {
            schema: ScopeQuerySchema::V0Dot11,
            scope: loaded.scope,
            result,
        })
    }

    fn resolve_selector(
        &self,
        selector: &DocumentSelector,
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, QueryError> {
        self.resolve(
            &QueryRequest {
                schema: RequestSchema::V0Dot11,
                input: QueryInput::Document {
                    selector: selector.selector.clone(),
                    source: selector.source.clone(),
                    manual_section: selector.manual_section.clone(),
                },
                view: mant_protocol::QueryView::Full {},
            },
            policy,
        )
    }
}

struct ScopeResolution {
    graph: ResolvedDocumentScope,
    documents: Vec<ResolvedContent>,
    positions: BTreeMap<DocumentAddress, usize>,
    queue: VecDeque<usize>,
    content_bytes: u64,
}

impl ScopeResolution {
    fn new(query: &DocumentScope) -> Self {
        Self {
            graph: ResolvedDocumentScope {
                query: query.clone(),
                documents: Vec::new(),
                edges: Vec::new(),
                frontier: Vec::new(),
                unresolved: Vec::new(),
            },
            documents: Vec::new(),
            positions: BTreeMap::new(),
            queue: VecDeque::new(),
            content_bytes: 0,
        }
    }

    fn resolve_roots(&mut self, resolver: &DocumentResolver) {
        for (root_index, selector) in self.graph.query.documents.clone().iter().enumerate() {
            match resolver.resolve_selector(selector, QueryPolicy::Combined) {
                Ok(bundle) => {
                    self.insert_root(bundle, selector, root_index);
                }
                Err(error) => self.graph.unresolved.push(UnresolvedDocument {
                    from: None,
                    selector: selector.clone(),
                    reason: error.to_string(),
                }),
            }
        }
    }

    fn insert_root(
        &mut self,
        bundle: ResolvedContent,
        selector: &DocumentSelector,
        root_index: usize,
    ) {
        let Some(address) = bundle.address.clone() else {
            self.graph.unresolved.push(UnresolvedDocument {
                from: None,
                selector: selector.clone(),
                reason: "selector did not resolve to a registered document".to_owned(),
            });
            return;
        };
        let root_index = u16::try_from(root_index).unwrap_or(u16::MAX);
        if let Some(position) = self.positions.get(&address).copied() {
            let roots = &mut self.graph.documents[position].root_indices;
            if !roots.contains(&root_index) {
                roots.push(root_index);
            }
            return;
        }
        if !self.reserve_content_bytes(&bundle) {
            self.graph.unresolved.push(UnresolvedDocument {
                from: None,
                selector: selector.clone(),
                reason: format!(
                    "document exceeds the {} MiB aggregate scope content budget",
                    MAX_SCOPE_CONTENT_BYTES / (1024 * 1024)
                ),
            });
            return;
        }
        let position = self.documents.len();
        self.positions.insert(address.clone(), position);
        self.documents.push(bundle);
        self.graph.documents.push(ScopedDocument {
            address,
            depth: 0,
            root_indices: vec![root_index],
            reached_from: Vec::new(),
        });
        self.queue.push_back(position);
    }

    fn follow_links(&mut self, resolver: &DocumentResolver) {
        while let Some(position) = self.queue.pop_front() {
            let depth = self.graph.documents[position].depth;
            if depth >= self.graph.query.traversal.effective_max_depth() {
                self.record_depth_frontier(position);
                continue;
            }
            let from = self.graph.documents[position].address.clone();
            for reference in document_references(&self.documents[position]) {
                self.follow_reference(resolver, &from, depth, &reference);
            }
        }
    }

    fn record_depth_frontier(&mut self, position: usize) {
        let from = self.graph.documents[position].address.clone();
        for reference in document_references(&self.documents[position]) {
            if let Some(address) = reference.exact_address(&from) {
                let edge = DocumentEdge {
                    from: from.clone(),
                    to: address,
                    kind: reference.kind,
                };
                if self.record_existing_edge(&edge) {
                    continue;
                }
            }
            self.record_frontier(&from, &reference, TraversalLimit::MaxDepth);
        }
    }

    fn follow_reference(
        &mut self,
        resolver: &DocumentResolver,
        from: &DocumentAddress,
        depth: u16,
        reference: &DocumentReference,
    ) {
        if let Some(address) = reference.exact_address(from) {
            let edge = DocumentEdge {
                from: from.clone(),
                to: address.clone(),
                kind: reference.kind,
            };
            if self.record_existing_edge(&edge) {
                return;
            }
            if self.at_document_limit() {
                self.record_frontier(from, reference, TraversalLimit::MaxDocuments);
                return;
            }
        } else if self.at_document_limit() {
            self.record_frontier(from, reference, TraversalLimit::MaxDocuments);
            return;
        }

        let Some(selector) = reference.selector(from) else {
            self.graph.unresolved.push(UnresolvedDocument {
                from: Some(from.clone()),
                selector: reference.fallback_selector(),
                reason: "relative document link escapes its registered namespace".to_owned(),
            });
            return;
        };
        let policy = if reference.kind == DocumentEdgeKind::Manual {
            QueryPolicy::ManualOnly
        } else {
            QueryPolicy::Combined
        };
        let bundle = match resolver.resolve_selector(&selector, policy) {
            Ok(bundle) => bundle,
            Err(error) => {
                self.graph.unresolved.push(UnresolvedDocument {
                    from: Some(from.clone()),
                    selector,
                    reason: error.to_string(),
                });
                return;
            }
        };
        let Some(address) = bundle.address.clone() else {
            self.graph.unresolved.push(UnresolvedDocument {
                from: Some(from.clone()),
                selector,
                reason: "link did not resolve to a registered document".to_owned(),
            });
            return;
        };
        let edge = DocumentEdge {
            from: from.clone(),
            to: address.clone(),
            kind: reference.kind,
        };
        if self.record_existing_edge(&edge) {
            return;
        }
        if self.at_document_limit() {
            self.record_frontier(from, reference, TraversalLimit::MaxDocuments);
            return;
        }
        if !self.insert_linked(bundle, address, from, depth + 1, edge) {
            self.record_frontier(from, reference, TraversalLimit::MaxContentBytes);
        }
    }

    fn record_existing_edge(&mut self, edge: &DocumentEdge) -> bool {
        let Some(position) = self.positions.get(&edge.to).copied() else {
            return false;
        };
        if !self.graph.edges.contains(edge) {
            self.graph.edges.push(edge.clone());
        }
        if edge.to != edge.from
            && !self.graph.documents[position]
                .reached_from
                .contains(&edge.from)
        {
            self.graph.documents[position]
                .reached_from
                .push(edge.from.clone());
        }
        true
    }

    fn insert_linked(
        &mut self,
        bundle: ResolvedContent,
        address: DocumentAddress,
        from: &DocumentAddress,
        depth: u16,
        edge: DocumentEdge,
    ) -> bool {
        if !self.reserve_content_bytes(&bundle) {
            return false;
        }
        if !self.graph.edges.contains(&edge) {
            self.graph.edges.push(edge);
        }
        let position = self.documents.len();
        self.positions.insert(address.clone(), position);
        self.documents.push(bundle);
        self.graph.documents.push(ScopedDocument {
            address,
            depth,
            root_indices: Vec::new(),
            reached_from: vec![from.clone()],
        });
        self.queue.push_back(position);
        true
    }

    fn reserve_content_bytes(&mut self, bundle: &ResolvedContent) -> bool {
        let bytes = normalized_content_bytes(bundle);
        let Some(total) = self.content_bytes.checked_add(bytes) else {
            return false;
        };
        if total > MAX_SCOPE_CONTENT_BYTES {
            return false;
        }
        self.content_bytes = total;
        true
    }

    fn at_document_limit(&self) -> bool {
        u32::try_from(self.documents.len()).unwrap_or(u32::MAX)
            >= self.graph.query.traversal.effective_max_documents()
    }

    fn record_frontier(
        &mut self,
        from: &DocumentAddress,
        reference: &DocumentReference,
        limit: TraversalLimit,
    ) {
        let frontier = DocumentFrontier {
            from: from.clone(),
            target: reference
                .selector(from)
                .unwrap_or_else(|| reference.fallback_selector()),
            kind: reference.kind,
            limit,
        };
        if !self.graph.frontier.contains(&frontier) {
            self.graph.frontier.push(frontier);
        }
    }

    fn finish(self) -> LoadedDocumentScope {
        LoadedDocumentScope {
            scope: self.graph,
            documents: self.documents,
        }
    }
}

/// Count the retained semantic payload without allocating an additional
/// serialized copy. The count intentionally follows the normalized IR rather
/// than compressed or on-disk source bytes: the IR is what scope resolution
/// retains for all later projections.
fn normalized_content_bytes(content: &ResolvedContent) -> u64 {
    let mut counter = ByteCounter::default();
    if let Some(document) = &content.document {
        serde_json::to_writer(&mut counter, document)
            .expect("writing normalized document bytes to a counter cannot fail");
    }
    if let Some(tldr) = &content.tldr {
        serde_json::to_writer(&mut counter, tldr)
            .expect("writing normalized tldr bytes to a counter cannot fail");
    }
    counter.0
}

#[derive(Default)]
struct ByteCounter(u64);

impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn execute_scope_explain(loaded: &LoadedDocumentScope, entry: &str) -> ScopeQueryResult {
    let mut matches = Vec::new();
    let mut missed = 0_u32;
    let mut failures = Vec::new();
    for (scoped, bundle) in loaded.scope.documents.iter().zip(&loaded.documents) {
        match select_explanation_with_text_hint(bundle, entry) {
            Ok(excerpt) => matches.push(ScopedExplanation {
                address: scoped.address.clone(),
                depth: scoped.depth,
                excerpt,
            }),
            Err(ProjectionError::UnknownSelector { .. }) => {
                missed = missed.saturating_add(1);
            }
            Err(error) => failures.push(ScopedQueryFailure {
                address: scoped.address.clone(),
                reason: error.to_string(),
            }),
        }
    }
    ScopeQueryResult::Explain {
        entry: entry.to_owned(),
        matches,
        missed,
        failures,
    }
}

fn execute_scope_search(
    loaded: &LoadedDocumentScope,
    query: &SearchQuery,
) -> Result<ScopeQueryResult, ScopeQueryError> {
    let mut total = 0_u32;
    let mut remaining_skip = query.offset;
    let mut remaining_take = query.limit;
    let mut groups = Vec::new();
    for (scoped, bundle) in loaded.scope.documents.iter().zip(&loaded.documents) {
        let document_ordinal_base = total;
        let local = search_query(
            bundle,
            &SearchQuery {
                offset: remaining_skip,
                limit: remaining_take.max(1),
                ..query.clone()
            },
        )
        .map_err(ScopeQueryError::Search)?;
        total = total.saturating_add(local.total);
        remaining_skip = remaining_skip.saturating_sub(local.total);
        if remaining_take == 0 || local.matches.is_empty() {
            continue;
        }
        let mut local = local;
        let mut hits = std::mem::take(&mut local.matches);
        if u32::try_from(hits.len()).unwrap_or(u32::MAX) > remaining_take {
            hits.truncate(usize::try_from(remaining_take).unwrap_or(usize::MAX));
        }
        for hit in &mut hits {
            hit.ordinal = document_ordinal_base.saturating_add(hit.ordinal);
        }
        remaining_take =
            remaining_take.saturating_sub(u32::try_from(hits.len()).unwrap_or(u32::MAX));
        groups.push(ScopedSearchDocument {
            address: scoped.address.clone(),
            depth: scoped.depth,
            render: local.render,
            matches: hits,
        });
    }
    let returned = query.limit.saturating_sub(remaining_take);
    let end = query.offset.saturating_add(returned);
    Ok(ScopeQueryResult::Search {
        search: ScopeSearch {
            query: query.clone(),
            total,
            returned,
            offset: query.offset,
            truncated: end < total,
            next_offset: (end < total).then_some(end),
            documents: groups,
        },
    })
}

#[derive(Clone)]
struct DocumentReference {
    target: SemanticDocumentReference,
    kind: DocumentEdgeKind,
    source_offset: Option<u32>,
    sequence: usize,
}

impl DocumentReference {
    fn exact_address(&self, from: &DocumentAddress) -> Option<DocumentAddress> {
        self.target.resolve_from(from)
    }

    fn selector(&self, from: &DocumentAddress) -> Option<DocumentSelector> {
        match &self.target {
            SemanticDocumentReference::Document { name, .. } => {
                let address = from.resolve_document_reference(name)?;
                Some(DocumentSelector {
                    selector: address.catalog_path(),
                    source: None,
                    manual_section: None,
                })
            }
            SemanticDocumentReference::Manual {
                name,
                manual_section,
            } => Some(DocumentSelector {
                selector: name.clone(),
                source: None,
                manual_section: manual_section.clone(),
            }),
        }
    }

    fn fallback_selector(&self) -> DocumentSelector {
        let selector = match &self.target {
            SemanticDocumentReference::Document { name, .. }
            | SemanticDocumentReference::Manual { name, .. } => name.clone(),
        };
        DocumentSelector {
            selector,
            source: None,
            manual_section: None,
        }
    }
}

fn document_references(bundle: &ResolvedContent) -> Vec<DocumentReference> {
    struct Collector {
        references: Vec<DocumentReference>,
        source_offset: Option<u32>,
        sequence: usize,
    }
    impl Collector {
        fn push(
            &mut self,
            target: SemanticDocumentReference,
            kind: DocumentEdgeKind,
            source_offset: Option<u32>,
        ) {
            self.references.push(DocumentReference {
                target,
                kind,
                source_offset,
                sequence: self.sequence,
            });
            self.sequence += 1;
        }
    }
    impl<'ir> Visit<'ir> for Collector {
        fn visit_block(&mut self, block: &'ir Block) {
            let previous = self.source_offset;
            self.source_offset = crate::block::block_source(block)
                .and_then(|source| source.byte_range)
                .map(|range| range.start.get());
            walk_block(self, block);
            self.source_offset = previous;
        }

        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link { target, .. } = inline
                && let Some(target) = SemanticDocumentReference::from_link_target(target)
            {
                let kind = reference_edge_kind(&target);
                self.push(target, kind, self.source_offset);
            }
            walk_inline(self, inline);
        }

        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            let previous = self.source_offset;
            self.source_offset = item
                .description
                .first()
                .and_then(crate::block::block_source)
                .and_then(|source| source.byte_range)
                .map(|range| range.start.get())
                .or(previous);
            walk_definition_item(self, item);
            if let Some(ValueDomain::EntrySet {
                reference, source, ..
            }) = item
                .identity
                .as_ref()
                .and_then(|identity| identity.value_domain.as_ref())
            {
                self.push(
                    reference.clone(),
                    reference_edge_kind(reference),
                    source
                        .and_then(|source| source.byte_range)
                        .map(|range| range.start.get()),
                );
            }
            self.source_offset = previous;
        }
    }
    let mut collector = Collector {
        references: Vec::new(),
        source_offset: None,
        sequence: 0,
    };
    if let Some(document) = bundle.document.as_ref() {
        collector.visit_document(document);
    }
    collector.references.sort_by_key(|reference| {
        (
            reference.source_offset.unwrap_or(u32::MAX),
            reference.sequence,
        )
    });
    collector.references
}

const fn reference_edge_kind(reference: &SemanticDocumentReference) -> DocumentEdgeKind {
    match reference {
        SemanticDocumentReference::Document { .. } => DocumentEdgeKind::Document,
        SemanticDocumentReference::Manual { .. } => DocumentEdgeKind::Manual,
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::{DocumentAddress, MarkdownOrigin};

    use super::*;

    #[test]
    fn entry_domains_participate_in_typed_document_traversal() {
        let parsed = crate::parse_markdown(
            "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->\n",
            None,
        )
        .expect("semantic domain document");
        let references = document_references(&ResolvedContent {
            label: "ssh".to_owned(),
            address: Some(DocumentAddress::Manual {
                name: "ssh".to_owned(),
                manual_section: "1".to_owned(),
            }),
            document: Some(parsed.document),
            tldr: None,
        });

        assert!(matches!(
            references.as_slice(),
            [DocumentReference {
                target: SemanticDocumentReference::Manual {
                    name,
                    manual_section: Some(section),
                },
                kind: DocumentEdgeKind::Manual,
                ..
            }] if name == "ssh_config" && section == "5"
        ));
    }

    #[test]
    fn ambiguous_entry_domains_do_not_participate_in_traversal() {
        let parsed = crate::parse_markdown(
            "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=first.md roles=configuration-key -->\n  <!-- mant:domain entries=second.md roles=configuration-key -->\n",
            None,
        )
        .expect("ambiguous semantic domain document");
        let references = document_references(&ResolvedContent {
            label: "ssh".to_owned(),
            address: Some(DocumentAddress::Markdown {
                path: "ssh".to_owned(),
                origin: MarkdownOrigin::Documents,
            }),
            document: Some(parsed.document),
            tldr: None,
        });

        assert!(references.is_empty());
    }

    #[test]
    fn semantic_relationships_follow_authored_source_order() {
        let parsed = crate::parse_markdown(
            "# Tools\n\n<!-- mant:entries role=command case=sensitive -->\n- [`target`](target.md): See [description](description.md).\n\n  <!-- mant:domain entries=domain.md roles=command -->\n",
            None,
        )
        .expect("semantic relationships");
        let references = document_references(&ResolvedContent {
            label: "tools".to_owned(),
            address: Some(DocumentAddress::Markdown {
                path: "indexes/tools".to_owned(),
                origin: mant_ir::MarkdownOrigin::Documents,
            }),
            document: Some(parsed.document),
            tldr: None,
        });

        assert_eq!(
            references
                .iter()
                .map(|reference| match &reference.target {
                    SemanticDocumentReference::Document { name, .. }
                    | SemanticDocumentReference::Manual { name, .. } => name.as_str(),
                })
                .collect::<Vec<_>>(),
            ["target", "description", "domain"]
        );
    }

    #[test]
    fn scope_bounds_include_every_root() {
        let scope = DocumentScope {
            documents: vec![
                DocumentSelector {
                    selector: "a".to_owned(),
                    source: None,
                    manual_section: None,
                },
                DocumentSelector {
                    selector: "b".to_owned(),
                    source: None,
                    manual_section: None,
                },
            ],
            traversal: mant_protocol::DocumentTraversal {
                follow_links: true,
                max_documents: Some(1),
                ..mant_protocol::DocumentTraversal::default()
            },
        };
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::DocumentLimit)
        );
    }

    #[test]
    fn explicit_traversal_limits_require_link_following() {
        let scope = DocumentScope {
            documents: vec![DocumentSelector {
                selector: "a".to_owned(),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal {
                follow_links: false,
                max_depth: Some(0),
                max_documents: None,
            },
        };
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::TraversalLimitsRequireLinks)
        );
    }

    #[test]
    fn native_scope_request_enforces_document_selector_contract() {
        let mut scope = DocumentScope {
            documents: vec![DocumentSelector {
                selector: "a".repeat(MAX_DOCUMENT_SELECTOR_CHARS + 1),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal::default(),
        };
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::DocumentSelector(ScopeTextError::TooLong {
                maximum: MAX_DOCUMENT_SELECTOR_CHARS,
            }))
        );

        scope.documents[0].selector = "界".repeat(MAX_DOCUMENT_SELECTOR_CHARS);
        assert_eq!(validate_document_scope(&scope), Ok(()));

        scope.documents[0].selector = "root\nother".to_owned();
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::DocumentSelector(
                ScopeTextError::ControlCharacter
            ))
        );
    }

    #[test]
    fn native_scope_request_enforces_entry_selector_contract() {
        let mut request = ScopeQueryRequest {
            schema: mant_protocol::ScopeRequestSchema::V0Dot11,
            scope: DocumentScope {
                documents: vec![DocumentSelector {
                    selector: "root".to_owned(),
                    source: None,
                    manual_section: None,
                }],
                traversal: mant_protocol::DocumentTraversal::default(),
            },
            view: ScopeQueryView::Explain {
                entry: "x".repeat(MAX_SEMANTIC_ENTRY_CHARS + 1),
            },
        };
        assert_eq!(
            validate_scope_query_request(&request),
            Err(ScopeQueryError::EntrySelector(ScopeTextError::TooLong {
                maximum: MAX_SEMANTIC_ENTRY_CHARS,
            }))
        );

        request.view = ScopeQueryView::Explain {
            entry: "界".repeat(MAX_SEMANTIC_ENTRY_CHARS),
        };
        assert_eq!(validate_scope_query_request(&request), Ok(()));
    }

    #[test]
    fn scope_explain_retains_a_visible_text_probe_as_a_qualified_failure() {
        let address = DocumentAddress::Markdown {
            path: "shell".to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        let loaded = LoadedDocumentScope {
            scope: ResolvedDocumentScope {
                query: DocumentScope {
                    documents: vec![DocumentSelector {
                        selector: "documents/shell".to_owned(),
                        source: None,
                        manual_section: None,
                    }],
                    traversal: mant_protocol::DocumentTraversal::default(),
                },
                documents: vec![ScopedDocument {
                    address: address.clone(),
                    depth: 0,
                    root_indices: vec![0],
                    reached_from: Vec::new(),
                }],
                edges: Vec::new(),
                frontier: Vec::new(),
                unresolved: Vec::new(),
            },
            documents: vec![
                crate::query_markdown_text(
                    "# Shell\n\n## Startup\n\nThe `VISUAL` name selects an editor.\n",
                    Some("shell.md".to_owned()),
                )
                .expect("probe fixture"),
            ],
        };

        let ScopeQueryResult::Explain {
            matches,
            missed,
            failures,
            ..
        } = execute_scope_explain(&loaded, "VISUAL")
        else {
            panic!("explain result");
        };
        assert!(matches.is_empty());
        assert_eq!(missed, 0);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].address, address);
        assert!(failures[0].reason.contains("outline node 1 (Startup)"));
        assert!(failures[0].reason.contains("at line"));
    }

    #[test]
    fn scope_search_uses_one_global_cursor_and_global_ordinals() {
        let address = |path: &str| DocumentAddress::Markdown {
            path: path.to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        let markdown = |title: &str, count: usize| {
            let body = (1..=count)
                .map(|index| format!("needle {index}"))
                .collect::<Vec<_>>()
                .join("\n\n");
            crate::query_markdown_text(&format!("# {title}\n\n{body}\n"), None)
                .expect("search fixture")
        };
        let documents = ["alpha", "beta"]
            .into_iter()
            .map(|path| ScopedDocument {
                address: address(path),
                depth: 0,
                root_indices: Vec::new(),
                reached_from: Vec::new(),
            })
            .collect::<Vec<_>>();
        let loaded = LoadedDocumentScope {
            scope: ResolvedDocumentScope {
                query: DocumentScope {
                    documents: Vec::new(),
                    traversal: mant_protocol::DocumentTraversal::default(),
                },
                documents,
                edges: Vec::new(),
                frontier: Vec::new(),
                unresolved: Vec::new(),
            },
            documents: vec![markdown("Alpha", 3), markdown("Beta", 10)],
        };
        let query = SearchQuery {
            pattern: "needle".to_owned(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Insensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 5,
            offset: 0,
        };

        let ScopeQueryResult::Search { search } =
            execute_scope_search(&loaded, &query).expect("scope search")
        else {
            panic!("search result");
        };

        assert_eq!(search.total, 13);
        assert_eq!(search.returned, 5);
        assert_eq!(search.next_offset, Some(5));
        assert_eq!(search.documents.len(), 2);
        assert_eq!(
            search
                .documents
                .iter()
                .flat_map(|document| document.matches.iter().map(|hit| hit.ordinal))
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );

        let cross_boundary_query = SearchQuery { offset: 2, ..query };
        let ScopeQueryResult::Search { search } =
            execute_scope_search(&loaded, &cross_boundary_query).expect("scope search")
        else {
            panic!("search result");
        };

        assert_eq!(search.total, 13);
        assert_eq!(search.returned, 5);
        assert_eq!(search.next_offset, Some(7));
        assert_eq!(search.documents.len(), 2);
        assert_eq!(
            search
                .documents
                .iter()
                .flat_map(|document| document.matches.iter().map(|hit| hit.ordinal))
                .collect::<Vec<_>>(),
            [3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn relative_links_use_the_current_markdown_namespace() {
        let reference = DocumentReference {
            target: SemanticDocumentReference::Document {
                name: "../other".to_owned(),
                fragment: None,
            },
            kind: DocumentEdgeKind::Document,
            source_offset: None,
            sequence: 0,
        };
        let from = DocumentAddress::Markdown {
            path: "guide/start".to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        assert_eq!(
            reference.selector(&from).map(|selector| selector.selector),
            Some("documents/other".to_owned())
        );
    }

    #[test]
    fn frontier_retains_unresolved_manual_targets_without_inventing_an_address() {
        let scope = DocumentScope {
            documents: vec![DocumentSelector {
                selector: "root".to_owned(),
                source: None,
                manual_section: Some("1".to_owned()),
            }],
            traversal: mant_protocol::DocumentTraversal {
                follow_links: true,
                max_depth: None,
                max_documents: Some(1),
            },
        };
        let from = DocumentAddress::Manual {
            name: "root".to_owned(),
            manual_section: "1".to_owned(),
        };
        let reference = DocumentReference {
            target: SemanticDocumentReference::Manual {
                name: "child".to_owned(),
                manual_section: None,
            },
            kind: DocumentEdgeKind::Manual,
            source_offset: None,
            sequence: 0,
        };
        let mut resolution = ScopeResolution::new(&scope);
        resolution.record_frontier(&from, &reference, TraversalLimit::MaxDocuments);

        assert_eq!(resolution.graph.frontier.len(), 1);
        assert_eq!(resolution.graph.frontier[0].target.selector, "child");
        assert_eq!(resolution.graph.frontier[0].target.manual_section, None);
        assert_eq!(
            resolution.graph.frontier[0].limit,
            TraversalLimit::MaxDocuments
        );
    }

    #[test]
    fn normalized_content_budget_refuses_another_document_before_retaining_it() {
        let scope = DocumentScope {
            documents: vec![DocumentSelector {
                selector: "root".to_owned(),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal::default(),
        };
        let content =
            crate::query_markdown_text("# Child\n\nBody.\n", None).expect("fixture content");
        let bytes = normalized_content_bytes(&content);
        assert!(bytes > 0 && bytes <= MAX_SCOPE_CONTENT_BYTES);

        let mut resolution = ScopeResolution::new(&scope);
        resolution.content_bytes = MAX_SCOPE_CONTENT_BYTES - bytes + 1;

        assert!(!resolution.reserve_content_bytes(&content));
        assert_eq!(
            resolution.content_bytes,
            MAX_SCOPE_CONTENT_BYTES - bytes + 1
        );
    }

    #[test]
    fn root_content_budget_is_reported_as_an_unresolved_root() {
        let selector = DocumentSelector {
            selector: "root".to_owned(),
            source: None,
            manual_section: None,
        };
        let scope = DocumentScope {
            documents: vec![selector.clone()],
            traversal: mant_protocol::DocumentTraversal::default(),
        };
        let mut content =
            crate::query_markdown_text("# Root\n\nBody.\n", None).expect("fixture content");
        content.address = Some(DocumentAddress::Markdown {
            path: "root".to_owned(),
            origin: MarkdownOrigin::Documents,
        });
        let mut resolution = ScopeResolution::new(&scope);
        resolution.content_bytes = MAX_SCOPE_CONTENT_BYTES;

        resolution.insert_root(content, &selector, 0);

        assert!(resolution.documents.is_empty());
        assert_eq!(resolution.graph.unresolved.len(), 1);
        assert!(
            resolution.graph.unresolved[0]
                .reason
                .contains("aggregate scope content budget")
        );
    }
}
