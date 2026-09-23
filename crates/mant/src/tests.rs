use std::cell::Cell;

use mant_ir::ResolvedContent;

use mant_sources::{
    DocumentSourcesPrune, DocumentSourcesPruneSchema, DocumentSourcesUpdate,
    DocumentSourcesUpdateSchema,
};

use mant_ir::{
    Block, ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, DefinitionItem,
    Document, DocumentMeta, EntryFacts, EntryKind, Heading, Inline, LayoutHint, NameCase,
    Provenance, Section, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
    TldrDocument, TldrOrigin,
};
use mant_protocol::{
    CatalogSchema, DoctorCheck, DoctorCheckStatus, DoctorEnvironment, DoctorReport,
    DocumentSummary, MarkdownOrigin, Producer, QueryInput, QueryRequest, TldrCacheAction,
    TldrCacheUpdate,
};

use crate::{
    CLI_PROTOCOL_VERSION,
    application::request_for_address,
    arguments::{self, ColorMode, Command, DisplayMode, OutputOptions},
    cli::{run_command, run_with_host},
    error::Failure,
    host::CliHost,
    output_policy::{TerminalCapabilities, TerminalKind, resolve_process_presentation},
    request_input::read_native_request,
};
use mant_loader::LoadPolicy;
use mant_protocol::{CatalogQuery, DocumentAddress, DocumentCatalog};

#[test]
fn invalid_complete_views_are_rejected_before_the_cli_host_is_called() {
    let host = FakeHost::new();
    for view in [
        mant_protocol::QueryView::Excerpt { selectors: vec![] },
        mant_protocol::QueryView::Explain {
            entry: String::new(),
            options: mant_protocol::ExplanationOptions::default(),
        },
        mant_protocol::QueryView::Outline {
            entries: mant_protocol::EntryProjection::Kinds { kinds: vec![] },
            root: None,
            references: mant_protocol::ReferenceProjection::default(),
        },
        mant_protocol::QueryView::Search {
            pattern: "[".into(),
            syntax: mant_protocol::SearchSyntax::Regex,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    ] {
        let request = QueryRequest {
            schema: mant_protocol::RequestSchema::V0Dot12,
            input: QueryInput::Document {
                selector: "tool".into(),
                source: None,
                manual_section: None,
            },
            view,
        };
        assert!(crate::application::execute_query(&request, LoadPolicy::Combined, &host).is_err());
        assert_eq!(host.query_calls.get(), 0);
    }
    let scope = mant_protocol::ScopeQueryRequest {
        schema: mant_protocol::ScopeRequestSchema::V0Dot12,
        scope: mant_protocol::DocumentScope {
            documents: vec![mant_protocol::DocumentSelector {
                selector: "tool".into(),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal::default(),
        },
        view: mant_protocol::ScopeQueryView::Explain {
            entry: String::new(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    };
    assert!(crate::application::execute_scope_query(&scope, &host).is_err());
    assert_eq!(host.query_calls.get(), 0);
}

struct FakeHost {
    query_calls: Cell<usize>,
    update_calls: Cell<usize>,
    last_policy: Cell<LoadPolicy>,
    document: Option<Document>,
    tldr: Option<TldrDocument>,
    doctor_error: bool,
}

#[test]
fn catalog_addresses_reopen_the_exact_source_or_manual_section() {
    let (request, policy) = request_for_address(&DocumentAddress::Markdown {
        path: "Start-Process".to_owned(),
        origin: MarkdownOrigin::Source {
            name: "pwsh7".to_owned(),
        },
    });
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "sources/pwsh7/Start-Process".to_owned(),
            source: None,
            manual_section: None,
        }
    );
    assert_eq!(policy, LoadPolicy::Combined);

    let (request, policy) = request_for_address(&DocumentAddress::Manual {
        name: "printf".to_owned(),
        manual_section: "3".to_owned(),
    });
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "manual/3/printf".to_owned(),
            source: None,
            manual_section: None,
        }
    );
    assert_eq!(policy, LoadPolicy::ManualOnly);

    let (request, policy) = request_for_address(&DocumentAddress::Markdown {
        path: "en/tool".into(),
        origin: MarkdownOrigin::Documents,
    });
    assert_eq!(policy, LoadPolicy::Combined);
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "documents/en/tool".into(),
            source: None,
            manual_section: None,
        }
    );
}

#[test]
fn unqualified_manual_navigation_preserves_native_resolution_without_a_default_section() {
    let (request, policy) =
        crate::application::request_for_navigation(&mant_protocol::DocumentOpenTarget::Manual {
            name: "printf".into(),
            manual_section: None,
        });
    assert_eq!(policy, LoadPolicy::ManualOnly);
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "printf".into(),
            source: None,
            manual_section: None,
        }
    );
}

#[test]
fn terminal_capabilities_resolve_interactivity_and_text_colour() {
    let full_display = if cfg!(feature = "tui") {
        DisplayMode::Tui
    } else if cfg!(feature = "pager") {
        DisplayMode::Pager
    } else {
        DisplayMode::Direct
    };
    let partial_display = if cfg!(feature = "pager") {
        DisplayMode::Pager
    } else {
        DisplayMode::Direct
    };
    let mut terminal_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
    resolve_process_presentation(
        &mut terminal_query,
        TerminalCapabilities {
            input: true,
            output: true,
            color: true,
            kind: TerminalKind::Capable,
        },
    )
    .expect("terminal query");
    assert!(matches!(
        terminal_query,
        Command::Query {
            presentation: OutputOptions {
                display,
                ..
            },
            ..
        } if display == full_display
    ));

    let mut redirected_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
    resolve_process_presentation(
        &mut redirected_query,
        TerminalCapabilities {
            input: true,
            output: false,
            color: true,
            kind: TerminalKind::Capable,
        },
    )
    .expect("redirected query");
    assert!(matches!(
        redirected_query,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Never,
                display: DisplayMode::Direct
            },
            ..
        }
    ));

    let mut outline =
        arguments::parse(&["git".to_owned(), "--outline".to_owned()]).expect("outline query");
    resolve_process_presentation(
        &mut outline,
        TerminalCapabilities {
            input: true,
            output: true,
            color: true,
            kind: TerminalKind::Capable,
        },
    )
    .expect("outline remains non-interactive");
    assert!(matches!(
        outline,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Always,
                display
            },
            ..
        } if display == partial_display
    ));

    let mut tldr = arguments::parse(&["git".to_owned(), "--tldr".to_owned()]).expect("tldr query");
    resolve_process_presentation(
        &mut tldr,
        TerminalCapabilities {
            input: true,
            output: true,
            color: true,
            kind: TerminalKind::Capable,
        },
    )
    .expect("tldr remains non-interactive");
    assert!(matches!(
        tldr,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Always,
                ..
            },
            ..
        }
    ));
}

