use super::{
    LoadError, LoadHost, LoadPolicy, LoadSpec, MAX_MARKDOWN_BYTES, RegisteredLookupPhase,
    RegisteredSelection, RegisteredSelectionGroup, load_markdown_text, read_capped_utf8,
    read_capped_utf8_io, validate_load_spec,
};
use crate::{ManualPage, ManualRequest};
use mant_ir::{
    Block, Diagnostic, DiagnosticLevel, Document, DocumentAddress, DocumentMeta, DocumentSource,
    Inline, LayoutHint, MarkdownOrigin, Section, SourceFormat, TldrDocument, TldrOrigin,
};
use mant_protocol::{InputFormat, MAX_DOCUMENT_SELECTOR_CHARS, ScopeTextError};
use mant_sources::BUILTIN_CONTENT_PRIORITY;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};
struct TestRequest {
    input: TestInput,
}
enum TestInput {
    Document {
        selector: String,
        source: Option<String>,
        manual_section: Option<String>,
    },
    File {
        path: String,
        format: InputFormat,
    },
}
impl TestRequest {
    fn spec(&self) -> LoadSpec<'_> {
        match &self.input {
            TestInput::Document {
                selector,
                source,
                manual_section,
            } => LoadSpec::Document {
                selector,
                source: source.as_deref(),
                manual_section: manual_section.as_deref(),
            },
            TestInput::File { path, format } => LoadSpec::File {
                path,
                format: *format,
            },
        }
    }
}
fn validate_request(request: &TestRequest, policy: LoadPolicy) -> Result<(), LoadError> {
    validate_load_spec(request.spec(), policy)
}
fn load_request(
    request: &TestRequest,
    policy: LoadPolicy,
    host: &dyn LoadHost,
) -> Result<mant_ir::ResolvedContent, LoadError> {
    super::load_with(request.spec(), policy, host)
}
#[derive(Clone)]
struct StubHost {
    native_available: bool,
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

impl LoadHost for StubHost {
    fn native_available(&self) -> bool {
        self.native_available
    }
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
        heading: None,
        parser: None,
        source: DocumentSource { format, path: None },
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: unsupported
            .then_some(Diagnostic {
                impact: mant_ir::DiagnosticImpact::None,
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
                fragment_aliases: Vec::new(),
                heading: "NAME".into(),
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
        native_available: true,
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

fn request() -> TestRequest {
    TestRequest {
        input: TestInput::Document {
            selector: " tool ".to_owned(),
            source: None,
            manual_section: None,
        },
    }
}

#[test]
fn unavailable_native_backend_rejects_explicit_inputs_before_native_access() {
    for (spec, policy) in [
        (
            LoadSpec::Document {
                selector: "tool",
                source: None,
                manual_section: None,
            },
            LoadPolicy::ManualOnly,
        ),
        (
            LoadSpec::Document {
                selector: "tool",
                source: None,
                manual_section: Some("1"),
            },
            LoadPolicy::Combined,
        ),
        (
            LoadSpec::Document {
                selector: "manual/1/tool",
                source: None,
                manual_section: None,
            },
            LoadPolicy::Combined,
        ),
        (
            LoadSpec::File {
                path: "missing.1",
                format: InputFormat::Auto,
            },
            LoadPolicy::Combined,
        ),
        (
            LoadSpec::File {
                path: "missing",
                format: InputFormat::Roff,
            },
            LoadPolicy::Combined,
        ),
    ] {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.native_available = false;
        assert_eq!(
            super::load_with(spec, policy, &host),
            Err(LoadError::NativeBackendUnavailable { tldr_topic: None })
        );
        assert!(host.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn unavailable_native_backend_preserves_markdown_precedence_and_tldr_policy() {
    let mut host = host(Ok(document(SourceFormat::Man, false, true)));
    host.native_available = false;
    host.registered_document = Some(PathBuf::from("fallback.md"));
    host.registered_source_priority = Some(0);
    host.markdown = Ok("# Fallback\n\nBody.\n".to_owned());
    host.tldr = Ok(Some(tldr()));
    let spec = LoadSpec::Document {
        selector: "tool",
        source: None,
        manual_section: None,
    };
    let content = super::load_with(spec, LoadPolicy::Combined, &host).unwrap();
    let document = content.document.unwrap();
    // A Markdown H1 is a visible heading, not native TH/Dt metadata.
    assert_eq!(document.heading.unwrap().plain_text(), "Fallback");
    assert_eq!(document.meta.title, None);
    assert_eq!(document.source.format, SourceFormat::Markdown);
    assert_eq!(document.source.path.as_deref(), Some("fallback.md"));
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["name", "tldr", "fallback", "markdown"]
    );

    host.calls.lock().unwrap().clear();
    host.registered_document = None;
    assert_eq!(
        super::load_with(spec, LoadPolicy::Combined, &host),
        Err(LoadError::NativeBackendUnavailable {
            tldr_topic: Some("tool".to_owned())
        })
    );
    assert_eq!(*host.calls.lock().unwrap(), ["name", "tldr", "fallback"]);

    host.calls.lock().unwrap().clear();
    assert!(
        super::load_with(spec, LoadPolicy::TldrOnly, &host)
            .unwrap()
            .tldr
            .is_some()
    );
    assert!(
        !host
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| matches!(*call, "locate" | "parse"))
    );
}

#[test]
fn ordinary_manual_uses_the_native_parser() {
    let host = host(Ok(document(SourceFormat::Man, false, true)));
    let result = load_request(&request(), LoadPolicy::default(), &host).expect("query");

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
            load_request(&request(), LoadPolicy::default(), &host).expect("command manual query");

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

    let result =
        load_request(&request(), LoadPolicy::default(), &host).expect("file format manual");

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
    let request = TestRequest {
        input: TestInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some("3".to_owned()),
        },
    };

    let result = load_request(&request, LoadPolicy::default(), &host).expect("query");
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
    let request = TestRequest {
        input: TestInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some("1".to_owned()),
        },
    };

    let result = load_request(&request, LoadPolicy::Combined, &host)
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
    let request_for = |section: &str| TestRequest {
        input: TestInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some(section.to_owned()),
        },
    };

    let result = load_request(&request_for("1"), LoadPolicy::TldrOnly, &host)
        .expect("section 1 identifies a command topic");
    assert_eq!(result.tldr.expect("tldr").title, "tool");

    assert_eq!(
        load_request(&request_for("5"), LoadPolicy::TldrOnly, &host),
        Err(LoadError::TldrManualSection {
            section: "5".to_owned(),
        })
    );
}

#[test]
fn explicit_source_reads_only_registered_markdown() {
    let mut host = host(Err("manual must not be read".to_owned()));
    host.registered_document = Some(PathBuf::from("/documents/tool.md"));
    host.markdown = Ok("# Tool\n\nSource body.\n".to_owned());
    let request = TestRequest {
        input: TestInput::Document {
            selector: "tool".to_owned(),
            source: Some("team".to_owned()),
            manual_section: None,
        },
    };
    let result = load_request(&request, LoadPolicy::default(), &host).expect("source query");
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
        result
            .document
            .expect("Markdown")
            .display_title()
            .as_deref(),
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
    let request = TestRequest {
        input: TestInput::Document {
            selector: "documents/en/tool".to_owned(),
            source: None,
            manual_section: None,
        },
    };
    let result = load_request(&request, LoadPolicy::default(), &markdown).expect("canonical");
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
    let request = TestRequest {
        input: TestInput::Document {
            selector: "manual/1/tool".to_owned(),
            source: None,
            manual_section: None,
        },
    };
    let result = load_request(&request, LoadPolicy::default(), &manual).expect("manual path");
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
    let result = load_request(&request(), LoadPolicy::default(), &host).expect("query");

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
    let result =
        load_request(&request(), LoadPolicy::ManualOnly, &host).expect("manual-only query");

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

    let error = load_request(&request(), LoadPolicy::ManualOnly, &host)
        .expect_err("an optional tldr page must not hide native parser failure");

    let LoadError::Manual(detail) = error else {
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
    let request = TestRequest {
        input: TestInput::Document {
            selector: "tool".to_owned(),
            source: None,
            manual_section: Some("7".to_owned()),
        },
    };

    let error = load_request(&request, LoadPolicy::default(), &host)
        .expect_err("an explicit section must require a native manual");

    assert!(matches!(&error, LoadError::Manual(_)));
    assert!(error.to_string().contains("section not found"));
    assert_eq!(*host.calls.lock().expect("calls lock"), ["locate"]);
}

#[test]
fn truncated_unsupported_document_is_an_error_by_default() {
    let host = host(Ok(document(SourceFormat::Man, true, false)));

    let LoadError::Manual(detail) = load_request(&request(), LoadPolicy::default(), &host)
        .expect_err("empty-section document must error by default")
    else {
        panic!("expected Manual error");
    };
    assert!(detail.to_string().contains("produced no readable sections"));
}

#[test]
fn readable_best_effort_document_survives_parser_findings() {
    let host = host(Ok(document(SourceFormat::Mdoc, true, true)));
    let result = load_request(&request(), LoadPolicy::default(), &host).expect("query");
    assert_eq!(
        result.document.expect("manual").source.format,
        SourceFormat::Mdoc
    );
}

#[test]
fn root_only_native_document_is_readable() {
    let mut root_only = document(SourceFormat::Man, false, false);
    root_only.blocks.push(Block::Paragraph {
        children: vec![Inline::Text {
            value: "manual text before any section".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    });
    let host = host(Ok(root_only));

    let result = load_request(&request(), LoadPolicy::default(), &host).expect("root content");
    let document = result.document.expect("manual");
    assert!(document.sections.is_empty());
    assert_eq!(document.blocks.len(), 1);
}

#[test]
fn ordinary_query_reports_a_tldr_hint_after_total_document_failure() {
    let mut host = host(Err("libmandoc failed".to_owned()));
    host.locate = Err("source not found".to_owned());
    host.tldr = Ok(Some(tldr()));
    let error = load_request(&request(), LoadPolicy::default(), &host)
        .expect_err("ordinary query must require a full document");

    assert!(matches!(error, LoadError::ManualWithTldr { .. }));
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
        load_request(&request(), LoadPolicy::TldrOnly, &host).expect("explicit tldr-only query");

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

    let result = load_request(&request(), LoadPolicy::TldrOnly, &host)
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

    let result = load_request(&request(), LoadPolicy::TldrOnly, &host).expect("builtin tldr cache");

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

    let result = load_request(&request(), LoadPolicy::TldrOnly, &host)
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

    let result = load_request(&request(), LoadPolicy::TldrOnly, &host)
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
    let TestInput::Document { source, .. } = &mut request.input else {
        unreachable!("document request")
    };
    *source = Some("team".to_owned());

    let result =
        load_request(&request, LoadPolicy::TldrOnly, &host).expect("source-owned embedded tldr");

    assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
    assert_eq!(*host.calls.lock().expect("calls"), ["source", "markdown"]);
}

#[test]
fn reports_both_manual_paths_when_no_content_exists() {
    let mut host = host(Err("libmandoc failed".to_owned()));
    host.locate = Err("source not found".to_owned());
    let error =
        load_request(&request(), LoadPolicy::default(), &host).expect_err("empty query must fail");
    assert_eq!(
        error.to_string(),
        "could not load manual 'tool': source not found"
    );
}

#[test]
fn borrowed_load_specs_execute_without_request_schema_or_query_view() {
    let loading_host = host(Ok(document(SourceFormat::Man, false, true)));
    let selector = String::from("tool");
    let spec = LoadSpec::Document {
        selector: &selector,
        source: None,
        manual_section: Some("1"),
    };
    let loaded = super::load_with(spec, LoadPolicy::ManualOnly, &loading_host).unwrap();
    assert_eq!(loaded.label, "tool");
    assert!(loaded.document.is_some());
    assert!(loaded.tldr.is_none());
    assert_eq!(
        *loading_host.calls.lock().unwrap(),
        ["locate", "parse"],
        "manual-only loading does not inspect registered names or tldr"
    );

    let invalid_host = host(Ok(document(SourceFormat::Man, false, true)));
    assert!(matches!(
        super::load_with(
            LoadSpec::Document {
                selector: "bad\nname",
                source: None,
                manual_section: None
            },
            LoadPolicy::Combined,
            &invalid_host,
        ),
        Err(LoadError::InvalidSelector {
            field: "document selector",
            ..
        })
    ));
    assert!(invalid_host.calls.lock().unwrap().is_empty());
}

#[test]
fn validates_before_touching_host_state() {
    let host = host(Ok(document(SourceFormat::Man, false, true)));
    assert_eq!(
        load_request(
            &TestRequest {
                input: TestInput::Document {
                    selector: " ".to_owned(),
                    source: None,
                    manual_section: None,
                },
            },
            LoadPolicy::default(),
            &host
        ),
        Err(LoadError::EmptyName)
    );
    assert!(host.calls.lock().expect("calls lock").is_empty());
}

#[test]
fn document_input_selector_obeys_the_shared_native_bound() {
    let mut request = request();
    request.input = TestInput::Document {
        selector: "界".repeat(MAX_DOCUMENT_SELECTOR_CHARS + 1),
        source: None,
        manual_section: None,
    };
    assert_eq!(
        validate_request(&request, LoadPolicy::default()),
        Err(LoadError::InvalidSelector {
            field: "document selector",
            error: ScopeTextError::TooLong {
                maximum: MAX_DOCUMENT_SELECTOR_CHARS,
            },
        })
    );

    let TestInput::Document { selector, .. } = &mut request.input else {
        unreachable!()
    };
    *selector = "bad\nselector".to_owned();
    assert_eq!(
        validate_request(&request, LoadPolicy::default()),
        Err(LoadError::InvalidSelector {
            field: "document selector",
            error: ScopeTextError::ControlCharacter,
        })
    );
}

#[test]
fn registered_markdown_shadows_an_unqualified_manual_name() {
    let mut host = host(Err("manual parser must not run".to_owned()));
    host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
    host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());

    let result =
        load_request(&request(), LoadPolicy::default(), &host).expect("registered Markdown name");

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
        load_request(&request(), LoadPolicy::default(), &host).expect("positive-priority Markdown");

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

    let result = load_request(&request(), LoadPolicy::default(), &host).expect("native manual");

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

    let result = load_request(&request(), LoadPolicy::default(), &host).expect("Markdown fallback");

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

    let result = load_request(&request(), LoadPolicy::default(), &host)
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
        load_request(&request(), LoadPolicy::default(), &host).expect("native executable manual");

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
        load_request(&request(), LoadPolicy::default(), &host).expect("exact registered document");

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
    let result = load_request(
        &TestRequest {
            input: TestInput::File {
                path: "docs/tool.md".to_owned(),
                format: InputFormat::Markdown,
            },
        },
        LoadPolicy::default(),
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
    let result = load_markdown_text("# Piped\n\nBody.\n", None).expect("stdin Markdown query");

    assert_eq!(result.label, "stdin");
    assert!(result.tldr.is_none());
    let document = result.document.expect("document");
    assert_eq!(document.display_title().as_deref(), Some("Piped"));
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
        load_markdown_text(source, Some("docs/demo.md".to_owned())).expect("Markdown query");

    let tldr = result.tldr.expect("embedded tldr");
    assert_eq!(tldr.title, "demo");
    assert_eq!(tldr.origin, TldrOrigin::Embedded);
    assert_eq!(tldr.source_path, "docs/demo.md");
    assert_eq!(tldr.examples[0].command, "demo {{path}}");

    let document = result.document.expect("document body");
    assert_eq!(document.display_title().as_deref(), Some("Demo"));
    assert_eq!(document.sections[0].heading.plain_text(), "Options");
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
    let error = load_markdown_text(
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
