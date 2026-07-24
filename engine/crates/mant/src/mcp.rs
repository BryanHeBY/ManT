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
    ExcerptSelection, OutlineDetail, QueryBundle, QueryExcerpt, QueryInput, QueryOutline,
    QueryRequest, QueryView, SearchCase, SearchQuery, SearchScope, SearchSyntax,
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

/// Common local-manual selector shared by the outline and projection tools.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManualTarget {
    /// Local manual-page name, for example `tar`, `git`, or `printf`.
    topic: String,
    /// Optional manual section such as `1`, `3`, or `3p`.
    section: Option<String>,
}

/// Parameters for the hierarchy-discovery tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutlineParams {
    #[serde(flatten)]
    target: ManualTarget,
    /// Include only sections, or include addressable option and command entries.
    detail: Option<OutlineDetail>,
}

/// Parameters for retrieving one or more outline nodes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetParams {
    #[serde(flatten)]
    target: ManualTarget,
    /// Outline paths, stable IDs, or entry aliases returned by `mant_manual_outline`.
    #[schemars(length(min = 1))]
    nodes: Vec<String>,
}

/// Parameters for resolving a single option, command, or environment entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExplainParams {
    #[serde(flatten)]
    target: ManualTarget,
    /// Option spelling, command name, environment variable, outline path, or stable ID.
    entry: String,
}

/// Parameters for structure-aware manual search.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchParams {
    #[serde(flatten)]
    target: ManualTarget,
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
    word: Option<bool>,
    /// Full Markdown lines of context before and after each match, at most 100.
    #[schemars(range(max = 100))]
    context_lines: Option<u16>,
    /// Maximum result count from 1 through 10,000. The default is 100.
    #[schemars(range(min = 1, max = 10000))]
    limit: Option<u32>,
    /// Number of matches to skip for deterministic pagination.
    offset: Option<u32>,
}

// ── Query execution ──────────────────────────────────────────────────────

/// A bounded, in-process MCP server for local manual data.
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
        name = "mant_manual_outline",
        annotations(
            title = "ManT manual outline",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn manual_outline(
        &self,
        parameters: Parameters<OutlineParams>,
    ) -> Result<Json<QueryOutline>, String> {
        let parameters = parameters.0;
        let detail = parameters.detail.unwrap_or(OutlineDetail::Options);
        let request = request_for(parameters.target, QueryView::Outline { detail })?;
        let query = self.query(request).await?;
        let outline = mant_core::build_outline_with_detail(&query, detail)
            .map_err(|error| error.to_string())?;
        Ok(Json(outline))
    }

    /// Return complete content for one or more nodes from a manual outline.
    #[tool(
        name = "mant_manual_get",
        annotations(
            title = "ManT selected manual content",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn manual_get(
        &self,
        parameters: Parameters<GetParams>,
    ) -> Result<Json<QueryExcerpt>, String> {
        let parameters = parameters.0;
        validate_nodes(&parameters.nodes)?;
        let request = request_for(
            parameters.target,
            QueryView::Excerpt {
                nodes: parameters.nodes.clone(),
            },
        )?;
        let query = self.query(request).await?;
        let excerpt = mant_core::select_excerpt(&query, &parameters.nodes)
            .map_err(|error| error.to_string())?;
        Ok(Json(excerpt))
    }

    /// Explain exactly one option, command, or environment variable by alias or ID.
    #[tool(
        name = "mant_manual_explain",
        annotations(
            title = "ManT option explanation",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn manual_explain(
        &self,
        parameters: Parameters<ExplainParams>,
    ) -> Result<Json<QueryExcerpt>, String> {
        let parameters = parameters.0;
        let entry = non_empty(&parameters.entry, "entry")?;
        let request = request_for(
            parameters.target,
            QueryView::Excerpt {
                nodes: vec![entry.clone()],
            },
        )?;
        let query = self.query(request).await?;
        let excerpt =
            mant_core::select_excerpt(&query, &[entry]).map_err(|error| error.to_string())?;
        if matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { .. }]
        ) {
            Ok(Json(excerpt))
        } else {
            Err("entry does not resolve to one option, command, or environment variable".to_owned())
        }
    }

    /// Search manual text and return exact matching nodes and Markdown coordinates.
    #[tool(
        name = "mant_manual_search",
        annotations(
            title = "ManT manual search",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn manual_search(
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
            parameters.target,
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
                "Query local manual pages only. Start with mant_manual_outline, then use IDs, paths, or aliases with mant_manual_get or mant_manual_explain.",
            )
    }
}

// ── Input validation ─────────────────────────────────────────────────────

fn request_for(target: ManualTarget, view: QueryView) -> Result<QueryRequest, String> {
    let topic = non_empty(&target.topic, "topic")?;
    let section = target
        .section
        .map(|section| non_empty(&section, "section"))
        .transpose()?;
    Ok(QueryRequest {
        schema: mant_ast::RequestSchema::V3,
        input: QueryInput::Manual { topic, section },
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

    use super::MantMcpServer;

    #[test]
    fn publishes_only_the_read_only_manual_tools_with_generated_schemas() {
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
                "mant_manual_explain",
                "mant_manual_get",
                "mant_manual_outline",
                "mant_manual_search",
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