#[test]
#[cfg(feature = "tui")]
fn explicit_interactive_queries_require_both_terminal_streams() {
    for terminal in [
        TerminalCapabilities {
            input: false,
            output: true,
            color: true,
            kind: TerminalKind::Capable,
        },
        TerminalCapabilities {
            input: true,
            output: false,
            color: true,
            kind: TerminalKind::Capable,
        },
        TerminalCapabilities {
            input: true,
            output: true,
            color: false,
            kind: TerminalKind::Dumb,
        },
    ] {
        let mut command =
            arguments::parse(&["git".to_owned(), "--display".to_owned(), "tui".to_owned()])
                .expect("UI query");
        let error = resolve_process_presentation(&mut command, terminal)
            .expect_err("incomplete terminal must fail");
        assert!(error.message().contains("interactive display requires"));
    }
}

#[test]
fn dumb_term_uses_copyable_output_for_automatic_queries() {
    let mut command = arguments::parse(&["git".to_owned()]).expect("automatic query");
    resolve_process_presentation(
        &mut command,
        TerminalCapabilities {
            input: true,
            output: true,
            color: false,
            kind: TerminalKind::Dumb,
        },
    )
    .expect("dumb terminal falls back to output");

    assert!(matches!(
        command,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Never,
                display: DisplayMode::Direct
            },
            ..
        }
    ));
}

#[test]
fn automatic_full_text_retains_explicit_and_detected_colour() {
    for (flags, input, output, capable_color, expected) in [
        (vec![], false, true, true, ColorMode::Always),
        (vec![], true, false, true, ColorMode::Never),
        (vec![], false, true, false, ColorMode::Never),
        (
            vec!["--color", "always"],
            true,
            false,
            false,
            ColorMode::Always,
        ),
        (
            vec!["--color", "never"],
            false,
            true,
            true,
            ColorMode::Never,
        ),
    ] {
        let mut args = vec!["demo".to_owned()];
        args.extend(flags.into_iter().map(str::to_owned));
        let mut command = arguments::parse(&args).expect("full query");
        resolve_process_presentation(
            &mut command,
            TerminalCapabilities {
                input,
                output,
                color: capable_color,
                kind: TerminalKind::Capable,
            },
        )
        .expect("resolve text colour");
        assert!(matches!(command, Command::Query {
            presentation: OutputOptions {
                format: None, color, ..
            }, ..
        } if color == expected));
    }
}

#[test]
fn catalog_paging_requires_text_and_a_complete_non_dumb_terminal() {
    let should_page_catalog = |command: &Command, terminal| {
        resolve_process_presentation(&mut command.clone(), terminal).expect("resolve catalog")
            == DisplayMode::Pager
    };
    let terminal = TerminalCapabilities {
        input: true,
        output: true,
        color: false,
        kind: TerminalKind::Capable,
    };
    let list = arguments::parse(&["--list".to_owned()]).expect("catalog list");
    assert_eq!(
        should_page_catalog(&list, terminal),
        cfg!(feature = "pager")
    );

    let direct = arguments::parse(&[
        "--list".to_owned(),
        "--display".to_owned(),
        "direct".to_owned(),
    ])
    .expect("direct catalog list");
    assert!(!should_page_catalog(&direct, terminal));

    let json = arguments::parse(&[
        "--find".to_owned(),
        "git".to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ])
    .expect("catalog JSON");
    assert!(!should_page_catalog(&json, terminal));
    assert!(!should_page_catalog(
        &list,
        TerminalCapabilities {
            output: false,
            ..terminal
        }
    ));
    assert!(!should_page_catalog(
        &list,
        TerminalCapabilities {
            kind: TerminalKind::Dumb,
            ..terminal
        }
    ));
}

impl FakeHost {
    fn new() -> Self {
        Self {
            query_calls: Cell::new(0),
            update_calls: Cell::new(0),
            last_policy: Cell::new(LoadPolicy::default()),
            document: None,
            tldr: None,
            doctor_error: false,
        }
    }

    fn with_doctor_error() -> Self {
        Self {
            doctor_error: true,
            ..Self::new()
        }
    }

    fn with_manual() -> Self {
        Self {
            document: Some(manual()),
            ..Self::new()
        }
    }

    fn with_manual_and_tldr() -> Self {
        Self {
            document: Some(manual()),
            tldr: Some(tldr()),
            ..Self::new()
        }
    }

    fn with_tldr() -> Self {
        Self {
            tldr: Some(tldr()),
            ..Self::new()
        }
    }

    fn with_explainable_manual() -> Self {
        Self {
            document: Some(explainable_manual()),
            ..Self::new()
        }
    }

    fn with_semantic_markdown() -> Self {
        Self {
            document: Some(semantic_markdown()),
            ..Self::new()
        }
    }
}

impl CliHost for FakeHost {
    fn doctor(&self) -> Result<DoctorReport, Failure> {
        Ok(DoctorReport::new(
            Producer {
                name: "mant".to_owned(),
                version: "0.9.0".to_owned(),
                engine: None,
            },
            DoctorEnvironment {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                data_root: Some("/data/mant".to_owned()),
                config_path: Some("/data/mant/sources.toml".to_owned()),
                documents_root: Some("/data/mant/documents".to_owned()),
                sources_root: Some("/data/mant/sources".to_owned()),
                manual_roots: Vec::new(),
                tldr_roots: Vec::new(),
            },
            vec![DoctorCheck {
                code: "runtime.fixture".to_owned(),
                subject: None,
                status: if self.doctor_error {
                    DoctorCheckStatus::Error
                } else {
                    DoctorCheckStatus::Ok
                },
                message: "fixture result".to_owned(),
                details: Vec::new(),
                remediation: None,
            }],
        ))
    }

