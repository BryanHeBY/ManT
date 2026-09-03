//! Converts clap's syntax model into `ManT`'s validated command model.

use super::{
    CatalogKindMode, CatalogPaging, CatalogQuery, Cli, ColorMode, Command, CommandFactory,
    DocumentScope, DocumentSelector, DocumentTraversal, EntryProjection, ErrorKind, InputFormat,
    InputFormatMode, NodeSelector, QueryFormat, QueryInput, QueryPolicy, QueryPresentation,
    QueryRequest, QuerySource, QueryView, RequestSchema, ScopeQueryView, SearchCase, SearchScope,
    SearchSyntax, default_search_limit, is_manual_section, normalize_tldr_topic,
    parenthesized_manual_reference,
};

pub(super) fn normalize(mut parsed: Cli, color: ColorMode) -> Result<Command, clap::Error> {
    validate_scope_mode(&parsed, color)?;
    if parsed.no_pager && !parsed.list && parsed.find.is_none() {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--no-pager applies only to --list and --find",
            color,
        ));
    }
    if parsed.mcp {
        return Ok(Command::Mcp);
    }
    if parsed.doctor {
        return normalize_doctor(&parsed, color);
    }
    if parsed.update_docs {
        return Ok(Command::UpdateDocs {
            pretty: !parsed.compact,
        });
    }
    if parsed.prune_docs {
        return Ok(Command::PruneDocs {
            pretty: !parsed.compact,
            dry_run: parsed.dry_run,
        });
    }
    if parsed.update_tldr {
        return Ok(Command::UpdateTldr {
            pretty: !parsed.compact,
        });
    }
    if parsed.protocol_version {
        return Ok(Command::ProtocolVersion {
            pretty: !parsed.compact,
        });
    }
    if let Some(contract) = parsed.schema {
        return Ok(Command::Schema {
            contract,
            pretty: !parsed.compact,
        });
    }
    if parsed.list || parsed.find.is_some() {
        return normalize_catalog(parsed, color);
    }
    validate_query_search_options(&parsed, color)?;

    let view = normalize_query_view(&mut parsed);
    validate_output_options(
        parsed.compact,
        parsed.format,
        parsed.preserve_anchors,
        &view,
        color,
    )?;
    let source = normalize_query_source(
        QuerySourceOptions {
            request_json: parsed.request_json,
            selectors: parsed.selector,
            documents: parsed.document,
            follow_links: parsed.follow_links,
            max_depth: parsed.max_depth,
            max_documents: parsed.max_documents,
            input_path: parsed.input,
            input_format: parsed.input_format,
            configured_source: parsed.source,
            manual_section: parsed.man_section,
            tldr: parsed.tldr,
        },
        view,
        color,
    )?;
    validate_manual_source(parsed.manual, &source, color)?;
    let presentation = normalize_presentation(
        parsed.ui,
        parsed.format,
        parsed.preserve_anchors,
        &source,
        parsed.tldr,
        parsed.color,
    );

    Ok(Command::Query {
        source,
        presentation,
        pretty: !parsed.compact,
        policy: if parsed.manual {
            QueryPolicy::ManualOnly
        } else if parsed.tldr {
            QueryPolicy::TldrOnly
        } else {
            QueryPolicy::Combined
        },
        preserve_anchors: parsed.preserve_anchors,
    })
}

fn validate_scope_mode(parsed: &Cli, color: ColorMode) -> Result<(), clap::Error> {
    let configured =
        parsed.follow_links || parsed.max_depth.is_some() || parsed.max_documents.is_some();
    if configured
        && (parsed.list
            || parsed.find.is_some()
            || parsed.request_json
            || parsed.input.is_some()
            || parsed.doctor
            || parsed.update_docs
            || parsed.prune_docs
            || parsed.update_tldr
            || parsed.protocol_version
            || parsed.schema.is_some()
            || parsed.mcp)
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "document-scope options apply only to registered document queries",
            color,
        ));
    }
    Ok(())
}

fn normalize_doctor(parsed: &Cli, color: ColorMode) -> Result<Command, clap::Error> {
    let format = parsed.format.unwrap_or(QueryFormat::Text);
    if !matches!(format, QueryFormat::Text | QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::InvalidValue,
            "doctor supports only text and json formats",
            color,
        ));
    }
    if parsed.compact && format != QueryFormat::Json {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json",
            color,
        ));
    }
    Ok(Command::Doctor {
        format,
        pretty: !parsed.compact,
        color: parsed.color.unwrap_or_default(),
    })
}

