use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use mant_ir::{
    Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, Section, SourceFormat,
    TldrDocument, TldrOrigin,
};
use mant_protocol::{
    DocumentAddress, InputFormat, MAX_SEMANTIC_ENTRY_BYTES, MarkdownOrigin, QueryInput,
    QueryRequest, QueryView, RequestSchema, ScopeTextError,
};
use mant_sources::BUILTIN_CONTENT_PRIORITY;

use crate::{ManualPage, ManualRequest};

use super::{
    MAX_MARKDOWN_BYTES, QueryError, QueryExecutionError, QueryHost, QueryPolicy,
    RegisteredLookupPhase, RegisteredSelection, RegisteredSelectionGroup, project_query_view,
    query_markdown_text, query_with, read_capped_utf8, read_capped_utf8_io, validate_query_request,
};

use crate::ProjectionError;

#[derive(Clone)]
struct StubHost {
    name_candidates: Option<Vec<String>>,
    registered_document: Option<PathBuf>,
    registered_name: Option<String>,
    registered_source_priority: Option<i32>,
    locate: Result<ManualPage, String>,
    manual_name: Option<String>,
    direct: Result<Document, String>,
    tldr: Result<Option<TldrDocument>, String>,
    markdown: Result<String, String>,
    calls: std::sync::Arc<Mutex<Vec<&'static str>>>,
}

impl QueryHost for StubHost {
    fn name_candidates(&self, name: &str) -> Vec<String> {
        self.name_candidates
            .clone()
            .unwrap_or_else(|| vec![name.to_owned()])
    }

    fn locate_registered_document(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Option<RegisteredSelection>, String> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(if source.is_some() {
                "source"
            } else {
                match phase {
                    RegisteredLookupPhase::BeforeBuiltin => "name",
                    RegisteredLookupPhase::AfterBuiltin => "fallback",
                }
            });
        if source.is_none()
            && match phase {
                RegisteredLookupPhase::BeforeBuiltin => self
                    .registered_source_priority
                    .is_some_and(|priority| priority <= BUILTIN_CONTENT_PRIORITY),
                RegisteredLookupPhase::AfterBuiltin => self
                    .registered_source_priority
                    .is_none_or(|priority| priority > BUILTIN_CONTENT_PRIORITY),
            }
        {
            return Ok(None);
        }
        if self
            .registered_name
            .as_deref()
            .is_some_and(|registered_name| {
                !candidates
                    .iter()
                    .any(|candidate| candidate == registered_name)
            })
        {
            return Ok(None);
        }
        Ok(self
            .registered_document
            .clone()
            .map(|path| RegisteredSelection {
                path,
                address: DocumentAddress::Markdown {
                    path: self
                        .registered_name
                        .clone()
                        .unwrap_or_else(|| candidates[0].clone()),
                    origin: source.map_or_else(
                        || {
                            self.registered_source_priority.map_or(
                                MarkdownOrigin::Documents,
                                |_| MarkdownOrigin::Source {
                                    name: "team".to_owned(),
                                },
                            )
                        },
                        |name| MarkdownOrigin::Source {
                            name: name.to_owned(),
                        },
                    ),
                },
            }))
    }

