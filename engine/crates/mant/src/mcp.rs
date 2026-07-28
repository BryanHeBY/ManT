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
    DiagnosticLevel, ExcerptSelection, OutlineDetail, QueryBundle, QueryExcerpt, QueryInput,
    QueryOutline, QueryRequest, QueryView, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    default_search_limit,
};
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
pub(super) async fn run_stdio() -> u8 {
    let transport = (
        LineBoundedReader::new(tokio::io::stdin(), MAX_MCP_LINE_BYTES),
        tokio::io::stdout(),
    );
    let service = match MantMcpServer::new().serve(transport).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("mant: cannot start MCP stdio server: {error}");
            return 1;
        }
    };

    match service.waiting().await {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("mant: MCP stdio server failed: {error}");
            1
        }
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

/// The query-input `target` argument with tolerance for stringified JSON.
///
/// Some function-calling models serialize the nested `target` object as one
/// JSON string. The public `--request-json` contract stays strict; only this
/// MCP boundary re-parses such strings before normal validation, and failures
/// answer with a correct example so the model can retry.
#[derive(Debug)]
struct Target(QueryInput);

const TARGET_HINT: &str = r#"target must be an object such as {"kind":"manual","topic":"ls"} or {"kind":"markdown-file","path":"README.md"}"#;

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        let value = serde_json::Value::deserialize(deserializer)?;
        let value = match value {
            serde_json::Value::String(text) => serde_json::from_str(&text)
                .map_err(|error| D::Error::custom(format!("{error}; {TARGET_HINT}")))?,
            other => other,
        };
        serde_json::from_value(value)
            .map(Self)
            .map_err(|error| D::Error::custom(format!("{error}; {TARGET_HINT}")))
    }
}

impl JsonSchema for Target {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        QueryInput::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        QueryInput::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        QueryInput::json_schema(generator)
    }

    fn inline_schema() -> bool {
        QueryInput::inline_schema()
    }
}

/// Parameters for the hierarchy-discovery tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutlineParams {
    /// A local manual topic or Markdown file using the public query-input schema.
    target: Target,
    /// Include only sections, or include addressable option and command entries.
    detail: Option<OutlineDetail>,
}

/// Parameters for retrieving one or more outline nodes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetParams {
    /// A local manual topic or Markdown file using the public query-input schema.
    target: Target,
    /// Outline paths, stable IDs, or entry aliases returned by `mant_document_outline`.
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "lenient_nodes")]
    nodes: Vec<String>,
}

/// Parameters for resolving a single option, command, or environment entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExplainParams {
    /// A local manual topic or Markdown file using the public query-input schema.
    target: Target,
    /// Option spelling, command name, environment variable, outline path, or stable ID.
    entry: String,
}

/// Parameters for structure-aware manual search.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchParams {
    /// A local manual topic or Markdown file using the public query-input schema.
    target: Target,
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

const NODES_HINT: &str =
    r#"nodes must be an array of outline selectors such as ["2","options.-l"]"#;

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

    async fn query(&self, request: QueryRequest) -> Result<QueryBundle, String> {
        let permit = Arc::clone(&self.query_gate)
            .acquire_owned()
            .await
            .map_err(|_| "MCP query service is shutting down".to_owned())?;
        task::spawn_blocking(move || {
            let _permit = permit;
            mant_core::query(&request).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("MCP query worker failed: {error}"))?
    }
}