fn normalize_catalog(parsed: Cli, color: ColorMode) -> Result<Command, clap::Error> {
    if parsed.list && (parsed.regex || parsed.search_case.is_some()) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--regex and --case require --find",
            color,
        ));
    }
    if parsed.word || parsed.search_scope.is_some() || parsed.context.is_some() {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--word, --scope, and --context apply only to document-content search",
            color,
        ));
    }
    if parsed.preserve_anchors {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors does not apply to document discovery",
            color,
        ));
    }
    if parsed.source.is_some() && parsed.kind == Some(CatalogKindMode::Manual)
        || parsed.man_section.is_some() && parsed.kind == Some(CatalogKindMode::Markdown)
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--source selects Markdown while --man-section selects native manuals",
            color,
        ));
    }
    let format = parsed.format.unwrap_or(QueryFormat::Text);
    if !matches!(format, QueryFormat::Text | QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::InvalidValue,
            "document discovery supports only text and json formats",
            color,
        ));
    }
    if parsed.compact && format != QueryFormat::Json {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json",
            color,
        ));
    }
    Ok(Command::Catalog {
        query: CatalogQuery {
            pattern: parsed.find,
            syntax: if parsed.regex {
                SearchSyntax::Regex
            } else {
                SearchSyntax::Literal
            },
            case: parsed
                .search_case
                .map_or(SearchCase::Insensitive, Into::into),
            kind: parsed.kind.map(Into::into),
            source: parsed.source,
            manual_section: parsed.man_section,
            limit: parsed.limit.unwrap_or(10_000),
            offset: parsed.offset.unwrap_or(0),
        },
        grouped: parsed.list,
        format,
        pretty: !parsed.compact,
        paging: if parsed.no_pager {
            CatalogPaging::Disabled
        } else {
            CatalogPaging::Auto
        },
    })
}

fn validate_query_search_options(parsed: &Cli, color: ColorMode) -> Result<(), clap::Error> {
    if parsed.search.is_none()
        && (parsed.regex
            || parsed.search_case.is_some()
            || parsed.limit.is_some()
            || parsed.offset.is_some())
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--regex, --case, --limit, and --offset require --search or --find",
            color,
        ));
    }
    if parsed.kind.is_some() {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--kind requires --list or --find",
            color,
        ));
    }
    if (parsed.follow_links || parsed.max_depth.is_some() || parsed.max_documents.is_some())
        && parsed.selector.is_empty()
        && parsed.document.is_empty()
    {
        return Err(command_error(
            ErrorKind::MissingRequiredArgument,
            "--follow-links requires a document selector or --document",
            color,
        ));
    }
    if (!parsed.document.is_empty() || parsed.follow_links) && (parsed.manual || parsed.tldr) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--manual and --tldr accept one document; document scopes resolve typed registered links",
            color,
        ));
    }
    Ok(())
}

fn normalize_query_view(parsed: &mut Cli) -> QueryView {
    if parsed.tldr {
        QueryView::Excerpt {
            selectors: vec![NodeSelector::from("tldr")],
        }
    } else if parsed.outline {
        QueryView::Outline {
            entries: parsed
                .outline_entries
                .take()
                .map_or(EntryProjection::Summary, |entries| entries.0),
            root: parsed.outline_root.take().map(NodeSelector::new),
        }
    } else if let Some(pattern) = parsed.search.take() {
        QueryView::Search {
            pattern,
            syntax: if parsed.regex {
                SearchSyntax::Regex
            } else {
                SearchSyntax::Literal
            },
            case: parsed
                .search_case
                .take()
                .map_or(SearchCase::Insensitive, Into::into),
            scope: parsed
                .search_scope
                .take()
                .map_or(SearchScope::Visible, Into::into),
            word: parsed.word,
            context_lines: parsed.context.take().unwrap_or(0),
            limit: parsed.limit.take().unwrap_or_else(default_search_limit),
            offset: parsed.offset.take().unwrap_or(0),
        }
    } else if let Some(selector) = parsed.explain.take() {
        QueryView::Explain { entry: selector }
    } else if parsed.node.is_empty() {
        QueryView::Full {}
    } else {
        QueryView::Excerpt {
            selectors: std::mem::take(&mut parsed.node)
                .into_iter()
                .map(NodeSelector::from)
                .collect(),
        }
    }
}