    fn locate_registered_document_groups(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Vec<RegisteredSelectionGroup>, String> {
        self.locate_registered_document(candidates, source, phase)
            .map(|selection| {
                selection
                    .map(|value| {
                        vec![RegisteredSelectionGroup {
                            documents: vec![value],
                        }]
                    })
                    .unwrap_or_default()
            })
    }

    fn locate_registered_address(
        &self,
        address: &DocumentAddress,
    ) -> Result<Option<RegisteredSelection>, String> {
        self.calls.lock().expect("calls lock").push("address");
        Ok(self
            .registered_document
            .clone()
            .map(|path| RegisteredSelection {
                path,
                address: address.clone(),
            }))
    }

    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String> {
        self.calls.lock().expect("calls lock").push("locate");
        if self
            .manual_name
            .as_deref()
            .is_some_and(|manual_name| manual_name != request.name)
        {
            return Err("source not found".to_owned());
        }
        self.locate.clone()
    }

    fn parse_manual(&self, _page: &ManualPage) -> Result<Document, String> {
        self.calls.lock().expect("calls lock").push("parse");
        self.direct.clone()
    }

    fn parse_manual_input(&self, _path: &Path) -> Result<Document, String> {
        self.calls.lock().expect("calls lock").push("manual-input");
        self.direct.clone()
    }

    fn read_tldr(&self, _name: &str) -> Result<Option<TldrDocument>, String> {
        self.calls.lock().expect("calls lock").push("tldr");
        self.tldr.clone()
    }

    fn read_markdown(&self, _path: &Path) -> Result<String, String> {
        self.calls.lock().expect("calls lock").push("markdown");
        self.markdown.clone()
    }
}

fn document(format: SourceFormat, unsupported: bool, readable: bool) -> Document {
    Document {
        parser: None,
        source: DocumentSource { format, path: None },
        meta: DocumentMeta::default(),
        diagnostics: unsupported
            .then_some(Diagnostic {
                level: DiagnosticLevel::Unsupported,
                code: None,
                message: "unsupported request".to_owned(),
                source: None,
            })
            .into_iter()
            .collect(),
        blocks: Vec::new(),
        sections: readable
            .then_some(Section {
                id: "name-1".to_owned().into(),
                title: "NAME".to_owned(),
                spacing_before_lines: 0,
                blocks: Vec::new(),
                children: Vec::new(),
                source: None,
            })
            .into_iter()
            .collect(),
    }
}

fn tldr() -> TldrDocument {
    TldrDocument {
        title: "tool".to_owned(),
        description: vec!["quick reference".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/cache/pages/common/tool.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    }
}

fn embedded_tldr_markdown() -> String {
    "\
<!-- mant:tldr:start -->
# tool

> Source-owned quick reference.

- Run the tool:

`tool`
<!-- mant:tldr:end -->

# Tool

Full documentation.
"
    .to_owned()
}

fn host(direct: Result<Document, String>) -> StubHost {
    StubHost {
        name_candidates: None,
        registered_document: None,
        registered_name: None,
        registered_source_priority: None,
        locate: Ok(ManualPage {
            name: "tool".to_owned(),
            section: "1".to_owned(),
            path: PathBuf::from("/man/tool.1"),
            manual_root: PathBuf::from("/man"),
        }),
        manual_name: None,
        direct,
        tldr: Ok(None),
        markdown: Err("Markdown unavailable".to_owned()),
        calls: std::sync::Arc::default(),
    }
}

fn request() -> QueryRequest {
    QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: " tool ".to_owned(),
            source: None,
            manual_section: None,
        },
        view: QueryView::Full {},
    }
}

#[test]
fn ordinary_manual_uses_the_native_parser() {
    let host = host(Ok(document(SourceFormat::Man, false, true)));
    let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

    assert_eq!(result.label, "tool");
    assert_eq!(
        result.document.expect("manual").source.format,
        SourceFormat::Man
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "locate", "parse", "tldr"]
    );
}

#[test]
fn ordinary_command_manuals_can_attach_cached_tldr() {
    for section in ["1", "1p", "8", "8x"] {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.locate.as_mut().expect("manual page").section = section.to_owned();
        host.tldr = Ok(Some(tldr()));

        let result =
            query_with(&request(), QueryPolicy::default(), &host).expect("command manual query");

        assert_eq!(result.tldr.expect("attached tldr").title, "tool");
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "locate", "parse", "tldr"]
        );
    }
}

#[test]
fn non_command_manuals_do_not_attach_or_probe_cached_tldr() {
    let mut host = host(Ok(document(SourceFormat::Man, false, true)));
    host.locate.as_mut().expect("manual page").section = "5".to_owned();
    host.tldr = Ok(Some(tldr()));

    let result = query_with(&request(), QueryPolicy::default(), &host).expect("file format manual");

    assert!(result.tldr.is_none());
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "locate", "parse"]
    );
}

#[test]
fn requested_manual_section_backfills_metadata_the_parser_left_empty() {
    let mut host = host(Ok(document(SourceFormat::Man, false, true)));
    host.locate.as_mut().expect("manual page").section = "3".to_owned();
    host.tldr = Ok(Some(tldr()));
    let request = QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some("3".to_owned()),
        },
        view: QueryView::Full {},
    };

    let result = query_with(&request, QueryPolicy::default(), &host).expect("query");
    assert_eq!(
        result.address,
        Some(DocumentAddress::Manual {
            name: "tool".to_owned(),
            manual_section: "3".to_owned(),
        })
    );
    assert_eq!(
        result
            .document
            .as_ref()
            .expect("manual")
            .meta
            .manual_section
            .as_deref(),
        Some("3"),
        "requested section must label output when the parser omits it"
    );
    assert!(
        result.tldr.is_none(),
        "a non-command manual category cannot inherit a tldr page"
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["locate", "parse"],
        "an explicit non-command section bypasses Markdown and tldr lookup"
    );
}