#[tool_router(router = tool_router)]
impl MantMcpServer {
    /// Return a hierarchical tree of sections and optional addressable entries.
    #[tool(
        name = "mant_document_outline",
        annotations(
            title = "ManT document outline",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn document_outline(
        &self,
        parameters: Parameters<OutlineParams>,
    ) -> Result<Json<QueryOutline>, String> {
        let parameters = parameters.0;
        let detail = parameters.detail.unwrap_or(OutlineDetail::Options);
        let request = request_for(parameters.target.0, QueryView::Outline { detail })?;
        let query = self.query(request).await?;
        let outline = mant_core::build_outline_with_detail(&query, detail)
            .map_err(|error| error.to_string())?;
        Ok(Json(outline))
    }

    /// Return complete content for one or more nodes from a document outline.
    #[tool(
        name = "mant_document_get",
        annotations(
            title = "ManT selected document content",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn document_get(
        &self,
        parameters: Parameters<GetParams>,
    ) -> Result<Json<QueryExcerpt>, String> {
        let parameters = parameters.0;
        validate_nodes(&parameters.nodes)?;
        let request = request_for(
            parameters.target.0,
            QueryView::Excerpt {
                nodes: parameters.nodes.clone(),
            },
        )?;
        let query = self.query(request).await?;
        let mut excerpt = mant_core::select_excerpt(&query, &parameters.nodes)
            .map_err(|error| error.to_string())?;
        retain_consumer_diagnostics(&mut excerpt);
        Ok(Json(excerpt))
    }

    /// Explain exactly one option, command, or environment variable by alias or ID.
    #[tool(
        name = "mant_document_explain",
        annotations(
            title = "ManT option explanation",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
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
            parameters.target.0,
            QueryView::Excerpt {
                nodes: vec![entry.clone()],
            },
        )?;
        let query = self.query(request).await?;
        let mut excerpt =
            mant_core::select_excerpt(&query, &[entry]).map_err(|error| error.to_string())?;
        if matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { .. }]
        ) {
            retain_consumer_diagnostics(&mut excerpt);
            Ok(Json(excerpt))
        } else {
            Err("entry does not resolve to one option, command, or environment variable".to_owned())
        }
    }

    /// Search document text and return exact matching nodes and Markdown coordinates.
    #[tool(
        name = "mant_document_search",
        annotations(
            title = "ManT document search",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn document_search(
        &self,
        parameters: Parameters<SearchParams>,
    ) -> Result<Json<mant_ast::QuerySearch>, String> {
        let parameters = parameters.0;
        let search = SearchQuery {
            pattern: non_empty(&parameters.pattern, "pattern")?,
            syntax: parameters.syntax.unwrap_or_default(),
            case: parameters.case.unwrap_or_default(),
            scope: parameters.scope.unwrap_or_default(),
            word: parameters.word.unwrap_or(false),
            context_lines: parameters.context_lines.unwrap_or(0),
            limit: parameters.limit.unwrap_or_else(default_search_limit),
            offset: parameters.offset.unwrap_or(0),
        };
        mant_core::validate_search_query(&search).map_err(|error| error.to_string())?;
        let request = request_for(
            parameters.target.0,
            QueryView::Search {
                pattern: search.pattern.clone(),
                syntax: search.syntax,
                case: search.case,
                scope: search.scope,
                word: search.word,
                context_lines: search.context_lines,
                limit: search.limit,
                offset: search.offset,
            },
        )?;
        let query = self.query(request).await?;
        let result = mant_core::search_query(&query, &search).map_err(|error| error.to_string())?;
        Ok(Json(result))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MantMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mant", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Query local manual pages or Markdown files. Start with mant_document_outline, then use IDs, paths, or aliases with mant_document_get or mant_document_explain.",
            )
    }
}

// ── Input validation ─────────────────────────────────────────────────────

/// Drops parser lint levels that only concern manual-page authors.
///
/// The direct CLI already treats `style` and `warning` findings as opt-in
/// debug output (`--force-libmandoc`). MCP consumers only need the levels
/// that signal degraded, best-effort document content.
fn retain_consumer_diagnostics(excerpt: &mut QueryExcerpt) {
    excerpt.diagnostics.retain(|diagnostic| {
        matches!(
            diagnostic.level,
            DiagnosticLevel::Error | DiagnosticLevel::Unsupported
        )
    });
}

fn request_for(target: QueryInput, view: QueryView) -> Result<QueryRequest, String> {
    let input = match target {
        QueryInput::Manual { topic, section } => QueryInput::Manual {
            topic: non_empty(&topic, "topic")?,
            section: section
                .map(|section| non_empty(&section, "section"))
                .transpose()?,
        },
        QueryInput::MarkdownFile { path } => QueryInput::MarkdownFile {
            path: non_empty(&path, "path")?,
        },
    };
    Ok(QueryRequest {
        schema: mant_ast::RequestSchema::V3,
        input,
        view,
    })
}

