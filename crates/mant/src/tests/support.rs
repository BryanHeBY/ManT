//! Shared host and document fixtures for process-boundary tests.

use super::*;

pub(super) struct FakeHost {
    pub(super) query_calls: Cell<usize>,
    pub(super) update_calls: Cell<usize>,
    pub(super) last_policy: Cell<LoadPolicy>,
    pub(super) document: Option<Document>,
    pub(super) tldr: Option<TldrDocument>,
    pub(super) doctor_error: bool,
}

impl FakeHost {
    pub(super) fn new() -> Self {
        Self {
            query_calls: Cell::new(0),
            update_calls: Cell::new(0),
            last_policy: Cell::new(LoadPolicy::default()),
            document: None,
            tldr: None,
            doctor_error: false,
        }
    }

    pub(super) fn with_doctor_error() -> Self {
        Self {
            doctor_error: true,
            ..Self::new()
        }
    }

    pub(super) fn with_manual() -> Self {
        Self {
            document: Some(manual()),
            ..Self::new()
        }
    }

    pub(super) fn with_manual_and_tldr() -> Self {
        Self {
            document: Some(manual()),
            tldr: Some(tldr()),
            ..Self::new()
        }
    }

    pub(super) fn with_tldr() -> Self {
        Self {
            tldr: Some(tldr()),
            ..Self::new()
        }
    }

    pub(super) fn with_explainable_manual() -> Self {
        Self {
            document: Some(explainable_manual()),
            ..Self::new()
        }
    }

    pub(super) fn with_semantic_markdown() -> Self {
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
            schema: CatalogSchema::V0Dot14,
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

pub(super) fn invoke(arguments: &[&str], input: &[u8], host: &FakeHost) -> (u8, String, String) {
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

pub(super) fn invoke_with_terminal_output(
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

pub(super) fn manual() -> Document {
    Document {
        heading: None,
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: Some("/man/demo.1".to_owned()),
        },
        meta: DocumentMeta {
            manual_section: Some("1".to_owned()),
            ..DocumentMeta::default()
        },
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections: vec![
            section("name-1", "NAME", "demo - a test", Vec::new()),
            section(
                "options-2",
                "OPTIONS",
                "all options",
                vec![section(
                    "common-3",
                    "Common options",
                    "common details",
                    Vec::new(),
                )],
            ),
        ],
    }
}

pub(super) fn explainable_manual() -> Document {
    let mut manual = manual();
    let options = manual
        .sections
        .iter_mut()
        .find(|section| section.id == "options-2")
        .expect("options section");
    options.blocks.push(Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::from(false),
            source: None,
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
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
            terms: (vec![vec![Inline::Text {
                value: "--exclude=PATTERN".to_owned(),
            }]])
            .into_iter()
            .map(Into::into)
            .collect(),
            description: vec![Block::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![Inline::Text {
                    value: "Exclude matching files from the archive.".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    });
    manual
}

pub(super) fn semantic_markdown() -> Document {
    mant_codec::parse_markdown(
            "# Tool\n\n## Query\n\nGeneral query behavior.\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force a query.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Query registry data.\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/S COMPUTER`: Select a remote computer.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n",
            Some("semantic.md".to_owned()),
        )
        .expect("semantic Markdown fixture")
        .document
}

pub(super) fn tldr() -> TldrDocument {
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

pub(super) fn section(id: &str, title: &str, text: &str, children: Vec<Section>) -> Section {
    Section {
        id: id.to_owned().into(),
        fragment_aliases: Vec::new(),
        heading: title.into(),
        spacing_before_lines: 0,
        blocks: vec![Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children: vec![Inline::Text {
                value: text.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        children,
        source: None,
    }
}
