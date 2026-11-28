//! Explicit local-environment snapshot for view-independent acquisition.
use super::{
    CatalogQuery, Document, DocumentAddress, DocumentCatalog, LoadError, LoadHost, LoadPolicy,
    LoadSpec, MAX_MARKDOWN_BYTES, ManualIndex, ManualPage, ManualRequest, MarkdownOrigin, OnceLock,
    Path, PathBuf, RegisteredDocumentIndex, RegisteredDocumentOrigin, RegisteredLookupPhase,
    RegisteredSelection, RegisteredSelectionGroup, SourceConfigError, TldrDocument,
    discover_manual_roots, fs, load_with, locate_manual_source_in, query_name_candidates,
    read_cached_tldr_page, read_capped_utf8,
};
#[cfg(feature = "roff")]
use super::{parse_manual_page, parse_manual_source};
use mant_ir::ResolvedContent;

fn registered_selection(document: &mant_sources::RegisteredDocument) -> RegisteredSelection {
    RegisteredSelection {
        path: document.path.clone(),
        address: DocumentAddress::Markdown {
            path: document.logical_path.clone(),
            origin: match &document.origin {
                RegisteredDocumentOrigin::Documents => MarkdownOrigin::Documents,
                RegisteredDocumentOrigin::Source(name) => {
                    MarkdownOrigin::Source { name: name.clone() }
                }
            },
        },
    }
}

/// One explicit local document-environment snapshot.
pub struct DocumentLoader {
    registered: OnceLock<Result<RegisteredDocumentIndex, SourceConfigError>>,
    manual_roots: Vec<PathBuf>,
    manuals: OnceLock<ManualIndex>,
    available: OnceLock<Vec<crate::catalog::AvailableDocument>>,
}

impl DocumentLoader {
    /// Capture native manual roots and lazily snapshot the manual index and
    /// Markdown registration.
    #[must_use]
    pub fn from_system() -> Self {
        Self {
            registered: OnceLock::new(),
            manual_roots: discover_manual_roots(),
            manuals: OnceLock::new(),
            available: OnceLock::new(),
        }
    }

    /// Load one borrowed source specification against this environment snapshot.
    /// Reusing a loader preserves manual and registered-document precedence.
    /// Construct a new loader to refresh discovery.
    ///
    /// # Errors
    /// Returns invalid source-selection input or unreadable local content.
    pub fn load(
        &self,
        spec: LoadSpec<'_>,
        policy: LoadPolicy,
    ) -> Result<ResolvedContent, LoadError> {
        load_with(spec, policy, self)
    }

    /// Load once and pair the immutable result with source-native link membership.
    ///
    /// # Errors
    /// Returns the same selection, access, or parse errors as ordinary loading.
    pub fn load_with_link_layout(
        &self,
        spec: LoadSpec<'_>,
        policy: LoadPolicy,
    ) -> Result<super::ResolvedWithLinkLayout, LoadError> {
        let mut layout = mant_ir::LinkLayout::default();
        let content = super::input::load_impl(spec, policy, self, Some(&mut layout))?;
        Ok(super::paired::pair(content, layout))
    }

    /// Filter the same registered-document and manual snapshots used by
    /// [`Self::load`].
    ///
    /// # Errors
    ///
    /// Returns source-configuration or catalog-query failures as one host
    /// boundary diagnostic.
    pub fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, String> {
        let plan =
            crate::catalog::PreparedCatalogQuery::new(query).map_err(|error| error.to_string())?;
        self.discover_prepared(&plan)
    }

    /// Discover using an already validated and compiled catalog query.
    ///
    /// This reuses both the supplied matcher and this loader's environment
    /// snapshot. No source lookup occurs while the query is being prepared.
    ///
    /// # Errors
    /// Returns source configuration or catalog acquisition failures.
    pub fn discover_prepared(
        &self,
        plan: &crate::PreparedCatalogQuery<'_>,
    ) -> Result<DocumentCatalog, String> {
        let registered = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let manuals = self
            .manuals
            .get_or_init(|| ManualIndex::from_roots(self.manual_roots.clone()));
        let documents = self.available.get_or_init(|| {
            crate::catalog::list_available_documents_from(
                registered.documents().to_vec(),
                manuals.pages(),
            )
        });
        Ok(plan.apply(documents))
    }
}