fn validate_nodes(nodes: &[String]) -> Result<(), String> {
    if nodes.is_empty() {
        return Err("at least one outline node is required".to_owned());
    }
    if nodes.iter().any(|node| node.trim().is_empty()) {
        return Err("outline node must not be empty".to_owned());
    }
    Ok(())
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
    use std::io;

    use mant_ast::QueryInput;
    use serde_json::json;

    use super::{GetParams, MantMcpServer, OutlineParams, SearchParams};

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
    fn the_target_wrapper_publishes_the_public_query_input_schema() {
        let server = MantMcpServer::new();
        let tools = server.tool_router.list_all();
        let outline = tools
            .iter()
            .find(|tool| tool.name == "mant_document_outline")
            .expect("outline tool");
        let schema = serde_json::to_value(&outline.input_schema).expect("schema JSON");
        assert_eq!(
            schema["properties"]["target"]["$ref"],
            json!("#/$defs/QueryInput")
        );
        assert!(schema["$defs"]["QueryInput"]["oneOf"].is_array());
    }

    #[test]
    fn a_target_object_deserializes_directly() {
        let parameters: OutlineParams =
            serde_json::from_value(json!({"target": {"kind": "manual", "topic": "ls"}}))
                .expect("object target");
        assert_eq!(
            parameters.target.0,
            QueryInput::Manual {
                topic: "ls".to_owned(),
                section: None,
            }
        );
    }

    #[test]
    fn a_stringified_target_object_still_deserializes() {
        let parameters: OutlineParams = serde_json::from_value(
            json!({"target": "{\"kind\": \"markdown-file\", \"path\": \"README.md\"}"}),
        )
        .expect("stringified target");
        assert_eq!(
            parameters.target.0,
            QueryInput::MarkdownFile {
                path: "README.md".to_owned(),
            }
        );
    }

    #[test]
    fn an_invalid_target_reports_a_correct_example() {
        for target in [json!("ls"), json!(42), json!({"kind": "unknown"})] {
            let error = serde_json::from_value::<OutlineParams>(json!({"target": target}))
                .expect_err("invalid target");
            assert!(
                error
                    .to_string()
                    .contains(r#"{"kind":"manual","topic":"ls"}"#),
                "missing example in: {error}"
            );
        }
    }

    #[test]
    fn stringified_search_scalars_and_snake_case_context_still_deserialize() {
        let parameters: SearchParams = serde_json::from_value(json!({
            "target": {"kind": "manual", "topic": "ls"},
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
            "target": {"kind": "manual", "topic": "ls"},
            "pattern": "sort",
            "limit": "ten",
        }))
        .expect_err("invalid limit");
        assert!(error.to_string().contains(r#"cannot parse "ten""#));
    }

    #[test]
    fn node_selectors_accept_arrays_bare_strings_and_stringified_arrays() {
        for (nodes, expected) in [
            (json!(["2", "options.-l"]), vec!["2", "options.-l"]),
            (json!("2"), vec!["2"]),
            (json!("[\"2\", \"options.-l\"]"), vec!["2", "options.-l"]),
        ] {
            let parameters: GetParams = serde_json::from_value(json!({
                "target": {"kind": "manual", "topic": "ls"},
                "nodes": nodes,
            }))
            .expect("lenient nodes");
            assert_eq!(parameters.nodes, expected);
        }
    }

    #[test]
    fn malformed_node_selectors_report_a_correct_example() {
        let error = serde_json::from_value::<GetParams>(json!({
            "target": {"kind": "manual", "topic": "ls"},
            "nodes": "[1, 2]",
        }))
        .expect_err("non-string selectors");
        assert!(
            error.to_string().contains(r#"["2","options.-l"]"#),
            "missing example in: {error}"
        );
    }

    #[test]
    fn excerpts_keep_only_degradation_diagnostics() {
        use mant_ast::{Diagnostic, DiagnosticLevel, ExcerptSchema, QueryExcerpt};

        let diagnostic = |level| Diagnostic {
            level,
            code: None,
            message: "finding".to_owned(),
            source: None,
        };
        let mut excerpt = QueryExcerpt {
            schema: ExcerptSchema::V3,
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

        super::retain_consumer_diagnostics(&mut excerpt);
        assert_eq!(
            excerpt
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.level)
                .collect::<Vec<_>>(),
            [DiagnosticLevel::Error, DiagnosticLevel::Unsupported]
        );
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