#[test]
fn requested_command_section_keeps_the_combined_tldr_facet() {
    let mut host = host(Ok(document(SourceFormat::Man, false, true)));
    host.tldr = Ok(Some(tldr()));
    let request = QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some("1".to_owned()),
        },
        view: QueryView::Full {},
    };

    let result = query_with(&request, QueryPolicy::Combined, &host)
        .expect("section-qualified combined query");

    assert_eq!(result.tldr.expect("attached tldr").title, "tool");
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["locate", "parse", "tldr"]
    );
}

#[test]
fn tldr_only_accepts_command_sections_and_rejects_other_categories() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.tldr = Ok(Some(tldr()));
    let request_for = |section: &str| QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some(section.to_owned()),
        },
        view: QueryView::Full {},
    };

    let result = query_with(&request_for("1"), QueryPolicy::TldrOnly, &host)
        .expect("section 1 identifies a command topic");
    assert_eq!(result.tldr.expect("tldr").title, "tool");

    assert_eq!(
        query_with(&request_for("5"), QueryPolicy::TldrOnly, &host),
        Err(QueryError::TldrManualSection {
            section: "5".to_owned(),
        })
    );
}

#[test]
fn explicit_source_reads_only_registered_markdown() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/documents/tool.md"));
    host.markdown = Ok("# Tool\n\nSource body.\n".to_owned());
    let request = QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "tool".to_owned(),
            source: Some("team".to_owned()),
            manual_section: None,
        },
        view: QueryView::Full {},
    };
    let result = query_with(&request, QueryPolicy::default(), &host).expect("source query");
    assert_eq!(
        result.address,
        Some(DocumentAddress::Markdown {
            path: "tool".to_owned(),
            origin: MarkdownOrigin::Source {
                name: "team".to_owned(),
            },
        })
    );
    assert_eq!(
        result.document.expect("Markdown").meta.title.as_deref(),
        Some("Tool")
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["source", "markdown"]
    );
}

#[test]
fn canonical_catalog_paths_resolve_exact_addresses() {
    let mut markdown = host(Err("manual must not be read".to_owned()));
    markdown.registered_document = Some(PathBuf::from("/documents/en/tool.md"));
    markdown.markdown = Ok("# Tool\n\nBody.\n".to_owned());
    let request = QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "documents/en/tool".to_owned(),
            source: None,
            manual_section: None,
        },
        view: QueryView::Full {},
    };
    let result = query_with(&request, QueryPolicy::default(), &markdown).expect("canonical");
    assert_eq!(
        result.address,
        Some(DocumentAddress::Markdown {
            path: "en/tool".to_owned(),
            origin: MarkdownOrigin::Documents,
        })
    );
    assert_eq!(
        *markdown.calls.lock().expect("calls"),
        ["address", "markdown"]
    );

    let manual = host(Ok(document(SourceFormat::Man, false, true)));
    let request = QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "manual/1/tool".to_owned(),
            source: None,
            manual_section: None,
        },
        view: QueryView::Full {},
    };
    let result = query_with(&request, QueryPolicy::default(), &manual).expect("manual path");
    assert_eq!(
        result.address,
        Some(DocumentAddress::Manual {
            name: "tool".to_owned(),
            manual_section: "1".to_owned(),
        })
    );
    assert_eq!(
        *manual.calls.lock().expect("calls"),
        ["locate", "parse", "tldr"]
    );
}

#[test]
fn complete_direct_document_survives_an_unsupported_finding() {
    let host = host(Ok(document(SourceFormat::Man, true, true)));
    let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

    assert_eq!(
        result.document.expect("manual").source.format,
        SourceFormat::Man
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "locate", "parse", "tldr"]
    );
}