fn validate_output_options(
    compact: bool,
    format: Option<QueryFormat>,
    preserve_anchors: bool,
    view: &QueryView,
    color: ColorMode,
) -> Result<(), clap::Error> {
    if compact && format != Some(QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json for manual queries",
            color,
        ));
    }
    if preserve_anchors && format.is_some_and(|format| format != QueryFormat::Markdown) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors requires Markdown output",
            color,
        ));
    }
    if preserve_anchors && matches!(view, QueryView::Outline { .. } | QueryView::Search { .. }) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors applies only to full documents and excerpts",
            color,
        ));
    }
    if format == Some(QueryFormat::Man) && !matches!(view, QueryView::Full {}) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--format man applies only to full documents",
            color,
        ));
    }
    Ok(())
}

fn validate_manual_source(
    manual: bool,
    source: &QuerySource,
    color: ColorMode,
) -> Result<(), clap::Error> {
    if manual
        && !matches!(
            source,
            QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document { .. },
                ..
            })
        )
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--manual requires a document name rather than Markdown input",
            color,
        ));
    }
    Ok(())
}

fn normalize_presentation(
    ui: bool,
    format: Option<QueryFormat>,
    preserve_anchors: bool,
    source: &QuerySource,
    tldr: bool,
    color: Option<ColorMode>,
) -> QueryPresentation {
    let view = match source {
        QuerySource::Arguments(request) => Some(&request.view),
        QuerySource::ScopeArguments { view, .. } => {
            return if ui {
                QueryPresentation::Interactive
            } else if let Some(format) = format {
                QueryPresentation::Output {
                    format,
                    color: color.unwrap_or_default(),
                }
            } else if view.is_none() {
                QueryPresentation::Auto
            } else {
                QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: color.unwrap_or_default(),
                }
            };
        }
        QuerySource::InputStdin { view, .. } => Some(view),
        QuerySource::StdinJson => None,
    };
    let default_format = view.map_or(QueryFormat::Markdown, |view| {
        if matches!(view, QueryView::Full {}) {
            QueryFormat::Markdown
        } else {
            QueryFormat::Text
        }
    });
    if ui {
        QueryPresentation::Interactive
    } else if let Some(format) = format {
        QueryPresentation::Output {
            format,
            color: color.unwrap_or_default(),
        }
    } else if tldr {
        QueryPresentation::Tldr(color.unwrap_or_default())
    } else if preserve_anchors {
        QueryPresentation::Output {
            format: QueryFormat::Markdown,
            color: color.unwrap_or_default(),
        }
    } else if matches!(
        source,
        QuerySource::Arguments(QueryRequest {
            view: QueryView::Full {},
            ..
        })
    ) {
        QueryPresentation::Auto
    } else {
        QueryPresentation::Output {
            format: default_format,
            color: color.unwrap_or_default(),
        }
    }
}

struct QuerySourceOptions {
    request_json: bool,
    selectors: Vec<String>,
    documents: Vec<String>,
    follow_links: bool,
    max_depth: Option<u16>,
    max_documents: Option<u32>,
    input_path: Option<String>,
    input_format: Option<InputFormatMode>,
    configured_source: Option<String>,
    manual_section: Option<String>,
    tldr: bool,
}

struct NormalizedDocumentSelector {
    name: String,
    manual_section: Option<String>,
}

fn normalize_query_source(
    options: QuerySourceOptions,
    view: QueryView,
    color: ColorMode,
) -> Result<QuerySource, clap::Error> {
    if let Some(source) = normalize_scope_query_source(&options, view.clone(), color)? {
        return Ok(source);
    }
    let source = if options.request_json {
        QuerySource::StdinJson
    } else if let Some(path) = options.input_path {
        let format = options.input_format.map_or(InputFormat::Auto, Into::into);
        if path == "-" {
            if format == InputFormat::Auto {
                return Err(command_error(
                    ErrorKind::MissingRequiredArgument,
                    "--input - requires --input-format markdown or roff",
                    color,
                ));
            }
            QuerySource::InputStdin { format, view }
        } else {
            QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot11,
                input: QueryInput::File { path, format },
                view,
            })
        }
    } else {
        let normalized = normalize_document_operands(
            &options.selectors,
            options.manual_section,
            options.tldr,
            color,
        )?;
        QuerySource::Arguments(QueryRequest {
            schema: RequestSchema::V0Dot11,
            input: QueryInput::Document {
                selector: normalized.name,
                source: options.configured_source,
                manual_section: normalized.manual_section,
            },
            view,
        })
    };
    Ok(source)
}

