//! Implements `ManT`'s read-only Model Context Protocol server.
//!
//! This module deliberately calls `mant-core` in-process instead of spawning
//! `mant`. It exposes the same stable outline, excerpt, and search
//! projections as the direct CLI over MCP's standard-input/output transport.

use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use mant_ast::{
    CatalogDocumentKind, CatalogQuery, DocumentCatalog, OutlineDetail, QueryExcerpt, QueryInput,
    QueryOutline, QueryRequest, QueryView, SearchCase, SearchScope, SearchSyntax,
    default_search_limit,
};
use mant_core::{QueryPolicy, QueryViewResult};
use rmcp::{
    Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::{
    io::{AsyncRead, ReadBuf},
    sync::Semaphore,
    task,
};

// ── Stdio process boundary ────────────────────────────────────────────────

/// Upper bound on one newline-delimited MCP request, in bytes.
///
/// rmcp's stdio transport reads each JSON-RPC message with an unbounded
/// `read_until(b'\n', ..)`, so a peer that streams bytes without a newline
/// would grow the read buffer without limit. This cap keeps generous headroom
/// for large legitimate tool inputs while bounding that growth, mirroring the
/// intent of the direct CLI's own stdin cap (`MAX_REQUEST_BYTES`).
const MAX_MCP_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Run the MCP server until the peer closes its standard-input stream.
///
/// The transport is deliberately silent: MCP hosts own process logging and
/// tool failures already use structured protocol errors. Native lowering
/// diagnostics remain available through the ordinary CLI JSON surface.
pub(super) async fn run_stdio() -> u8 {
    let transport = (
        LineBoundedReader::new(tokio::io::stdin(), MAX_MCP_LINE_BYTES),
        tokio::io::stdout(),
    );
    let Ok(service) = MantMcpServer::new().serve(transport).await else {
        return 1;
    };

    match service.waiting().await {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

/// Wraps an [`AsyncRead`] and fails once a single line exceeds `max_line`.
///
/// The transport frames requests on `\n`, so counting bytes since the last
/// newline bounds one request. Exceeding the limit surfaces an I/O error that
/// ends the read loop rather than letting the buffer grow without limit.
struct LineBoundedReader<R> {
    inner: R,
    max_line: usize,
    since_newline: usize,
    tripped: bool,
}

impl<R> LineBoundedReader<R> {
    fn new(inner: R, max_line: usize) -> Self {
        Self {
            inner,
            max_line,
            since_newline: 0,
            tripped: false,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LineBoundedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // The AsyncRead contract requires that an error poll fill no bytes.
        // A prior read that pushed the line past the cap therefore reports the
        // overrun here, on its own poll, before touching the inner reader.
        if self.tripped {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP request line exceeded the maximum allowed length",
            )));
        }

        let start = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            let new = &buf.filled()[start..];
            match new.iter().rposition(|&byte| byte == b'\n') {
                Some(last_newline) => self.since_newline = new.len() - last_newline - 1,
                None => self.since_newline += new.len(),
            }
            // Trip on overrun; the next poll returns the error with nothing
            // filled. At most one buffer's worth passes beyond the cap.
            self.tripped = self.since_newline > self.max_line;
        }
        poll
    }
}

// ── MCP parameter contracts ──────────────────────────────────────────────

/// A document name resolved through registered Markdown and native manual paths.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DocumentSelector {
    /// Registered Markdown or manual page name, for example `git` or `mant`.
    name: String,
    /// Optional configured Markdown source. It bypasses root documents and manuals.
    source: Option<String>,
    /// Optional man section. Supplying it bypasses registered Markdown.
    section: Option<String>,
}

/// Parameters for the hierarchy-discovery tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutlineParams {
    #[serde(flatten)]
    selector: DocumentSelector,
    /// Include only sections, or include every addressable semantic entry.
    detail: Option<OutlineDetail>,
}

/// Parameters for retrieving one or more outline nodes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetParams {
    #[serde(flatten)]
    selector: DocumentSelector,
    /// Outline paths, stable IDs, or entry aliases returned by `mant_document_outline`.
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "lenient_nodes")]
    nodes: Vec<String>,
}