#[test]
fn manual_only_bypasses_registered_markdown() {
    let mut host = host(Ok(document(SourceFormat::Man, true, true)));
    host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
    host.markdown = Ok("# Registered".to_owned());
    host.tldr = Ok(Some(tldr()));
    let result = query_with(&request(), QueryPolicy::ManualOnly, &host).expect("manual-only query");

    assert_eq!(
        result.document.as_ref().expect("manual").source.format,
        SourceFormat::Man
    );
    assert!(result.tldr.is_none(), "manual-only must not attach tldr");
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["locate", "parse"],
        "manual-only lookup must not inspect Markdown or tldr namespaces"
    );
}

#[test]
fn manual_only_failure_is_not_hidden_by_tldr() {
    let mut host = host(Ok(document(SourceFormat::Man, true, false)));
    host.tldr = Ok(Some(tldr()));

    let error = query_with(&request(), QueryPolicy::ManualOnly, &host)
        .expect_err("an optional tldr page must not hide native parser failure");

    let QueryError::Manual(detail) = error else {
        panic!("expected the native parser diagnostic");
    };
    assert!(detail.to_string().contains("/man/tool.1"));
    assert!(
        detail
            .to_string()
            .contains("Unsupported: unsupported request")
    );
    assert_eq!(*host.calls.lock().expect("calls lock"), ["locate", "parse"]);
}

#[test]
fn requested_manual_section_failure_is_not_hidden_by_tldr() {
    let mut host = host(Err("libmandoc failed".to_owned()));
    host.locate = Err("section not found".to_owned());
    host.tldr = Ok(Some(tldr()));
    let request = QueryRequest {
        schema: RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some("7".to_owned()),
        },
        view: QueryView::Full {},
    };

    let error = query_with(&request, QueryPolicy::default(), &host)
        .expect_err("an explicit section must require a native manual");

    assert!(matches!(&error, QueryError::Manual(_)));
    assert!(error.to_string().contains("section not found"));
    assert_eq!(*host.calls.lock().expect("calls lock"), ["locate"]);
}

#[test]
fn truncated_unsupported_document_is_an_error_by_default() {
    let host = host(Ok(document(SourceFormat::Man, true, false)));

    let QueryError::Manual(detail) = query_with(&request(), QueryPolicy::default(), &host)
        .expect_err("empty-section document must error by default")
    else {
        panic!("expected Manual error");
    };
    assert!(detail.to_string().contains("produced no readable sections"));
}

#[test]
fn readable_best_effort_document_survives_parser_findings() {
    let host = host(Ok(document(SourceFormat::Mdoc, true, true)));
    let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");
    assert_eq!(
        result.document.expect("manual").source.format,
        SourceFormat::Mdoc
    );
}

#[test]
fn ordinary_query_reports_a_tldr_hint_after_total_document_failure() {
    let mut host = host(Err("libmandoc failed".to_owned()));
    host.locate = Err("source not found".to_owned());
    host.tldr = Ok(Some(tldr()));
    let error = query_with(&request(), QueryPolicy::default(), &host)
        .expect_err("ordinary query must require a full document");

    assert!(matches!(error, QueryError::ManualWithTldr { .. }));
    assert_eq!(
        error.to_string(),
        "could not load manual 'tool': source not found\nhint: a tldr entry is available; run `mant tool --tldr`"
    );
}

#[test]
fn explicit_tldr_policy_survives_total_manual_failure() {
    let mut host = host(Err("libmandoc failed".to_owned()));
    host.locate = Err("source not found".to_owned());
    host.tldr = Ok(Some(tldr()));
    let result =
        query_with(&request(), QueryPolicy::TldrOnly, &host).expect("explicit tldr-only query");

    assert!(result.document.is_none());
    assert_eq!(result.tldr.expect("tldr").title, "tool");
}

#[test]
fn positive_source_embedded_tldr_precedes_the_builtin_cache() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(1);
    host.markdown = Ok(embedded_tldr_markdown());
    host.tldr = Ok(Some(tldr()));

    let result = query_with(&request(), QueryPolicy::TldrOnly, &host)
        .expect("positive-priority embedded tldr");

    assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
    assert_eq!(
        result.address,
        Some(DocumentAddress::Markdown {
            path: "tool".to_owned(),
            origin: MarkdownOrigin::Source {
                name: "team".to_owned(),
            },
        })
    );
    assert_eq!(*host.calls.lock().expect("calls"), ["name", "markdown"]);
}