    fn discover(&self, _query: &CatalogQuery) -> Result<DocumentCatalog, Failure> {
        Ok(DocumentCatalog {
            schema: CatalogSchema::V0Dot12,
            query: CatalogQuery::default(),
            coverage: mant_protocol::CatalogCoverage::default(),
            total: 2,
            returned: 2,
            offset: 0,
            truncated: false,
            next_offset: None,
            documents: vec![
                DocumentSummary {
                    address: DocumentAddress::Markdown {
                        path: "guide".to_owned(),
                        origin: MarkdownOrigin::Source {
                            name: "team".to_owned(),
                        },
                    },
                },
                DocumentSummary {
                    address: DocumentAddress::Manual {
                        name: "printf".to_owned(),
                        manual_section: "3".to_owned(),
                    },
                },
            ],
        })
    }

    fn query(
        &self,
        prepared: &mant_engine::PreparedQueryRequest<'_>,
    ) -> Result<mant_engine::QueryViewResult, Failure> {
        let request = prepared.request();
        self.query_calls.set(self.query_calls.get() + 1);
        self.last_policy.set(prepared.policy());
        let label = match &request.input {
            QueryInput::Document { selector, .. } => selector.trim().to_owned(),
            QueryInput::File { path, .. } => path.clone(),
        };
        let content = ResolvedContent {
            address: None,
            label,
            document: self.document.clone(),
            tldr: self.tldr.clone(),
        };
        mant_engine::project_query_view(content, &request.view)
            .map_err(crate::error::query_execution_failure)
    }

    fn query_scope(
        &self,
        _request: &mant_engine::PreparedScopeQuery<'_>,
    ) -> Result<mant_protocol::ScopeQueryResponse, Failure> {
        self.query_calls.set(self.query_calls.get() + 1);
        Err(Failure::operational(
            "document scope queries are unavailable in this host",
        ))
    }

    fn query_markdown(&self, _source: &str) -> Result<ResolvedContent, Failure> {
        self.query_calls.set(self.query_calls.get() + 1);
        Ok(ResolvedContent {
            address: None,
            label: "stdin".to_owned(),
            document: self.document.clone(),
            tldr: None,
        })
    }

    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
        self.update_calls.set(self.update_calls.get() + 1);
        Ok(TldrCacheUpdate {
            schema: mant_protocol::TldrCacheUpdateSchema::V1,
            action: TldrCacheAction::Updated,
            cache_dir: Some("/cache/tldr".to_owned()),
            client: None,
            output: None,
            revision: Some("abc123".to_owned()),
        })
    }

    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure> {
        Ok(DocumentSourcesUpdate {
            schema: DocumentSourcesUpdateSchema::V2,
            config: "/data/mant/sources.toml".to_owned(),
            sources: Vec::new(),
            orphaned: Vec::new(),
        })
    }

    fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure> {
        Ok(DocumentSourcesPrune {
            schema: DocumentSourcesPruneSchema::V1,
            config: "/data/mant/sources.toml".to_owned(),
            dry_run,
            sources: Vec::new(),
        })
    }
}

fn invoke(arguments: &[&str], input: &[u8], host: &FakeHost) -> (u8, String, String) {
    let arguments = arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut input = input;
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let status = run_with_host(&arguments, &mut input, &mut output, &mut diagnostics, host);
    (
        status,
        String::from_utf8(output).expect("UTF-8 output"),
        String::from_utf8(diagnostics).expect("UTF-8 diagnostics"),
    )
}

fn invoke_with_terminal_output(
    arguments: &[&str],
    input: &[u8],
    host: &FakeHost,
) -> (u8, String, String) {
    let arguments = arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let command = arguments::parse(&arguments).expect("valid terminal command");
    let mut input = input;
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let status = run_command(
        command,
        &mut input,
        &mut output,
        &mut diagnostics,
        host,
        false,
        super::presentation::OutputTarget::Terminal,
    );
    (
        status,
        String::from_utf8(output).expect("UTF-8 output"),
        String::from_utf8(diagnostics).expect("UTF-8 diagnostics"),
    )
}

#[test]
fn terminal_markdown_masks_direct_input_controls_but_redirected_markdown_is_exact() {
    let mut document = semantic_markdown();
    let flow = document.flow_mut().expect("fixture Flow body");
    let heading = flow.heading.as_mut().expect("fixture heading");
    let Inline::Text { content } = &mut heading.content[0] else {
        panic!("plain fixture heading");
    };
    let atom = &mut flow.content_store.atoms[(content.atom.get() - 1) as usize];
    let mant_ir::ContentAtomKind::Text { text, .. } = &mut atom.kind else {
        panic!("text fixture atom");
    };
    *text = "ris\u{1b}c".to_owned();
    content.bytes.end = u32::try_from(text.len()).unwrap();
    let host = FakeHost {
        document: Some(document),
        ..FakeHost::new()
    };
    let path = "input.md";
    let arguments = ["--input", path, "--format", "markdown"];

    let (status, redirected, diagnostics) = invoke(&arguments, b"", &host);
    assert_eq!(status, 0);
    assert!(diagnostics.is_empty());
    assert!(redirected.contains('\u{1b}'));

    let (status, terminal, diagnostics) = invoke_with_terminal_output(&arguments, b"", &host);
    assert_eq!(status, 0);
    assert!(diagnostics.is_empty());
    assert!(!terminal.contains('\u{1b}'));
    assert!(terminal.contains("ris�c"));
}

#[test]
#[cfg(feature = "annotated-preview")]
fn annotated_preview_uses_the_real_cli_query_and_recovers_after_rejected_input() {
    use std::{fs, path::PathBuf};

    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/annotated-preview-tests");
    fs::create_dir_all(&directory).expect("repository target directory");
    let path = directory.join(format!("preview-{}.1", std::process::id()));
    let name = path.to_str().expect("UTF-8 repository path");
    let host = FakeHost::new();
    let arguments = [
        "--annotated-preview",
        "--input",
        name,
        "--input-format",
        "roff",
        "--format",
        "text",
        "--display",
        "direct",
    ];

    fs::write(&path, b".so missing.1\n").expect("write disallowed redirect");
    let (status, output, diagnostics) = invoke(&arguments, b"", &host);
    assert_ne!(status, 0);
    assert!(output.is_empty());
    assert!(diagnostics.contains("standalone .so redirects"));
    assert_eq!(host.query_calls.get(), 0);

    // Oracle: pinned CVS man_term.c::terminal_man prints the NAME body once.
    // The exact minimal input was run through target/mandoc-migration/reference/mandoc.
    fs::write(&path, b".TH T 1\n.SH NAME\nT \\- preview\n").expect("write valid standalone source");
    let (status, output, diagnostics) = invoke(&arguments, b"", &host);
    fs::remove_file(&path).expect("remove test-owned source");
    assert_eq!(status, 0, "{diagnostics}");
    assert!(diagnostics.is_empty());
    assert_eq!(output.matches("preview").count(), 1);
    assert_eq!(
        host.query_calls.get(),
        0,
        "preview must not use the old loader"
    );
}