/// Parameters for resolving a single option, command, variable, or environment entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExplainParams {
    #[serde(flatten)]
    selector: DocumentSelector,
    /// Option spelling, command or variable name, outline path, or stable ID.
    entry: String,
}

/// Parameters for structure-aware manual search.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchParams {
    #[serde(flatten)]
    selector: DocumentSelector,
    /// Literal text or a regular expression, depending on `syntax`.
    #[schemars(length(min = 1, max = 4096))]
    pattern: String,
    /// Interpret `pattern` literally (the default) or as a regular expression.
    syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    case: Option<SearchCase>,
    /// Search visible text (the default) or generated `CommonMark` source.
    scope: Option<SearchScope>,
    /// Restrict matches to Unicode-aware word boundaries.
    #[serde(default, deserialize_with = "lenient_scalar")]
    word: Option<bool>,
    /// Full Markdown lines of context before and after each match, at most 100.
    #[schemars(range(max = 100))]
    #[serde(default, deserialize_with = "lenient_scalar", alias = "context_lines")]
    context_lines: Option<u16>,
    /// Maximum result count from 1 through 10,000. The default is 100.
    #[schemars(range(min = 1, max = 10000))]
    #[serde(default, deserialize_with = "lenient_scalar")]
    limit: Option<u32>,
    /// Number of matches to skip for deterministic pagination.
    #[serde(default, deserialize_with = "lenient_scalar")]
    offset: Option<u32>,
}

/// Filters and pagination for the unified local document catalog.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DocumentListParams {
    /// Case-insensitive substring applied to document names.
    query: Option<String>,
    /// Restrict discovery to registered Markdown or manual pages.
    kind: Option<CatalogDocumentKind>,
    /// Interpret `query` literally (the default) or as a regular expression.
    syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    case: Option<SearchCase>,
    /// Restrict manual pages to one exact section; excludes Markdown entries.
    section: Option<String>,
    /// Restrict Markdown discovery to one configured source.
    source: Option<String>,
    /// Maximum entries returned from 1 through 10,000. The default is 100.
    #[schemars(range(min = 1, max = 10000))]
    #[serde(default, deserialize_with = "lenient_scalar")]
    limit: Option<u32>,
    /// Number of matching entries to skip for deterministic pagination.
    #[serde(default, deserialize_with = "lenient_scalar")]
    offset: Option<u32>,
}

/// Accepts a native scalar or its stringified spelling such as `"10"` or `"True"`.
fn lenient_scalar<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + std::str::FromStr,
    T::Err: std::fmt::Display,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(text) => text
            .trim()
            .to_ascii_lowercase()
            .parse()
            .map(Some)
            .map_err(|error| D::Error::custom(format!("cannot parse {text:?}: {error}"))),
        other => serde_json::from_value(other)
            .map(Some)
            .map_err(D::Error::custom),
    }
}

const NODES_HINT: &str = r#"nodes must be an array of outline selectors such as ["2","1/o1"]"#;

/// Accepts a selector array, one bare selector, or a stringified JSON array.
fn lenient_nodes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    let value = match value {
        serde_json::Value::String(text) => match serde_json::from_str(&text) {
            Ok(parsed @ serde_json::Value::Array(_)) => parsed,
            _ => return Ok(vec![text]),
        },
        other => other,
    };
    serde_json::from_value(value)
        .map_err(|error| D::Error::custom(format!("{error}; {NODES_HINT}")))
}

// ── Query execution ──────────────────────────────────────────────────────

/// A bounded, in-process MCP server for local structured documents.
///
/// `mant-core` performs filesystem reads and native parser calls synchronously.
/// The semaphore keeps those costly calls serialized, while `spawn_blocking`
/// leaves the stdio JSON-RPC loop responsive to protocol traffic.
#[derive(Debug, Clone)]
struct MantMcpServer {
    tool_router: ToolRouter<Self>,
    query_gate: Arc<Semaphore>,
}

impl MantMcpServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            query_gate: Arc::new(Semaphore::new(1)),
        }
    }

    async fn query(&self, request: QueryRequest) -> Result<QueryViewResult, String> {
        let permit = Arc::clone(&self.query_gate)
            .acquire_owned()
            .await
            .map_err(|_| "MCP query service is shutting down".to_owned())?;
        task::spawn_blocking(move || {
            let _permit = permit;
            mant_core::execute_query(&request, QueryPolicy::default())
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("MCP query worker failed: {error}"))?
    }
}