#[test]
fn builtin_tldr_cache_wins_a_zero_priority_tie() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(0);
    host.markdown = Ok(embedded_tldr_markdown());
    host.tldr = Ok(Some(tldr()));

    let result = query_with(&request(), QueryPolicy::TldrOnly, &host).expect("builtin tldr cache");

    assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::TldrPages);
    assert_eq!(*host.calls.lock().expect("calls"), ["name", "tldr"]);
}

#[test]
fn tldr_lookup_skips_markdown_without_an_embedded_quick_reference() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(10);
    host.markdown = Ok("# Tool\n\nFull documentation only.\n".to_owned());
    host.tldr = Ok(Some(tldr()));

    let result = query_with(&request(), QueryPolicy::TldrOnly, &host)
        .expect("cached tldr after empty Markdown candidate");

    assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::TldrPages);
    assert_eq!(
        *host.calls.lock().expect("calls"),
        ["name", "markdown", "tldr"]
    );
}

#[test]
fn negative_source_embedded_tldr_is_the_final_fallback() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(-1);
    host.markdown = Ok(embedded_tldr_markdown());

    let result = query_with(&request(), QueryPolicy::TldrOnly, &host)
        .expect("negative-priority embedded tldr");

    assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
    assert_eq!(
        *host.calls.lock().expect("calls"),
        ["name", "tldr", "fallback", "markdown"]
    );
}

#[test]
fn explicit_source_limits_tldr_lookup_to_that_source() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(-1);
    host.markdown = Ok(embedded_tldr_markdown());
    host.tldr = Ok(Some(tldr()));
    let mut request = request();
    let QueryInput::Document { source, .. } = &mut request.input else {
        unreachable!("document request")
    };
    *source = Some("team".to_owned());

    let result =
        query_with(&request, QueryPolicy::TldrOnly, &host).expect("source-owned embedded tldr");

    assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
    assert_eq!(*host.calls.lock().expect("calls"), ["source", "markdown"]);
}

#[test]
fn reports_both_manual_paths_when_no_content_exists() {
    let mut host = host(Err("libmandoc failed".to_owned()));
    host.locate = Err("source not found".to_owned());
    let error =
        query_with(&request(), QueryPolicy::default(), &host).expect_err("empty query must fail");
    assert_eq!(
        error.to_string(),
        "could not load manual 'tool': source not found"
    );
}

#[test]
fn validates_before_touching_host_state() {
    let host = host(Ok(document(SourceFormat::Man, false, true)));
    assert_eq!(
        query_with(
            &QueryRequest {
                schema: RequestSchema::V0Dot10,
                input: QueryInput::Document {
                    selector: " ".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Full {},
            },
            QueryPolicy::default(),
            &host
        ),
        Err(QueryError::EmptyName)
    );
    assert!(host.calls.lock().expect("calls lock").is_empty());
}

#[test]
fn every_single_document_selector_obeys_the_shared_native_bound() {
    let oversized = "x".repeat(MAX_SEMANTIC_ENTRY_BYTES + 1);
    for (field, view) in [
        (
            "semantic entry",
            QueryView::Explain {
                entry: oversized.clone(),
            },
        ),
        (
            "outline node",
            QueryView::Excerpt {
                selectors: vec![oversized.clone().into()],
            },
        ),
        (
            "outline root",
            QueryView::Outline {
                entries: mant_protocol::EntryProjection::Summary,
                root: Some(oversized.clone().into()),
            },
        ),
    ] {
        let mut request = request();
        request.view = view;
        assert_eq!(
            validate_query_request(&request, QueryPolicy::default()),
            Err(QueryError::InvalidViewSelector {
                field,
                error: ScopeTextError::TooLong {
                    maximum: MAX_SEMANTIC_ENTRY_BYTES,
                },
            })
        );
    }
}

#[test]
fn explanation_misses_distinguish_visible_prose_from_absent_text() {
    let query = query_markdown_text(
        "# shell\n\n## Invocation\n\nThe option `-b` ends option processing.\n",
        None,
    )
    .expect("Markdown query");
    let error = project_query_view(
        query.clone(),
        &QueryView::Explain {
            entry: "-b".to_owned(),
        },
    )
    .expect_err("prose is not a semantic entry");
    let QueryExecutionError::Projection(ProjectionError::SelectorFoundOnlyInText {
        selector,
        path,
        title,
        line,
        ..
    }) = error
    else {
        panic!("expected prose-only selector diagnostic");
    };
    assert_eq!(selector, "-b");
    assert_eq!(path, "1");
    assert_eq!(title, "Invocation");
    assert!(line > 0);

    assert!(matches!(
        project_query_view(
            query,
            &QueryView::Explain {
                entry: "--absent".to_owned(),
            },
        ),
        Err(QueryExecutionError::Projection(
            ProjectionError::UnknownSelector { .. }
        ))
    ));
}

#[test]
fn registered_markdown_shadows_an_unqualified_manual_name() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
    host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());

    let result =
        query_with(&request(), QueryPolicy::default(), &host).expect("registered Markdown name");

    assert_eq!(result.label, "tool");
    assert!(result.tldr.is_none());
    let document = result.document.expect("registered document");
    assert_eq!(document.source.format, SourceFormat::Markdown);
    assert_eq!(document.source.path.as_deref(), Some("/data/mant/tool.md"));
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "markdown"],
        "a registered name must not consult man or external tldr caches"
    );
}