#[test]
#[cfg(feature = "annotated-preview")]
fn annotated_preview_loads_all_four_representative_pages_end_to_end() {
    // Each exact compressed fixture was decoded and rendered with the pinned
    // CVS -Tutf8 reference before these assertions. This exercises the real
    // loader, native result, Fixed IR, presentation and CLI direct output.
    let fixtures = [
        "../../tests/fixtures/roff/real/archlinux/gcc.1.gz",
        "../../tests/fixtures/roff/real/archlinux/git.1.gz",
        "../../tests/fixtures/roff/real/archlinux/clang.1.gz",
        "../../tests/fixtures/roff/real/windows-releases/rclone.1.zst",
    ];
    let host = FakeHost::new();
    for fixture in fixtures {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture);
        let name = path.to_str().expect("UTF-8 fixture path");
        let args = [
            "--annotated-preview",
            "--input",
            name,
            "--input-format",
            "roff",
            "--format",
            "text",
            "--display",
            "direct",
            "--color",
            "never",
        ];
        let (status, output, diagnostics) = invoke(&args, b"", &host);
        assert_eq!(status, 0, "{fixture}: {diagnostics}");
        assert!(diagnostics.is_empty(), "{fixture}: {diagnostics}");
        assert!(output.len() > 1_000, "{fixture}: incomplete display");
    }
    assert_eq!(host.query_calls.get(), 0, "preview must not use old loader");
}

#[test]
#[cfg(all(feature = "annotated-preview", feature = "tui"))]
fn annotated_preview_four_pages_keep_one_surface_across_real_viewports() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    let fixtures = [
        "../../tests/fixtures/roff/real/archlinux/gcc.1.gz",
        "../../tests/fixtures/roff/real/archlinux/git.1.gz",
        "../../tests/fixtures/roff/real/archlinux/clang.1.gz",
        "../../tests/fixtures/roff/real/windows-releases/rclone.1.zst",
    ];
    for fixture in fixtures {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture);
        let query = crate::annotated_preview::load(path.to_str().expect("UTF-8 fixture path"))
            .unwrap_or_else(|error| panic!("{fixture}: {error:?}"));
        let document = query.document.as_ref().expect("annotated document");
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            panic!("{fixture}: wrong document body");
        };
        let source_rows = fixed.surface.rows.len();
        let source_bytes = fixed.surface.text.len();
        assert!(source_rows > 100, "{fixture}: incomplete native surface");
        assert!(!fixed.headings.is_empty(), "{fixture}: missing navigation");
        let first_visible = fixed
            .surface
            .rows
            .iter()
            .position(|row| row.run_count != 0)
            .expect("nonempty native row");
        let view = mant_ui::DocumentView::new(&query);
        for width in [20_u16, 40, 78, 120] {
            let rendered = view.render(width);
            assert_eq!(
                rendered.row_count, source_rows,
                "{fixture} at width {width}"
            );
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, 6));
            Paragraph::new(rendered.text)
                .scroll((u16::try_from(first_visible).expect("first row fits"), 0))
                .render(buffer.area, &mut buffer);
            assert!(
                buffer.content.iter().any(|cell| cell.symbol() != " "),
                "{fixture} at width {width}: blank viewport"
            );
        }
        assert_eq!(
            fixed.surface.text.len(),
            source_bytes,
            "viewport changed {fixture}"
        );
        eprintln!(
            "G1 {fixture}: rows={source_rows} bytes={source_bytes} runs={} headings={} owners={} links={} anchors={} nav={}",
            fixed.surface.runs.len(),
            fixed.headings.len(),
            fixed.owners.len(),
            fixed.links.len(),
            fixed.anchors.len(),
            view.navigation().len()
        );
    }
}

#[test]
#[ignore = "manual G1 complete four-page, four-width Ratatui cell comparison"]
#[cfg(all(feature = "annotated-preview", feature = "tui"))]
#[allow(clippy::too_many_lines)] // One bounded full-surface audit keeps all four fixture widths together.
fn annotated_preview_full_body_matches_real_tui_buffer_at_four_widths() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        text::Line,
        widgets::{Paragraph, Widget},
    };

    // All four exact decoded inputs were run through the fixed CVS reference
    // before these P1 assertions. This checks the consumer of the same final
    // Fixed result, not a separate formatter run per viewport.
    for fixture in [
        "../../tests/fixtures/roff/real/archlinux/gcc.1.gz",
        "../../tests/fixtures/roff/real/archlinux/git.1.gz",
        "../../tests/fixtures/roff/real/archlinux/clang.1.gz",
        "../../tests/fixtures/roff/real/windows-releases/rclone.1.zst",
    ] {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture);
        let query = crate::annotated_preview::load(path.to_str().expect("UTF-8 fixture path"))
            .unwrap_or_else(|error| panic!("{fixture}: {error:?}"));
        let document = query.document.as_ref().expect("annotated document");
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            panic!("{fixture}: wrong body");
        };
        let native_rows = fixed.surface.rows.len();
        let text = mant_render::render_query_text(&query);
        let text_rows = text.split_terminator('\n').collect::<Vec<_>>();
        assert!(text_rows.len() >= 2 && text_rows[1].is_empty());
        let body_rows = &text_rows[2..];
        assert_eq!(body_rows.len(), native_rows, "{fixture}: text rows");
        let view = mant_ui::DocumentView::new(&query);
        for width in [20_u16, 40, 78, 120] {
            let rendered = view.render(width);
            assert_eq!(rendered.row_count, native_rows);
            assert_eq!(rendered.text.lines.len(), native_rows);
            let mut compared = 0;
            for start in (0..native_rows).step_by(128) {
                let end = (start + 128).min(native_rows);
                let height = u16::try_from(end - start).expect("bounded viewport");
                let area = Rect::new(0, 0, width, height);
                let mut expected = Buffer::empty(area);
                let mut actual = Buffer::empty(area);
                let source = body_rows[start..end]
                    .iter()
                    .map(|line| Line::raw(*line))
                    .collect::<Vec<_>>();
                Paragraph::new(source).render(area, &mut expected);
                Paragraph::new(rendered.text.lines[start..end].to_vec()).render(area, &mut actual);
                for (column, (left, right)) in
                    expected.content.iter().zip(&actual.content).enumerate()
                {
                    assert_eq!(
                        left.symbol(),
                        right.symbol(),
                        "{fixture}: width {width}, row {}, column {}",
                        start + column / usize::from(width),
                        column % usize::from(width)
                    );
                }
                compared += end - start;
            }
            assert_eq!(compared, native_rows, "{fixture}: incomplete Buffer probe");
            eprintln!("G1 full Buffer {fixture}: width={width} rows={compared} complete=true");
        }
    }
}