#[tool_router(router = tool_router)]
impl MantMcpServer {
    /// List registered Markdown and locally indexed manual pages.
    #[tool(
        name = "mant_documents_list",
        annotations(
            title = "ManT local documents",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn documents_list(
        &self,
        parameters: Parameters<DocumentListParams>,
    ) -> Result<Json<DocumentCatalog>, String> {
        let parameters = validate_document_list(parameters.0)?;
        let permit = Arc::clone(&self.query_gate)
            .acquire_owned()
            .await
            .map_err(|_| "MCP query service is shutting down".to_owned())?;
        let query = catalog_query(&parameters);
        let catalog = task::spawn_blocking(move || {
            let _permit = permit;
            mant_core::discover_documents(&query)
        })
        .await
        .map_err(|error| format!("MCP document discovery worker failed: {error}"))??;
        Ok(Json(catalog))
    }

    /// Return a hierarchical tree of sections and optional addressable entries.
    #[tool(
        name = "mant_document_outline",
        annotations(
            title = "ManT document outline",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn document_outline(
        &self,
        parameters: Parameters<OutlineParams>,
    ) -> Result<Json<QueryOutline>, String> {
        let parameters = parameters.0;
        let detail = parameters.detail.unwrap_or(OutlineDetail::Entries);
        let request = request_for(parameters.selector, QueryView::Outline { detail });
        let QueryViewResult::Outline(mut outline) = self.query(request).await? else {
            unreachable!("outline request materializes an outline")
        };
        discard_outline_diagnostics(&mut outline);
        Ok(Json(outline))
    }

    /// Return complete content for one or more nodes from a document outline.
    #[tool(
        name = "mant_document_get",
        annotations(
            title = "ManT selected document content",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn document_get(
        &self,
        parameters: Parameters<GetParams>,
    ) -> Result<Json<QueryExcerpt>, String> {
        let parameters = parameters.0;
        let request = request_for(
            parameters.selector,
            QueryView::Excerpt {
                nodes: parameters.nodes.clone(),
            },
        );
        let QueryViewResult::Excerpt(mut excerpt) = self.query(request).await? else {
            unreachable!("excerpt request materializes an excerpt")
        };
        discard_lowering_diagnostics(&mut excerpt);
        Ok(Json(excerpt))
    }

    /// Explain exactly one option, command, variable, or environment variable by alias or ID.
    #[tool(
        name = "mant_document_explain",
        annotations(
            title = "ManT semantic entry explanation",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn document_explain(
        &self,
        parameters: Parameters<ExplainParams>,
    ) -> Result<Json<QueryExcerpt>, String> {
        let parameters = parameters.0;
        let entry = non_empty(&parameters.entry, "entry")?;
        let request = request_for(
            parameters.selector,
            QueryView::Explain {
                entry: entry.clone(),
            },
        );
        let QueryViewResult::Excerpt(mut excerpt) = self.query(request).await? else {
            unreachable!("explain request materializes an excerpt")
        };
        discard_lowering_diagnostics(&mut excerpt);
        Ok(Json(excerpt))
    }

    /// Search document text and return exact matching nodes and Markdown coordinates.
    #[tool(
        name = "mant_document_search",
        annotations(
            title = "ManT document search",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn document_search(
        &self,
        parameters: Parameters<SearchParams>,
    ) -> Result<Json<mant_ast::QuerySearch>, String> {
        let parameters = parameters.0;
        let request = request_for(
            parameters.selector,
            QueryView::Search {
                pattern: parameters.pattern.trim().to_owned(),
                syntax: parameters.syntax.unwrap_or_default(),
                case: parameters.case.unwrap_or_default(),
                scope: parameters.scope.unwrap_or_default(),
                word: parameters.word.unwrap_or(false),
                context_lines: parameters.context_lines.unwrap_or(0),
                limit: parameters.limit.unwrap_or_else(default_search_limit),
                offset: parameters.offset.unwrap_or(0),
            },
        );
        let QueryViewResult::Search(result) = self.query(request).await? else {
            unreachable!("search request materializes search results")
        };
        Ok(Json(result))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MantMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mant", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read locally installed Markdown documents and manual pages by name. Use mant_documents_list for discovery, optionally select a configured source, then call mant_document_outline before retrieving IDs, paths, or aliases. Files may change between calls; this server does not update sources.",
            )
    }
}

// ── Input validation ─────────────────────────────────────────────────────

fn validate_document_list(
    mut parameters: DocumentListParams,
) -> Result<DocumentListParams, String> {
    parameters.query = parameters
        .query
        .map(|query| query.trim().to_owned())
        .filter(|query| !query.is_empty());
    parameters.section = parameters
        .section
        .map(|section| non_empty(&section, "section"))
        .transpose()?;
    parameters.source = parameters
        .source
        .map(|source| non_empty(&source, "source"))
        .transpose()?;
    if parameters.source.is_some() && parameters.section.is_some() {
        return Err("source and section cannot be combined".to_owned());
    }
    let limit = parameters.limit.unwrap_or(100);
    if !(1..=10_000).contains(&limit) {
        return Err("limit must be between 1 and 10000".to_owned());
    }
    parameters.limit = Some(limit);
    parameters.offset = Some(parameters.offset.unwrap_or(0));
    Ok(parameters)
}

fn catalog_query(parameters: &DocumentListParams) -> CatalogQuery {
    CatalogQuery {
        pattern: parameters.query.clone(),
        syntax: parameters.syntax.unwrap_or_default(),
        case: parameters.case.unwrap_or_default(),
        kind: parameters.kind,
        source: parameters.source.clone(),
        section: parameters.section.clone(),
        limit: parameters.limit.unwrap_or(100),
        offset: parameters.offset.unwrap_or(0),
    }
}

/// Keep lowering diagnostics out of the agent-facing transport.
///
/// The ordinary CLI JSON representation remains the inspection surface for
/// these findings. MCP callers receive only selected document content and
/// structured tool errors, avoiding repeated parser noise in agent context.
fn discard_lowering_diagnostics(excerpt: &mut QueryExcerpt) {
    excerpt.diagnostics.clear();
}

/// Retain the compact completeness signal while omitting parser detail.
fn discard_outline_diagnostics(outline: &mut QueryOutline) {
    outline.diagnostics.clear();
}

fn request_for(selector: DocumentSelector, view: QueryView) -> QueryRequest {
    let input = QueryInput::Document {
        selector: selector.name,
        source: selector.source,
        section: selector.section,
    };
    QueryRequest {
        schema: mant_ast::RequestSchema::V7,
        input,
        view,
    }
}

fn non_empty(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use mant_ast::{CatalogDocumentKind, DocumentAddress};
    use mant_core::{AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin};
    use serde_json::json;

    use super::{
        DocumentListParams, GetParams, MantMcpServer, OutlineParams, SearchParams, catalog_query,
        validate_document_list,
    };

    #[test]
    fn publishes_only_the_read_only_document_tools_with_generated_schemas() {
        let server = MantMcpServer::new();
        let tools = server.tool_router.list_all();
        let mut names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        names.sort_unstable();

        assert_eq!(
            names,
            [
                "mant_document_explain",
                "mant_document_get",
                "mant_document_outline",
                "mant_document_search",
                "mant_documents_list",
            ]
        );
        for tool in tools {
            assert!(tool.input_schema.contains_key("properties"));
            assert!(tool.output_schema.is_some());
            let annotations = tool.annotations.expect("read-only annotation");
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.open_world_hint, Some(false));
        }
    }

    #[test]
    fn document_tools_publish_a_name_without_an_arbitrary_path_target() {
        let server = MantMcpServer::new();
        let tools = server.tool_router.list_all();
        let outline = tools
            .iter()
            .find(|tool| tool.name == "mant_document_outline")
            .expect("outline tool");
        let schema = serde_json::to_value(&outline.input_schema).expect("schema JSON");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert!(schema["properties"]["source"].is_object());
        assert!(schema["properties"]["section"].is_object());
        assert!(schema["properties"].get("target").is_none());
        assert!(!schema.to_string().contains("markdown-file"));
    }

    #[test]
    fn a_name_and_optional_manual_section_deserialize_directly() {
        let parameters: OutlineParams = serde_json::from_value(json!({
            "name": "printf",
            "section": "3"
        }))
        .expect("name selector");
        assert_eq!(parameters.selector.name, "printf");
        assert_eq!(parameters.selector.section.as_deref(), Some("3"));
    }

    #[test]
    fn a_configured_source_is_available_but_cannot_combine_with_a_section() {
        let parameters: OutlineParams = serde_json::from_value(json!({
            "name": "tool",
            "source": "team"
        }))
        .expect("source selector");
        assert_eq!(parameters.selector.source.as_deref(), Some("team"));
        let request = super::request_for(parameters.selector, mant_ast::QueryView::Full {});
        assert!(
            mant_core::validate_query_request(&request, mant_core::QueryPolicy::default()).is_ok()
        );

        let parameters: OutlineParams = serde_json::from_value(json!({
            "name": "tool",
            "source": "team",
            "section": "1"
        }))
        .expect("deserialize combined selector before semantic validation");
        let request = super::request_for(parameters.selector, mant_ast::QueryView::Full {});
        assert!(
            mant_core::validate_query_request(&request, mant_core::QueryPolicy::default())
                .expect_err("reject combined source and section")
                .to_string()
                .contains("cannot be combined")
        );
    }

    #[test]
    fn arbitrary_markdown_paths_are_not_mcp_inputs() {
        let error = serde_json::from_value::<OutlineParams>(json!({
            "target": {"kind": "markdown-file", "path": "README.md"}
        }))
        .expect_err("path target must be rejected");
        assert!(error.to_string().contains("name"));
    }

    #[test]
    fn document_catalog_filters_and_paginates_both_source_families() {
        let parameters = validate_document_list(DocumentListParams {
            query: Some("PRINT".to_owned()),
            kind: Some(CatalogDocumentKind::Manual),
            syntax: None,
            case: None,
            section: None,
            source: None,
            limit: Some(1),
            offset: Some(1),
        })
        .expect("catalog parameters");
        let catalog = mant_core::query_available_documents(
            &[
                AvailableDocument {
                    name: "printf".to_owned(),
                    logical_path: "printf".to_owned(),
                    kind: AvailableDocumentKind::Markdown,
                    section: None,
                    path: PathBuf::from("/data/mant/printf.md"),
                    origin: AvailableDocumentOrigin::Documents,
                },
                AvailableDocument {
                    name: "printf".to_owned(),
                    logical_path: "printf".to_owned(),
                    kind: AvailableDocumentKind::Manual,
                    section: Some("1".to_owned()),
                    path: PathBuf::from("/usr/share/man/man1/printf.1.gz"),
                    origin: AvailableDocumentOrigin::ManualPath,
                },
                AvailableDocument {
                    name: "printf".to_owned(),
                    logical_path: "printf".to_owned(),
                    kind: AvailableDocumentKind::Manual,
                    section: Some("3".to_owned()),
                    path: PathBuf::from("/usr/share/man/man3/printf.3.gz"),
                    origin: AvailableDocumentOrigin::ManualPath,
                },
            ],
            &catalog_query(&parameters),
        )
        .expect("catalog");

        assert_eq!(catalog.total, 2);
        assert_eq!(catalog.returned, 1);
        assert_eq!(catalog.offset, 1);
        assert!(!catalog.truncated);
        assert_eq!(
            catalog.documents[0].address,
            DocumentAddress::Manual {
                name: "printf".to_owned(),
                section: "3".to_owned(),
            }
        );
    }

    #[test]
    fn stringified_search_scalars_and_snake_case_context_still_deserialize() {
        let parameters: SearchParams = serde_json::from_value(json!({
            "name": "ls",
            "pattern": "sort",
            "word": "True",
            "context_lines": "2",
            "limit": "10",
            "offset": 5,
        }))
        .expect("lenient search parameters");
        assert_eq!(parameters.word, Some(true));
        assert_eq!(parameters.context_lines, Some(2));
        assert_eq!(parameters.limit, Some(10));
        assert_eq!(parameters.offset, Some(5));
    }

    #[test]
    fn unparsable_search_scalars_still_fail() {
        let error = serde_json::from_value::<SearchParams>(json!({
            "name": "ls",
            "pattern": "sort",
            "limit": "ten",
        }))
        .expect_err("invalid limit");
        assert!(error.to_string().contains(r#"cannot parse "ten""#));
    }

    #[test]
    fn node_selectors_accept_arrays_bare_strings_and_stringified_arrays() {
        for (nodes, expected) in [
            (json!(["2", "1/o1"]), vec!["2", "1/o1"]),
            (json!("2"), vec!["2"]),
            (json!("[\"2\", \"1/o1\"]"), vec!["2", "1/o1"]),
        ] {
            let parameters: GetParams = serde_json::from_value(json!({
                "name": "ls",
                "nodes": nodes,
            }))
            .expect("lenient nodes");
            assert_eq!(parameters.nodes, expected);
        }
    }

    #[test]
    fn malformed_node_selectors_report_a_correct_example() {
        let error = serde_json::from_value::<GetParams>(json!({
            "name": "ls",
            "nodes": "[1, 2]",
        }))
        .expect_err("non-string selectors");
        assert!(
            error.to_string().contains(r#"["2","1/o1"]"#),
            "missing example in: {error}"
        );
    }

    #[test]
    fn excerpts_discard_all_lowering_diagnostics() {
        use mant_ast::{Diagnostic, DiagnosticLevel, ExcerptSchema, QueryExcerpt};

        let diagnostic = |level| Diagnostic {
            level,
            code: None,
            message: "finding".to_owned(),
            source: None,
        };
        let mut excerpt = QueryExcerpt {
            schema: ExcerptSchema::V7,
            label: "demo".to_owned(),
            producer: None,
            source: None,
            meta: None,
            diagnostics: vec![
                diagnostic(DiagnosticLevel::Style),
                diagnostic(DiagnosticLevel::Warning),
                diagnostic(DiagnosticLevel::Error),
                diagnostic(DiagnosticLevel::Unsupported),
            ],
            selections: Vec::new(),
        };

        super::discard_lowering_diagnostics(&mut excerpt);
        assert!(excerpt.diagnostics.is_empty());
    }

    #[test]
    fn outlines_keep_completeness_without_lowering_diagnostics() {
        use mant_ast::{Diagnostic, DiagnosticLevel, OutlineDetail, OutlineSchema, QueryOutline};

        let mut outline = QueryOutline {
            schema: OutlineSchema::V7,
            detail: OutlineDetail::Entries,
            label: "demo".to_owned(),
            source: None,
            meta: None,
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("markdown.semantic-entry.invalid-option-name".to_owned()),
                message: "finding".to_owned(),
                source: None,
            }],
            entries_complete: false,
            nodes: Vec::new(),
        };

        super::discard_outline_diagnostics(&mut outline);
        assert!(outline.diagnostics.is_empty());
        assert!(!outline.entries_complete);
    }

    // Read the wrapped source to end (or first error) on a current-thread
    // runtime, so the bound is exercised through the real AsyncRead path.
    fn read_to_end(source: &'static [u8], max_line: usize) -> io::Result<Vec<u8>> {
        use tokio::io::AsyncReadExt;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("current-thread runtime");
        runtime.block_on(async move {
            let mut reader = super::LineBoundedReader::new(source, max_line);
            let mut collected = Vec::new();
            reader.read_to_end(&mut collected).await?;
            Ok(collected)
        })
    }

    #[test]
    fn line_bounded_reader_passes_lines_within_the_limit() {
        let source: &[u8] = b"short line\nnext\n";
        let collected = read_to_end(source, 32).expect("read within limit");
        assert_eq!(collected, source);
    }

    #[test]
    fn line_bounded_reader_rejects_a_line_over_the_limit() {
        // No newline within the cap, so the running count crosses `max_line`.
        let error = read_to_end(b"aaaaaaaaaaaaaaaaaaaa", 8).expect_err("oversized line must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn line_bounded_reader_resets_its_count_on_each_newline() {
        // Every line is under the cap even though the total exceeds it.
        let source: &[u8] = b"aaaa\nbbbb\ncccc\n";
        let collected = read_to_end(source, 5).expect("newlines reset the counter");
        assert_eq!(collected, source);
    }
}