#[test]
fn positive_source_priority_shadows_a_native_manual() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(1);
    host.markdown = Ok("# Team tool\n\nConfigured documentation.\n".to_owned());

    let result =
        query_with(&request(), QueryPolicy::default(), &host).expect("positive-priority Markdown");

    assert_eq!(
        result.document.expect("document").source.format,
        SourceFormat::Markdown
    );
    assert_eq!(*host.calls.lock().expect("calls"), ["name", "markdown"]);
}

#[test]
fn native_manual_wins_a_zero_priority_tie() {
    let mut host = host(Ok(document(SourceFormat::Man, false, true)));
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(0);
    host.markdown = Ok("# Team tool\n\nConfigured documentation.\n".to_owned());

    let result = query_with(&request(), QueryPolicy::default(), &host).expect("native manual");

    assert_eq!(
        result.document.expect("document").source.format,
        SourceFormat::Man
    );
    assert_eq!(
        *host.calls.lock().expect("calls"),
        ["name", "locate", "parse", "tldr"]
    );
}

#[test]
fn non_positive_source_priority_falls_back_when_the_manual_is_unavailable() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.locate = Err("source not found".to_owned());
    host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
    host.registered_source_priority = Some(-1);
    host.markdown = Ok("# Team tool\n\nConfigured documentation.\n".to_owned());

    let result = query_with(&request(), QueryPolicy::default(), &host).expect("Markdown fallback");

    assert_eq!(
        result.document.expect("document").source.format,
        SourceFormat::Markdown
    );
    assert_eq!(
        *host.calls.lock().expect("calls"),
        ["name", "locate", "tldr", "fallback", "markdown"]
    );
}

#[test]
fn windows_suffix_fallback_can_resolve_registered_markdown() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.name_candidates = Some(vec!["tool".to_owned(), "tool.EXE".to_owned()]);
    host.registered_name = Some("tool.EXE".to_owned());
    host.registered_document = Some(PathBuf::from("/data/mant/tool.exe.md"));
    host.markdown = Ok("# Tool executable\n\nWindows command documentation.\n".to_owned());

    let result = query_with(&request(), QueryPolicy::default(), &host)
        .expect("registered executable document");

    assert_eq!(result.label, "tool");
    assert_eq!(
        result.document.expect("document").source.path.as_deref(),
        Some("/data/mant/tool.exe.md")
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "markdown"]
    );
}

#[test]
fn windows_suffix_fallback_can_resolve_a_native_manual() {
    let mut host = host(Ok(document(SourceFormat::Man, false, true)));
    host.name_candidates = Some(vec!["tool".to_owned(), "tool.EXE".to_owned()]);
    host.manual_name = Some("tool.EXE".to_owned());
    host.locate = Ok(ManualPage {
        name: "tool.exe".to_owned(),
        section: "1".to_owned(),
        path: PathBuf::from("/man/tool.exe.1"),
        manual_root: PathBuf::from("/man"),
    });

    let result =
        query_with(&request(), QueryPolicy::default(), &host).expect("native executable manual");

    assert_eq!(result.label, "tool");
    assert_eq!(
        result.document.expect("manual").source.format,
        SourceFormat::Man
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "locate", "locate", "parse", "tldr"]
    );
}

#[test]
fn exact_names_win_before_windows_suffix_fallback() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.name_candidates = Some(vec!["tool".to_owned(), "tool.EXE".to_owned()]);
    host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
    host.markdown = Ok("# Exact tool\n\nExact-name documentation.\n".to_owned());

    let result =
        query_with(&request(), QueryPolicy::default(), &host).expect("exact registered document");

    assert_eq!(
        result.document.expect("document").source.path.as_deref(),
        Some("/data/mant/tool.md")
    );
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["name", "markdown"]
    );
}

