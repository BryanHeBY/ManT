//! Scope resolve: preserve request-local ownership and source order.
#[cfg(test)]
mod tests;
use super::references::{ScopeReference, document_references};
use super::{
    BTreeMap, BTreeSet, DocumentAddress, DocumentEdge, DocumentEdgeKind, DocumentFrontier,
    DocumentResolver, DocumentScope, DocumentSelector, LoadError, LoadSpec, LoadedDocumentScope,
    MAX_SCOPE_CONTENT_BYTES, QueryPolicy, ResolvedContent, ResolvedDocumentScope, ScopeQueryError,
    ScopedDocument, TraversalLimit, UnresolvedDocument, VecDeque, Write, validate_document_scope,
};

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

    fn resolve_selector(
        &self,
        selector: &DocumentSelector,
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, LoadError> {
        self.load(
            LoadSpec::Document {
                selector: &selector.selector,
                source: selector.source.as_deref(),
                manual_section: selector.manual_section.as_deref(),
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
    failures: ResolutionFailures,
    unresolved_keys: BTreeSet<UnresolvedKey>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct ResolutionKey {
    policy: u8,
    selector: String,
    source: Option<String>,
    manual_section: Option<String>,
}
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct UnresolvedKey {
    from: Option<DocumentAddress>,
    selector: String,
    source: Option<String>,
    manual_section: Option<String>,
    reason: String,
}

/// Request-local negative cache. Keys include policy and the fully qualified
/// selector, never a bare link label. Nothing survives into the next request.
#[derive(Default)]
struct ResolutionFailures(BTreeMap<ResolutionKey, String>);

impl ResolutionFailures {
    fn resolve<T>(
        &mut self,
        selector: &DocumentSelector,
        policy: QueryPolicy,
        load: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let key = ResolutionKey {
            policy: match policy {
                QueryPolicy::Combined => 0,
                QueryPolicy::ManualOnly => 1,
                QueryPolicy::TldrOnly => 2,
            },
            selector: selector.selector.clone(),
            source: selector.source.clone(),
            manual_section: selector.manual_section.clone(),
        };
        if let Some(reason) = self.0.get(&key) {
            return Err(reason.clone());
        }
        let result = load();
        if let Err(reason) = &result {
            self.0.insert(key, reason.clone());
        }
        result
    }
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
                reference_limits: Vec::new(),
            },
            documents: Vec::new(),
            positions: BTreeMap::new(),
            queue: VecDeque::new(),
            content_bytes: 0,
            failures: ResolutionFailures::default(),
            unresolved_keys: BTreeSet::new(),
        }
    }

    fn resolve_roots(&mut self, resolver: &DocumentResolver) {
        for (root_index, selector) in self.graph.query.documents.clone().iter().enumerate() {
            match self.failures.resolve(selector, QueryPolicy::Combined, || {
                resolver
                    .resolve_selector(selector, QueryPolicy::Combined)
                    .map_err(|error| error.to_string())
            }) {
                Ok(bundle) => {
                    self.insert_root(bundle, selector, root_index);
                }
                Err(error) => self.record_unresolved(UnresolvedDocument {
                    from: None,
                    selector: selector.clone(),
                    reason: error,
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
            self.record_unresolved(UnresolvedDocument {
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
        if !self.commit_document(
            bundle,
            ScopedDocument {
                address,
                depth: 0,
                root_indices: vec![root_index],
                reached_from: Vec::new(),
            },
            None,
        ) {
            self.record_unresolved(UnresolvedDocument {
                from: None,
                selector: selector.clone(),
                reason: format!(
                    "document exceeds the {} MiB aggregate scope content budget",
                    MAX_SCOPE_CONTENT_BYTES / (1024 * 1024)
                ),
            });
        }
    }

    fn follow_links(&mut self, resolver: &DocumentResolver) {
        while let Some(position) = self.queue.pop_front() {
            let depth = self.graph.documents[position].depth;
            if depth >= self.graph.query.traversal.effective_max_depth() {
                self.record_depth_frontier(position);
                continue;
            }
            let from = self.graph.documents[position].address.clone();
            for reference in self.collect_outbound_references(position) {
                self.follow_reference(resolver, &from, depth, &reference);
            }
        }
    }

    fn record_depth_frontier(&mut self, position: usize) {
        let from = self.graph.documents[position].address.clone();
        for reference in self.collect_outbound_references(position) {
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

    fn collect_outbound_references(&mut self, position: usize) -> Vec<ScopeReference> {
        let collected = document_references(&self.documents[position]);
        if !collected.report.complete() {
            self.graph
                .reference_limits
                .push(mant_protocol::ScopeReferenceLimit {
                    document: self.graph.documents[position].address.clone(),
                    coverage: mant_protocol::ReferenceCoverage::from_report(collected.report),
                    retention_limit: collected.retention_limit,
                });
        }
        collected.references
    }

    fn follow_reference(
        &mut self,
        resolver: &DocumentResolver,
        from: &DocumentAddress,
        depth: u16,
        reference: &ScopeReference,
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
            self.record_unresolved(UnresolvedDocument {
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
        let bundle = match self.failures.resolve(&selector, policy, || {
            resolver
                .resolve_selector(&selector, policy)
                .map_err(|error| error.to_string())
        }) {
            Ok(bundle) => bundle,
            Err(error) => {
                self.record_unresolved(UnresolvedDocument {
                    from: Some(from.clone()),
                    selector,
                    reason: error,
                });
                return;
            }
        };
        let Some(address) = bundle.address.clone() else {
            self.record_unresolved(UnresolvedDocument {
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

    fn record_unresolved(&mut self, failure: UnresolvedDocument) {
        let key = UnresolvedKey {
            from: failure.from.clone(),
            selector: failure.selector.selector.clone(),
            source: failure.selector.source.clone(),
            manual_section: failure.selector.manual_section.clone(),
            reason: failure.reason.clone(),
        };
        if self.unresolved_keys.insert(key) {
            self.graph.unresolved.push(failure);
        }
    }

    fn insert_linked(
        &mut self,
        bundle: ResolvedContent,
        address: DocumentAddress,
        from: &DocumentAddress,
        depth: u16,
        edge: DocumentEdge,
    ) -> bool {
        self.commit_document(
            bundle,
            ScopedDocument {
                address,
                depth,
                root_indices: Vec::new(),
                reached_from: vec![from.clone()],
            },
            Some(edge),
        )
    }

    /// The only admission point for a new snapshot. The BFS driver has already
    /// checked identity deduplication and the document limit. Compute the byte
    /// budget before changing any retained state; rejection leaves the graph,
    /// paired content, address ledger, queue and accounting unchanged.
    /// This is atomic with respect to controlled rejection, not allocation panic.
    fn commit_document(
        &mut self,
        bundle: ResolvedContent,
        source: ScopedDocument,
        edge: Option<DocumentEdge>,
    ) -> bool {
        debug_assert_eq!(bundle.address.as_ref(), Some(&source.address));
        debug_assert!(!self.positions.contains_key(&source.address));
        let bytes = normalized_content_bytes(&bundle);
        let Some(total) = self.content_bytes.checked_add(bytes) else {
            return false;
        };
        if total > MAX_SCOPE_CONTENT_BYTES {
            return false;
        }
        let position = self.documents.len();
        self.positions.insert(source.address.clone(), position);
        self.documents.push(bundle);
        self.graph.documents.push(source);
        self.queue.push_back(position);
        if let Some(edge) = edge
            && !self.graph.edges.contains(&edge)
        {
            self.graph.edges.push(edge);
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
        reference: &ScopeReference,
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