#[test]
#[ignore = "manual G1 release-stage timing on representative pages"]
#[cfg(feature = "annotated-preview")]
#[allow(clippy::too_many_lines)] // One repeated run reports comparable native, IR, query and render stages.
fn annotated_preview_four_page_stage_costs() {
    use std::time::Instant;

    use libmandoc_rs::{InputFormat, SourceBundle, annotated::AnnotatedRenderer};
    use mant_protocol::{
        ExplanationOptions, ExplanationQuery, OutlineDetail, SearchCase, SearchQuery, SearchScope,
        SearchSyntax,
    };

    // The exact four decoded sources were first run through the pinned CVS
    // reference. Timings are observational, not output expectations.
    let fixtures = [
        (
            "GCC",
            "../../tests/fixtures/roff/real/archlinux/gcc.1.gz",
            "-x",
        ),
        (
            "Git",
            "../../tests/fixtures/roff/real/archlinux/git.1.gz",
            "--help",
        ),
        (
            "Clang",
            "../../tests/fixtures/roff/real/archlinux/clang.1.gz",
            "-help",
        ),
        (
            "rclone",
            "../../tests/fixtures/roff/real/windows-releases/rclone.1.zst",
            "--help",
        ),
    ];
    for pass in 0..2 {
        for index in 0..fixtures.len() {
            let (name, fixture, term) = fixtures[if pass == 0 { index } else { 3 - index }];
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture);
            let started = Instant::now();
            let source = mant_loader::read_standalone_manual_bytes(&path).unwrap();
            let decode_us = started.elapsed().as_micros();
            let root = path.file_name().unwrap().to_str().unwrap();
            let mut bundle = SourceBundle::new();
            bundle.insert(root, source).unwrap();
            let started = Instant::now();
            let page = AnnotatedRenderer::default()
                .render_bundle(root, &bundle, InputFormat::Auto)
                .unwrap();
            let native_us = started.elapsed().as_micros();
            let started = Instant::now();
            let document = mant_codec::annotated_fixed::lower_annotated_document(page).unwrap();
            let lower_us = started.elapsed().as_micros();
            let query = ResolvedContent {
                label: name.into(),
                address: None,
                document: Some(document),
                tldr: None,
            };
            let started = Instant::now();
            let outline =
                mant_query::build_outline_with_detail(&query, OutlineDetail::Entries).unwrap();
            let outline_us = started.elapsed().as_micros();
            let started = Instant::now();
            let explanation = mant_query::explain_query(
                &query,
                &ExplanationQuery {
                    entry: term.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            let explain_us = started.elapsed().as_micros();
            let started = Instant::now();
            let search = mant_query::search_query(
                &query,
                &SearchQuery {
                    pattern: term.into(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    scope: SearchScope::Visible,
                    word: false,
                    context_lines: 0,
                    offset: 0,
                    limit: 10,
                },
            )
            .unwrap();
            let search_us = started.elapsed().as_micros();
            let started = Instant::now();
            let text = mant_render::render_query_text(&query);
            let render_us = started.elapsed().as_micros();
            assert!(!outline.nodes.is_empty() && !text.is_empty());
            eprintln!(
                "G1 stage pass={pass} page={name} decode_us={decode_us} native_us={native_us} lower_us={lower_us} outline_us={outline_us} explain_us={explain_us} search_us={search_us} text_render_us={render_us} explain_returned={} search_returned={}",
                explanation.returned, search.returned
            );
        }
    }
}

fn manual() -> Document {
    manual_with_option(false)
}

fn manual_with_option(explainable: bool) -> Document {
    let mut content = ContentStoreBuilder::new();
    let common = section(
        &mut content,
        "common-3",
        "Common options",
        "common details",
        Vec::new(),
    );
    let mut options = section(
        &mut content,
        "options-2",
        "OPTIONS",
        "all options",
        vec![common],
    );
    if explainable {
        options.blocks.push(explainable_option(&mut content));
    }
    let name = section(&mut content, "name-1", "NAME", "demo - a test", Vec::new());
    Document {
        parser: None,
        sources: vec![SourceRecord {
            key: SourceKey::FIRST,
            identity: SourceIdentity::Path {
                name: "/man/demo.1".to_owned(),
            },
            format: SourceFormat::Man,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: SourceKey::FIRST,
        body: mant_ir::DocumentBody::Flow(mant_ir::FlowBody {
            content_store: content.finish(),
            heading: None,
            blocks: Vec::new(),
            sections: vec![name, options],
        }),
        meta: DocumentMeta {
            manual_section: Some("1".to_owned()),
            ..DocumentMeta::default()
        },
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn explainable_manual() -> Document {
    manual_with_option(true)
}

fn explainable_option(content: &mut ContentStoreBuilder) -> Block {
    Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            source: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
            entry: Some(EntryFacts {
                name_bindings: vec![mant_ir::EntryNameBinding {
                    name: 0,
                    evidence: mant_ir::EntryNameEvidence::Declared,
                    occurrences: vec![mant_ir::EntryForm {
                        parts: vec![mant_ir::EntryContentSlice {
                            root: mant_ir::EntryInlineRoot::Term { index: 0 },
                            path: vec![0],
                            bytes: Some(0..9),
                        }],
                    }],
                }],
                alias_groups: Vec::new(),
                alias_of: None,
                forms: vec![mant_ir::EntryForm::term(0)],
                id: "exclude".to_owned().into(),
                kind: EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option,
                },
                case: NameCase::Sensitive,
                names: vec!["--exclude".to_owned()],
                value_domain: None,
            }),
            terms: vec![vec![test_text(
                content,
                ContentRootKind::Term,
                "--exclude=PATTERN",
            )]],
            description: vec![Block::Paragraph {
                children: vec![test_text(
                    content,
                    ContentRootKind::Body,
                    "Exclude matching files from the archive.",
                )],
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn semantic_markdown() -> Document {
    mant_codec::parse_markdown(
            "# Tool\n\n## Query\n\nGeneral query behavior.\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force a query.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Query registry data.\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/S COMPUTER`: Select a remote computer.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n",
            Some("semantic.md".to_owned()),
        )
        .expect("semantic Markdown fixture")
        .document
}

fn tldr() -> TldrDocument {
    TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A small demonstration.".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/cache/tldr/pages/common/demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    }
}

fn test_text(content: &mut ContentStoreBuilder, kind: ContentRootKind, value: &str) -> Inline {
    let owner = content.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
    let root = content.push_root(owner, kind, Provenance::Unknown);
    Inline::Text {
        content: content.push_text(
            root,
            value.to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        ),
    }
}

fn section(
    content: &mut ContentStoreBuilder,
    id: &str,
    title: &str,
    text: &str,
    children: Vec<Section>,
) -> Section {
    Section {
        id: id.to_owned().into(),
        fragment_aliases: Vec::new(),
        heading: Heading {
            content: vec![test_text(content, ContentRootKind::Heading, title)],
            source: None,
        },
        spacing_before_lines: 0,
        blocks: vec![Block::Paragraph {
            children: vec![test_text(content, ContentRootKind::Body, text)],
            layout: LayoutHint::default(),
            source: None,
        }],
        children,
        source: None,
    }
}

#[test]
fn stdin_protocol_emits_only_compact_query_json() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git","manualSection":"1"},"view":{"kind":"full"}}"#,
            &host,
        );

    assert_eq!(status, 0);
    assert_eq!(
        output,
        "{\"schema\":\"mant.query/v0.12\",\"label\":\"git\"}\n"
    );
    assert!(diagnostics.is_empty());
    assert_eq!(host.query_calls.get(), 1);
}

#[test]
fn malformed_or_extended_requests_fail_before_querying_the_host() {
    for input in [
            br"not-json".as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"full"},"futureField":true}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"   "},"view":{"kind":"full"}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","nodes":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","selectors":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","selectors":["1","2","3","4","5","6","7","8","9","10","11","12","13","14","15","16","17"]}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"outline","root":"   "}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"outline","entries":{"kind":"kinds","kinds":[]}}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"","limit":10}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"git","limit":0}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"git","contextLines":101}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"[","syntax":"regex"}}"#.as_slice(),
        ] {
            let host = FakeHost::new();
            let (status, output, diagnostics) = invoke(
                &["--request-json", "--format", "json", "--compact"],
                input,
                &host,
            );
            assert_eq!(status, 2);
            assert!(output.is_empty());
            assert!(diagnostics.starts_with("mant: "));
            assert_eq!(host.query_calls.get(), 0);
        }
}

#[test]
fn stdin_requests_select_outline_and_excerpt_projections() {
    let host = FakeHost::with_manual_and_tldr();
    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"demo"},"view":{"kind":"outline","entries":{"kind":"none"}}}"#,
            &host,
        );
    assert_eq!(status, 0);
    let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
    assert_eq!(outline["schema"], "mant.outline/v0.12");
    assert_eq!(outline["entries"]["kind"], "none");
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"demo"},"view":{"kind":"excerpt","selectors":[{"kind":"path","path":"2.1"}]}}"#,
            &host,
        );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(excerpt["schema"], "mant.excerpt/v0.12");
    assert_eq!(excerpt["selections"][0]["outline"]["node"]["path"], "2.1");
    assert!(diagnostics.is_empty());
    assert_eq!(host.query_calls.get(), 2);
}