fn normalize_scope_query_source(
    options: &QuerySourceOptions,
    view: QueryView,
    color: ColorMode,
) -> Result<Option<QuerySource>, clap::Error> {
    let scope_requested = !options.documents.is_empty()
        || options.follow_links
        || options.max_depth.is_some()
        || options.max_documents.is_some();
    if !scope_requested {
        return Ok(None);
    }
    if scope_requested && (options.request_json || options.input_path.is_some()) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "document scopes cannot be combined with --request-json or --input",
            color,
        ));
    }
    let operands = if options.documents.is_empty() {
        vec![normalize_document_operands(
            &options.selectors,
            options.manual_section.clone(),
            false,
            color,
        )?]
    } else {
        options
            .documents
            .iter()
            .map(|selector| {
                normalize_document_operands(
                    std::slice::from_ref(selector),
                    options.manual_section.clone(),
                    false,
                    color,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let documents = operands
        .into_iter()
        .map(|normalized| DocumentSelector {
            selector: normalized.name,
            source: options.configured_source.clone(),
            manual_section: normalized.manual_section,
        })
        .collect::<Vec<_>>();
    let view = match view {
        QueryView::Full {} => None,
        QueryView::Explain { entry } => Some(ScopeQueryView::Explain { entry }),
        QueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        } => Some(ScopeQueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        }),
        QueryView::Outline { .. } | QueryView::Excerpt { .. } => {
            return Err(command_error(
                ErrorKind::ArgumentConflict,
                "multi-document scopes support interactive reading, --search, and --explain",
                color,
            ));
        }
    };
    Ok(Some(QuerySource::ScopeArguments {
        scope: DocumentScope {
            documents,
            traversal: DocumentTraversal {
                follow_links: options.follow_links,
                max_depth: options.max_depth,
                max_documents: options.max_documents,
            },
        },
        view,
    }))
}

fn normalize_document_operands(
    operands: &[String],
    explicit_manual_section: Option<String>,
    tldr: bool,
    color: ColorMode,
) -> Result<NormalizedDocumentSelector, clap::Error> {
    if operands.is_empty() {
        return Err(command_error(
            ErrorKind::MissingRequiredArgument,
            if tldr {
                "--tldr requires a page name"
            } else {
                "a document selector or --input is required"
            },
            color,
        ));
    }

    if tldr {
        let inline_manual = match operands {
            [section, name] if is_manual_section(section) => {
                Some((name.as_str(), section.as_str()))
            }
            [selector] => parenthesized_manual_reference(selector),
            _ => None,
        };
        if let Some((name, section)) = inline_manual {
            return merge_manual_section(name, section, explicit_manual_section.is_some(), color);
        }
        return Ok(NormalizedDocumentSelector {
            name: normalize_tldr_topic(&operands.join(" ")),
            manual_section: explicit_manual_section,
        });
    }

    match operands {
        [selector] => {
            if let Some((name, section)) = parenthesized_manual_reference(selector) {
                merge_manual_section(name, section, explicit_manual_section.is_some(), color)
            } else {
                Ok(NormalizedDocumentSelector {
                    name: selector.clone(),
                    manual_section: explicit_manual_section,
                })
            }
        }
        [section, name] if is_manual_section(section) => {
            merge_manual_section(name, section, explicit_manual_section.is_some(), color)
        }
        _ => Err(command_error(
            ErrorKind::TooManyValues,
            "use one document selector, or SECTION NAME for a native manual",
            color,
        )),
    }
}

fn merge_manual_section(
    name: &str,
    inline_section: &str,
    has_explicit_section: bool,
    color: ColorMode,
) -> Result<NormalizedDocumentSelector, clap::Error> {
    if has_explicit_section {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "a man-style section selector cannot be combined with --man-section",
            color,
        ));
    }
    Ok(NormalizedDocumentSelector {
        name: name.to_owned(),
        manual_section: Some(inline_section.to_owned()),
    })
}

pub(super) fn non_empty(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("value must not be empty".to_owned())
    } else {
        Ok(value.to_owned())
    }
}

pub(super) fn command_error(
    kind: ErrorKind,
    message: impl std::fmt::Display,
    color: ColorMode,
) -> clap::Error {
    Cli::command().color(color.into()).error(kind, message)
}