#[test]
fn markdown_files_bypass_manual_and_tldr_sources() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());
    let result = query_with(
        &QueryRequest {
            schema: RequestSchema::V0Dot10,
            input: QueryInput::File {
                path: "docs/tool.md".to_owned(),
                format: InputFormat::Markdown,
            },
            view: QueryView::Full {},
        },
        QueryPolicy::default(),
        &host,
    )
    .expect("Markdown query");

    assert_eq!(result.label, "tool.md");
    assert!(result.tldr.is_none());
    let document = result.document.expect("document");
    assert_eq!(document.source.format, SourceFormat::Markdown);
    assert_eq!(document.source.path.as_deref(), Some("docs/tool.md"));
    assert_eq!(
        *host.calls.lock().expect("calls lock"),
        ["markdown"],
        "Markdown must not consult man or tldr"
    );
}

#[test]
fn in_memory_markdown_is_available_without_a_protocol_content_field() {
    let result = query_markdown_text("# Piped\n\nBody.\n", None).expect("stdin Markdown query");

    assert_eq!(result.label, "stdin");
    assert!(result.tldr.is_none());
    let document = result.document.expect("document");
    assert_eq!(document.meta.title.as_deref(), Some("Piped"));
    assert_eq!(document.source.path, None);
}

#[test]
fn leading_tldr_directives_are_independent_from_the_markdown_document() {
    let source = "\
<!-- mant:tldr:start -->
# demo

> Concise embedded help.

- Run the demo:

`demo {{path}}`
<!-- mant:tldr:end -->

# Demo

Document overview.

## Options

- `--help`: Show help.
";
    let result =
        query_markdown_text(source, Some("docs/demo.md".to_owned())).expect("Markdown query");

    let tldr = result.tldr.expect("embedded tldr");
    assert_eq!(tldr.title, "demo");
    assert_eq!(tldr.origin, TldrOrigin::Embedded);
    assert_eq!(tldr.source_path, "docs/demo.md");
    assert_eq!(tldr.examples[0].command, "demo {{path}}");

    let document = result.document.expect("document body");
    assert_eq!(document.meta.title.as_deref(), Some("Demo"));
    assert_eq!(document.sections[0].title, "Options");
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, mant_ir::Block::Paragraph { .. }))
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("mant:tldr"))
    );
}

#[test]
fn malformed_leading_tldr_directives_report_the_source_path() {
    let error = query_markdown_text(
        "<!-- mant:tldr:start -->\n# demo\n\n- Run:\n\n`demo`\n",
        Some("docs/broken.md".to_owned()),
    )
    .expect_err("unterminated directive");

    assert_eq!(
        error.to_string(),
        "could not load Markdown document 'docs/broken.md': top-level <!-- mant:tldr:start --> marker is missing its <!-- mant:tldr:end --> marker"
    );
}

#[test]
fn capped_read_accepts_input_up_to_the_limit() {
    let source = "abcd";
    assert_eq!(
        read_capped_utf8(source.as_bytes(), source.len() as u64).expect("within limit"),
        source
    );
}

#[test]
fn capped_read_rejects_input_past_the_limit_without_buffering_it_whole() {
    // An unbounded stream (modelled by io::repeat) must fail fast on the
    // limit rather than read forever, matching the /dev/zero guard.
    let error = read_capped_utf8(io::repeat(b'a'), 8).expect_err("over limit");
    assert!(error.contains("exceeds the 8-byte limit"), "{error}");
}

#[test]
fn capped_read_rejects_non_utf8_input() {
    let error = read_capped_utf8(&[0xff, 0xfe][..], MAX_MARKDOWN_BYTES).expect_err("invalid UTF-8");
    assert!(error.contains("must be UTF-8"), "{error}");
}

#[test]
fn capped_io_read_preserves_the_underlying_error_kind() {
    struct PermissionDeniedReader;

    impl io::Read for PermissionDeniedReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reader denied access",
            ))
        }
    }

    let error = read_capped_utf8_io(PermissionDeniedReader, MAX_MARKDOWN_BYTES)
        .expect_err("reader failure is preserved");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "reader denied access");
}