#[test]
fn direct_queries_render_outlines_and_selected_nodes_in_requested_formats() {
    let host = FakeHost::with_manual_and_tldr();
    let (status, output, diagnostics) = invoke(&["demo", "--outline"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("├─ 0 TLDR QUICK REFERENCE\n│    ID: tldr"));
    assert!(output.contains("├─ 1 NAME\n│    ID: name-1"));
    assert!(output.contains("└─ 2 OPTIONS\n     ID: options-2"));
    assert!(output.contains("└─ 2.1 Common options\n       ID: common-3"));
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
        &["demo", "--node", "2.1", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.excerpt/v0.12");
    assert_eq!(value["selections"][0]["outline"]["node"]["path"], "2.1");
    let excerpt: mant_protocol::QueryExcerpt = serde_json::from_value(value).unwrap();
    let mant_protocol::ExcerptSelection::DocumentSection { section, .. } = &excerpt.selections[0]
    else {
        panic!("section selection");
    };
    assert_eq!(
        section
            .heading
            .plain_text(excerpt.content_projection.as_ref().unwrap().content()),
        "Common options"
    );
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
        &["demo", "--node", "0", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("tldr excerpt JSON");
    assert_eq!(value["selections"][0]["kind"], "tldr");
    assert_eq!(value["selections"][0]["outline"]["node"]["path"], "0");
    assert_eq!(value["selections"][0]["document"]["title"], "demo");
    assert!(value.get("producer").is_none());
    assert!(value.get("diagnostics").is_none());
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(&["demo", "--tldr"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("A small demonstration."));
    assert!(!output.contains("## NAME"));
    assert!(diagnostics.is_empty());
}

#[test]
fn markdown_is_clean_by_default_and_preserves_anchors_on_request() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--format", "markdown"], b"", &host);
    assert_eq!(status, 0);
    assert!(!output.contains("<a "));
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(&["demo", "--preserve-anchors"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("<a id=\"name-1\"></a>"));
    assert!(output.contains("<a id=\"options-2\"></a>"));
    assert!(diagnostics.is_empty());
}

#[test]
fn man_format_rejects_a_tldr_only_result() {
    let host = FakeHost::with_tldr();
    let (status, output, diagnostics) = invoke(&["demo", "--format", "man"], b"", &host);

    assert_eq!(status, 1);
    assert!(output.is_empty());
    assert_eq!(
        diagnostics,
        "mant: manual page is unavailable; --format man cannot render tldr-only content\n"
    );
}

#[test]
fn explains_semantic_evidence_without_turning_sections_into_entries() {
    let host = FakeHost::with_explainable_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--explain", "--exclude"], b"", &host);

    assert_eq!(status, 0);
    assert!(
        output.contains("2/e1 [exclude]\nOPTIONS > --exclude\nKind: option\nMatched by:"),
        "{output}"
    );
    assert!(output.contains("--exclude=PATTERN"));
    assert!(output.contains("Exclude matching files from the archive."));
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--explain=--exclude",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.explanation/v0.12");
    assert_eq!(value["outcome"], "evidence");
    assert_eq!(value["evidence"][0]["outline"]["node"]["id"], "exclude");
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(&["demo", "--explain=2"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("no-evidence"), "{output}");
    assert!(diagnostics.is_empty());
}

#[test]
fn semantic_entries_work_through_cli_and_request_json() {
    let host = FakeHost::with_semantic_markdown();
    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
    assert_eq!(outline["schema"], "mant.outline/v0.12");
    let encoded = outline.to_string();
    assert!(encoded.contains("\"parameterKind\":\"option\""));
    assert!(encoded.contains("\"kind\":\"command\""));
    assert!(encoded.contains("\"kind\":\"environment-variable\""));
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--outline",
            "--outline-entries",
            "option",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let outline: serde_json::Value = serde_json::from_str(&output).expect("alias outline");
    assert_eq!(outline["entries"]["kind"], "kinds");
    assert!(diagnostics.is_empty());
}

#[test]
fn semantic_entry_selectors_work_through_cli_and_request_json() {
    let host = FakeHost::with_semantic_markdown();
    let (status, output, diagnostics) = invoke(
        &["demo", "--explain=query", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let evidence: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(evidence["evidence"].as_array().unwrap().iter().any(|item| {
        item["entry"]["kind"]["kind"] == "command"
            && item["bases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|basis| basis["kind"] == "name")
    }));
    assert!(diagnostics.is_empty());

    let (status, output, _) = invoke(
        &[
            "demo",
            "--explain=command-query",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(excerpt["evidence"][0]["entry"]["kind"]["kind"], "command");

    let (status, output, _) = invoke(
        &["demo", "--node=id:query", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("section excerpt");
    assert_eq!(excerpt["selections"][0]["kind"], "document-section");
    assert_eq!(excerpt["selections"][0]["outline"]["node"]["id"], "query");

    for (selector, role) in [("/s", "option"), ("$ENV:PATH", "environment-variable")] {
        let argument = format!("--explain={selector}");
        let (status, output, diagnostics) = invoke(
            &["demo", &argument, "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("role explanation");
        let kind = &excerpt["evidence"][0]["entry"]["kind"];
        if role == "option" {
            assert_eq!(kind["kind"], "parameter");
            assert_eq!(kind["parameterKind"], "option");
        } else {
            assert_eq!(kind["kind"], role);
        }
        assert!(diagnostics.is_empty());
    }

    let (status, output, diagnostics) = invoke(
        &["--request-json", "--format", "json", "--compact"],
        br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"demo"},"view":{"kind":"explain","entry":"command-query"}}"#,
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("request excerpt");
    assert_eq!(excerpt["evidence"][0]["entry"]["kind"]["kind"], "command");
    assert!(diagnostics.is_empty());
}

#[test]
fn ambiguous_semantic_entries_remain_addressable_by_returned_id() {
    let host = FakeHost::with_semantic_markdown();
    let (status, output, diagnostics) = invoke(&["demo", "--explain=/f"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("option-f"));
    assert!(output.contains("owners=2"), "{output}");
    assert!(diagnostics.is_empty());
    let multiple = output;

    let (status, outline, outline_diagnostics) = invoke(
        &[
            "demo",
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    assert!(outline_diagnostics.is_empty());
    let outline: serde_json::Value = serde_json::from_str(&outline).expect("outline JSON");
    let qualified_id = outline["nodes"][5]["children"][0]["id"]
        .as_str()
        .expect("returned semantic ID");
    assert!(qualified_id.starts_with("option-f-"));
    assert!(multiple.contains(qualified_id));

    let qualified_argument = format!("--explain={qualified_id}");
    let (status, output, diagnostics) = invoke(
        &["demo", &qualified_argument, "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("qualified entry");
    assert_eq!(
        excerpt["evidence"][0]["outline"]["node"]["id"],
        qualified_id
    );
    assert!(diagnostics.is_empty());
}

#[test]
#[cfg(feature = "roff")]
fn manual_option_reaches_the_resolution_policy_without_stderr_noise() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--outline", "--manual"], b"", &host);

    assert_eq!(status, 0);
    assert!(output.contains("1 NAME\n│    ID: name-1"));
    assert!(diagnostics.is_empty());
    assert_eq!(host.last_policy.get(), LoadPolicy::ManualOnly);
}

#[test]
fn searches_report_markdown_coordinates_and_reusable_outline_nodes() {
    let host = FakeHost::with_manual_and_tldr();
    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--search",
            "common details",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );

    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
    assert_eq!(value["schema"], "mant.search/v0.12");
    assert_eq!(value["total"], 1);
    assert_eq!(value["matches"][0]["outline"]["node"]["path"], "2.1");
    assert_eq!(value["matches"][0]["outline"]["node"]["id"], "common-3");
    assert_eq!(value["matches"][0]["location"]["kind"], "visible-flow");
    assert!(value["matches"][0]["location"]["unit"].as_u64() > Some(0));
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(&["demo", "--grep", "missing"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(output, "No matches for \"missing\" in demo(1).\n");
    assert!(diagnostics.is_empty());
}

#[test]
fn stdin_search_requests_use_the_same_projection_contract() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"demo"},"view":{"kind":"search","pattern":"options","limit":10}}"#,
            &host,
        );

    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
    assert_eq!(value["schema"], "mant.search/v0.12");
    assert_eq!(value["query"]["syntax"], "literal");
    assert_eq!(value["query"]["scope"], "visible");
    assert!(
        value["matches"]
            .as_array()
            .is_some_and(|matches| !matches.is_empty())
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn catalog_colour_preserves_records_and_newlines() {
    let host = FakeHost::new();
    let catalog = host
        .discover(&CatalogQuery::default())
        .expect("fake catalog");
    for grouped in [false, true] {
        let plain = super::presentation::render_catalog_output(&catalog, grouped, false);
        let colored = super::presentation::render_catalog_output(&catalog, grouped, true);
        assert!(colored.contains('\x1b'));
        // Catalog uses only SGR styling; stripping it must retain tabs and final newlines.
        let mut visible = String::new();
        let mut chars = colored.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for ch in chars.by_ref() {
                    if ch == 'm' {
                        break;
                    }
                }
            } else {
                visible.push(ch);
            }
        }
        assert_eq!(visible, plain);
    }
}

#[test]
fn unknown_nodes_are_concise_usage_failures() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) =
        invoke(&["demo", "--node", "9", "--format", "text"], b"", &host);

    assert_eq!(status, 2);
    assert!(output.is_empty());
    assert!(diagnostics.contains("document 'demo' has no outline node 'path:9'"));
    assert!(diagnostics.contains("mant demo --outline --outline-entries all --format json"));
}

#[test]
fn explain_reports_ordinary_support_without_inventing_a_definition() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--explain=details"], b"", &host);

    assert_eq!(status, 0);
    assert!(output.contains("2.1"), "{output}");
    assert!(output.contains("Common options"), "{output}");
    assert!(output.contains("Matched by: text mention"), "{output}");
    assert!(diagnostics.is_empty());
}

#[test]
#[cfg(feature = "update")]
fn update_results_are_stable_json_documents() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--update-tldr", "--compact"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(
        output,
        "{\"schema\":\"mant.tldr-update/v1\",\"action\":\"updated\",\"cacheDir\":\"/cache/tldr\",\"revision\":\"abc123\"}\n"
    );
    assert!(diagnostics.is_empty());
    assert_eq!(host.update_calls.get(), 1);

    let (status, output, diagnostics) =
        invoke(&["--prune-docs", "--dry-run", "--compact"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(
        output,
        "{\"schema\":\"mant.sources-prune/v1\",\"config\":\"/data/mant/sources.toml\",\"dryRun\":true,\"sources\":[]}\n"
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn protocol_results_are_stable_json_documents_without_optional_capabilities() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--protocol-version", "--compact"], b"", &host);
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("protocol JSON");
    assert_eq!(value["protocol"], CLI_PROTOCOL_VERSION);
    assert_eq!(value["nativeApiVersion"], "0.12");
    assert_eq!(value["requestSchema"], "mant.request/v0.12");
    assert_eq!(value["outlineSchema"], "mant.outline/v0.12");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v0.12");
    assert_eq!(value["searchSchema"], "mant.search/v0.12");
    assert_eq!(value["catalogSchema"], "mant.catalog/v0.12");
    assert!(diagnostics.is_empty());
}

#[test]
fn doctor_supports_copy_friendly_text_and_stable_json_exit_statuses() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--doctor"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.starts_with("ManT doctor\n\n[ok] runtime.fixture"));
    assert!(output.ends_with("1 ok, 0 info, 0 warning(s), 0 error(s)\n"));
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
        &["--doctor", "--format", "json", "--compact"],
        b"",
        &FakeHost::with_doctor_error(),
    );
    assert_eq!(status, 1);
    let value: serde_json::Value = serde_json::from_str(&output).expect("doctor JSON");
    assert_eq!(value["schema"], "mant.doctor/v1");
    assert_eq!(value["outcome"], "error");
    assert_eq!(value["summary"]["errors"], 1);
    assert!(diagnostics.is_empty());
}

#[test]
fn catalog_lists_grouped_documents_and_emits_flat_find_records() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--list"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(output, "manual/3\n  printf\n\nsources/team\n  guide\n");
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(&["--find", "guide"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(
        output,
        "sources/team/guide\tmarkdown\nmanual/3/printf\tmanual\n"
    );
    assert!(diagnostics.is_empty());

    let (status, output, diagnostics) = invoke(
        &["--find", "guide", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("catalog JSON");
    assert_eq!(value["schema"], "mant.catalog/v0.12");
    assert_eq!(value["documents"][0]["address"]["path"], "guide");
    assert!(value["documents"][0].get("sourcePath").is_none());
    assert!(diagnostics.is_empty());
}

#[test]
fn usage_errors_are_concise_and_never_trigger_side_effects() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--unknown"], b"", &host);
    assert_eq!(status, 2);
    assert!(output.is_empty());
    assert!(diagnostics.starts_with("error: unexpected argument '--unknown'"));
    assert!(diagnostics.contains("Usage: mant"));
    assert!(diagnostics.contains("For more information, try '--help'."));
    assert_eq!(host.query_calls.get(), 0);
    assert_eq!(host.update_calls.get(), 0);
}

#[test]
fn generated_schemas_are_json_only_and_side_effect_free() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--schema", "request", "--compact"], b"", &host);

    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("request schema");
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(value["additionalProperties"], false);
    assert!(output.contains("mant.request/v0.12"));
    assert!(diagnostics.is_empty());
    assert_eq!(host.query_calls.get(), 0);
    assert_eq!(host.update_calls.get(), 0);

    let (status, output, diagnostics) = invoke(&["--schema", "all"], b"", &host);
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("schema catalog");
    assert!(value["request"].is_object());
    assert!(value["query"].is_object());
    assert!(value["outline"].is_object());
    assert!(value["excerpt"].is_object());
    assert!(value["search"].is_object());
    assert!(value["doctor"].is_object());
    assert!(value["tldr-update"].is_object());
    assert!(diagnostics.is_empty());
    assert_eq!(host.query_calls.get(), 0);
    assert_eq!(host.update_calls.get(), 0);
}

#[test]
fn deep_generated_responses_reach_the_top_level_schema_diagnostic() {
    let input = format!(
        "{{\"schema\":\"mant.query/v0.12\",\"result\":{}}}",
        "[".repeat(140) + "0" + &"]".repeat(140)
    );
    let Err(error) = read_native_request(&mut input.as_bytes()) else {
        panic!("a response schema must not be accepted as a request");
    };

    assert!(
        error
            .into_message()
            .contains("unsupported request schema 'mant.query/v0.12'"),
    );
}