impl LoadHost for DocumentLoader {
    fn native_available(&self) -> bool {
        cfg!(feature = "roff")
    }
    fn name_candidates(&self, name: &str) -> Vec<String> {
        query_name_candidates(name)
    }

    fn locate_registered_document(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Option<RegisteredSelection>, String> {
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let selected = if source.is_some() {
            index.find(candidates, source)
        } else {
            match phase {
                RegisteredLookupPhase::BeforeBuiltin => index.find_before_builtin(candidates),
                RegisteredLookupPhase::AfterBuiltin => index.find_after_builtin(candidates),
            }
        };
        selected
            .map(|registered| registered.map(registered_selection))
            .map_err(|error| error.to_string())
    }

    fn locate_registered_document_groups(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Vec<RegisteredSelectionGroup>, String> {
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let groups = if let Some(source) = source {
            index.matches_in_source(candidates, source)
        } else {
            Ok(match phase {
                RegisteredLookupPhase::BeforeBuiltin => index.matches_before_builtin(candidates),
                RegisteredLookupPhase::AfterBuiltin => index.matches_after_builtin(candidates),
            })
        }
        .map_err(|error| error.to_string())?;
        Ok(groups
            .into_iter()
            .map(|group| RegisteredSelectionGroup {
                documents: group.documents.iter().map(registered_selection).collect(),
            })
            .collect())
    }

    fn locate_registered_address(
        &self,
        address: &DocumentAddress,
    ) -> Result<Option<RegisteredSelection>, String> {
        let DocumentAddress::Markdown { path, origin } = address else {
            return Ok(None);
        };
        let origin = match origin {
            MarkdownOrigin::Documents => RegisteredDocumentOrigin::Documents,
            MarkdownOrigin::Source { name } => RegisteredDocumentOrigin::Source(name.clone()),
        };
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        index
            .find_address(path, &origin)
            .map(|document| {
                document.map(|document| RegisteredSelection {
                    path: document.path.clone(),
                    address: address.clone(),
                })
            })
            .map_err(|error| error.to_string())
    }

    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String> {
        let manuals = self
            .manuals
            .get_or_init(|| ManualIndex::from_roots(self.manual_roots.clone()));
        locate_manual_source_in(request, manuals).map_err(|error| error.load_detail())
    }

    fn parse_manual(&self, page: &ManualPage) -> Result<Document, String> {
        #[cfg(feature = "roff")]
        {
            parse_manual_page(page).map_err(|error| error.to_string())
        }
        #[cfg(not(feature = "roff"))]
        {
            let _ = page;
            Err(LoadError::NativeBackendUnavailable { tldr_topic: None }.to_string())
        }
    }

    fn parse_manual_input(&self, path: &Path) -> Result<Document, String> {
        #[cfg(feature = "roff")]
        {
            parse_manual_source(path).map_err(|error| error.to_string())
        }
        #[cfg(not(feature = "roff"))]
        {
            let _ = path;
            Err(LoadError::NativeBackendUnavailable { tldr_topic: None }.to_string())
        }
    }

    #[cfg(feature = "roff")]
    fn parse_manual_with_layout(
        &self,
        page: &ManualPage,
    ) -> Result<(Document, mant_ir::LinkLayout), String> {
        crate::manual_input::parse_manual_page_with_link_layout(page)
            .map_err(|error| error.to_string())
    }

    #[cfg(feature = "roff")]
    fn parse_manual_input_with_layout(
        &self,
        path: &Path,
    ) -> Result<(Document, mant_ir::LinkLayout), String> {
        crate::manual_input::parse_manual_source_with_link_layout(path)
            .map_err(|error| error.to_string())
    }

    fn read_tldr(&self, name: &str) -> Result<Option<TldrDocument>, String> {
        read_cached_tldr_page(name).map_err(|error| error.to_string())
    }

    fn read_markdown(&self, path: &Path) -> Result<String, String> {
        let file = fs::File::open(path).map_err(|error| error.to_string())?;
        read_capped_utf8(file, MAX_MARKDOWN_BYTES)
    }
}

#[cfg(all(test, not(feature = "roff")))]
mod no_roff_tests {
    use super::*;

    #[test]
    fn markdown_only_loader_never_opens_selected_native_inputs() {
        let loader = DocumentLoader {
            registered: OnceLock::new(),
            manual_roots: Vec::new(),
            manuals: OnceLock::new(),
            available: OnceLock::new(),
        };
        for spec in [
            LoadSpec::File {
                path: "does-not-exist.1",
                format: mant_protocol::InputFormat::Auto,
            },
            LoadSpec::Document {
                selector: "manual/1/does-not-exist",
                source: None,
                manual_section: None,
            },
        ] {
            assert_eq!(
                loader.load(spec, LoadPolicy::Combined),
                Err(LoadError::NativeBackendUnavailable { tldr_topic: None })
            );
        }
        assert!(loader.manuals.get().is_none());
        assert!(loader.registered.get().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "roff")]
    #[test]
    fn same_visible_link_does_not_transfer_membership_between_snapshots() {
        use mant_protocol::InputFormat;
        const HEADER: &str = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd small roff inputs\n.Sh DESCRIPTION\n";
        let split = ".No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n";
        let clean = ".No HEAD\n.Lk https://z.test ZYU\nEND\n";
        let root = std::env::temp_dir().join(format!("mant-layout-{}", std::process::id()));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).unwrap();
        let file = man1.join("glyphs.1");
        fs::write(&file, format!("{HEADER}{split}")).unwrap();
        let loader = DocumentLoader {
            registered: OnceLock::new(),
            manual_roots: vec![root.clone()],
            manuals: OnceLock::new(),
            available: OnceLock::new(),
        };
        let direct = LoadSpec::File {
            path: file.to_str().unwrap(),
            format: InputFormat::Auto,
        };
        let named = LoadSpec::Document {
            selector: "manual/1/glyphs",
            source: None,
            manual_section: None,
        };
        let direct_pair = loader
            .load_with_link_layout(direct, LoadPolicy::Combined)
            .unwrap();
        let indexed_pair = loader
            .load_with_link_layout(named, LoadPolicy::ManualOnly)
            .unwrap();
        assert_eq!(direct_pair.receipts().len(), 1);
        assert_eq!(indexed_pair.receipts(), direct_pair.receipts());
        assert_eq!(direct_pair.receipts()[0].members, [0..1, 2..3]);
        assert!(std::ptr::eq(
            direct_pair.content(),
            direct_pair.content_arc().as_ref()
        ));

        fs::write(&file, format!("{HEADER}{clean}")).unwrap();
        let replaced = loader
            .load_with_link_layout(direct, LoadPolicy::Combined)
            .unwrap();
        assert!(replaced.receipts().is_empty(), "{replaced:?}");
        let exact = direct_pair.content_arc();
        assert_eq!(
            direct_pair.receipts_for(&exact),
            Some(direct_pair.receipts())
        );
        let equal_value = std::sync::Arc::new(direct_pair.content().clone());
        assert_eq!(exact, equal_value);
        assert!(direct_pair.receipts_for(&equal_value).is_none());
        assert!(direct_pair.receipts_for(&replaced.content_arc()).is_none());
        assert!(!std::ptr::eq(replaced.content(), direct_pair.content()));
        assert_eq!(direct_pair.receipts()[0].members, [0..1, 2..3]);
        fs::remove_file(&file).unwrap();
        fs::remove_dir(&man1).unwrap();
        fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn invalid_discovery_preserves_uninitialized_snapshot_indexes() {
        let loader = DocumentLoader {
            registered: OnceLock::new(),
            manual_roots: Vec::new(),
            manuals: OnceLock::new(),
            available: OnceLock::new(),
        };
        for query in [
            CatalogQuery {
                limit: 0,
                ..CatalogQuery::default()
            },
            CatalogQuery {
                pattern: Some("[".into()),
                syntax: mant_protocol::SearchSyntax::Regex,
                ..CatalogQuery::default()
            },
        ] {
            assert!(loader.discover(&query).is_err());
            assert!(loader.registered.get().is_none());
            assert!(loader.manuals.get().is_none());
            assert!(loader.available.get().is_none());
        }
    }
}
