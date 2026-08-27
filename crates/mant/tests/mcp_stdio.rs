//! Verifies the compact MCP tool surface over the real stdio transport.

mod support;

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

use support::{configure_registered_documents, registered_documents_dir};

#[test]
fn stdio_mode_exposes_compact_text_first_document_tools() {
    let executable = env!("CARGO_BIN_EXE_mant");
    let fixture_root = registered_document_fixture();
    let mut command = Command::new(executable);
    configure_registered_documents(&mut command, &fixture_root);
    #[cfg(windows)]
    command.env("PATHEXT", ".EXE;.COM;.MSC;.VBS");
    let mut child = command
        .arg("--mcp")
        .env("MANT_MANPATH", fixture_root.join("manuals"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant MCP server");
    let mut input = child.stdin.take().expect("MCP stdin");
    let output = child.stdout.take().expect("MCP stdout");
    let diagnostics = child.stderr.take().expect("MCP stderr");

    initialize(&mut input);
    request_tool_list(&mut input);
    input.flush().expect("flush MCP requests");

    let mut lines = BufReader::new(output).lines();
    let initialization = parse_reply(lines.next().expect("initialization reply"));
    assert_eq!(initialization["id"], 1);
    assert_eq!(initialization["result"]["serverInfo"]["name"], "mant");
    assert!(
        initialization["result"]["instructions"]
            .as_str()
            .expect("server instructions")
            .contains("may resolve uncertainty")
    );
    assert!(
        initialization["result"]["instructions"]
            .as_str()
            .expect("server instructions")
            .contains("untrusted reference material")
    );

    let tools_reply = parse_reply(lines.next().expect("tools list reply"));
    assert_eq!(tools_reply["id"], 2);
    assert!(
        tools_reply.to_string().len() < 16_000,
        "tool schemas grew unexpectedly"
    );
    assert_tool_catalog(
        tools_reply["result"]["tools"]
            .as_array()
            .expect("tool list"),
    );

    request_document_tools(&mut input);
    input.flush().expect("flush tool calls");

    let replies = (0..(14 + usize::from(cfg!(windows))))
        .map(|_| parse_reply(lines.next().expect("tool reply")))
        .collect::<Vec<_>>();
    assert_tool_replies(&replies);

    assert_silent_shutdown(child, input, diagnostics, fixture_root);
}

#[test]
fn oversized_request_lines_are_fatal_and_keep_stderr_silent() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mant"))
        .arg("--mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant MCP server");
    let mut input = child.stdin.take().expect("MCP stdin");
    input
        .write_all(&vec![b'x'; 256 * 1024 + 1])
        .expect("write oversized MCP line");
    drop(input);

    let output = child.wait_with_output().expect("wait for MCP failure");
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status: {output:?}"
    );
    assert!(
        output.stdout.is_empty(),
        "fatal input emitted protocol data"
    );
    assert!(output.stderr.is_empty(), "fatal input emitted stderr noise");
}

fn request_document_tools(input: &mut impl Write) {
    call_tool(
        input,
        3,
        "mant_find",
        &json!({
            "query": "^mcp-",
            "syntax": "regex",
            "case": "sensitive"
        }),
    );
    call_tool(
        input,
        4,
        "mant_search",
        &json!({
            "documents": ["documents/mcp-registered"],
            "pattern": "needle",
            "word": true,
            "contextLines": 1,
            "maxMatches": 1
        }),
    );
    call_tool(
        input,
        5,
        "mant_read",
        &json!({
            "document": "documents/mcp-registered",
            "selectors": ["root"]
        }),
    );
    call_tool(
        input,
        6,
        "mant_outline",
        &json!({
            "document": "documents/mcp-registered",
            "entries": {"kind": "all"}
        }),
    );
    call_tool(
        input,
        7,
        "mant_explain",
        &json!({
            "documents": ["documents/mcp-registered", "documents/mcp-suffix.exe"],
            "entry": "query"
        }),
    );
    call_tool(
        input,
        8,
        "mant_explain",
        &json!({
            "documents": ["documents/mcp-registered"],
            "entry": "/f"
        }),
    );
    call_tool(
        input,
        15,
        "mant_explain",
        &json!({
            "documents": ["documents/mcp-registered"],
            "entry": "VISUAL"
        }),
    );
    call_tool(
        input,
        16,
        "mant_outline",
        &json!({
            "document": "documents/mcp-suffix.exe",
            "entries": {
                "kind": "kinds",
                "kinds": [{"kind": "environment-variable"}]
            }
        }),
    );
    call_tool(
        input,
        10,
        "mant_search",
        &json!({
            "documents": ["documents/mcp-registered"],
            "followLinks": true,
            "maxDepth": 0,
            "pattern": "needle"
        }),
    );
    request_compatibility_and_page_tools(input);
    #[cfg(windows)]
    call_tool(
        input,
        9,
        "mant_outline",
        &json!({ "document": "mcp-suffix" }),
    );
}

fn request_compatibility_and_page_tools(input: &mut impl Write) {
    call_tool(
        input,
        17,
        "mant_explain",
        &json!({
            "documents": ["documents/mcp-suffix.exe"],
            "entry": "missing-entry"
        }),
    );
    call_tool(
        input,
        11,
        "mant_search",
        &json!({
            "documents": "[\"documents/mcp-registered\"]",
            "pattern": "needle",
            "word": "true",
            "contextLines": "1",
            "maxMatches": "1",
            "scope": "markdown",
            "offset": "1"
        }),
    );
    call_tool(
        input,
        12,
        "mant_read",
        &json!({
            "document": "documents/mcp-registered",
            "selectors": "[\"root\",\"1\"]"
        }),
    );
    call_tool(
        input,
        13,
        "mant_explain",
        &json!({
            "documents": "[\"documents/mcp-registered\",\"documents/mcp-suffix.exe\"]",
            "entry": "query"
        }),
    );
    call_tool(
        input,
        14,
        "mant_read",
        &json!({
            "document": "documents/mcp-registered",
            "selectors": ["root"],
            "startChar": 3,
            "maxChars": 7
        }),
    );
}

fn assert_tool_catalog(tools: &[Value]) {
    let mut names = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "mant_explain",
            "mant_find",
            "mant_outline",
            "mant_read",
            "mant_search",
        ]
    );
    for tool in tools {
        assert!(tool["inputSchema"]["properties"].is_object());
        assert!(tool.get("outputSchema").is_none());
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
        assert!(tool["inputSchema"]["properties"]["startChar"].is_object());
        assert!(tool["inputSchema"]["properties"]["maxChars"].is_object());
        assert!(tool["inputSchema"]["properties"].get("cursor").is_none());
        if matches!(tool["name"].as_str(), Some("mant_outline" | "mant_read")) {
            assert!(tool["inputSchema"]["properties"]["document"].is_object());
            assert!(tool["inputSchema"]["properties"].get("name").is_none());
            assert!(
                tool["inputSchema"]["properties"]
                    .get("manualSection")
                    .is_none()
            );
        }
        if matches!(tool["name"].as_str(), Some("mant_explain" | "mant_search")) {
            assert!(tool["inputSchema"]["properties"]["documents"].is_object());
            assert!(tool["inputSchema"]["properties"]["followLinks"].is_object());
            assert!(tool["inputSchema"]["properties"].get("document").is_none());
        }
        if tool["name"] == "mant_find" {
            assert!(tool["inputSchema"]["properties"]["maxResults"].is_object());
            assert!(tool["inputSchema"]["properties"]["syntax"].is_object());
            assert!(tool["inputSchema"]["properties"]["case"].is_object());
            assert!(tool["inputSchema"]["properties"]["offset"].is_object());
        }
        if tool["name"] == "mant_search" {
            assert!(tool["inputSchema"]["properties"]["maxMatches"].is_object());
            assert!(tool["inputSchema"]["properties"]["scope"].is_object());
            assert!(tool["inputSchema"]["properties"]["offset"].is_object());
            assert!(tool["inputSchema"]["properties"].get("limit").is_none());
        }
    }
}

fn assert_tool_replies(replies: &[Value]) {
    let find = reply(replies, 3);
    let find = successful_text(find);
    assert_page_header(find);
    assert!(find.contains("documents/mcp-registered\tmarkdown"));
    assert!(find.contains("manual/1/mcp-manual\tmanual"));
    assert!(find.contains("documents/mcp-suffix.exe\tmarkdown"));

    let search = successful_text(reply(replies, 4));
    assert_page_header(search);
    assert!(search.contains("needle"));
    assert_eq!(search.matches("Outline root").count(), 1);
    assert_eq!(search.matches("needle").count(), 1);
    assert!(
        search.contains("[search: offset=0, returned=1, totalMatchingLineGroups=2, nextOffset=1]"),
        "{search}"
    );
    assert!(!search.contains("--offset"), "{search}");
    assert!(!search.contains("total matching lines"), "{search}");

    let read = successful_text(reply(replies, 5));
    assert_page_header(read);
    assert!(read.contains("# MCP registered"), "{read}");
    assert!(read.contains("Read the MCP needle."));

    let outline = successful_text(reply(replies, 6));
    assert_page_header(outline);
    assert!(outline.contains("[command-query] query"));
    assert!(outline.contains("[option-s] /S"));
    assert!(outline.contains("[environment-path] PATH, $env:PATH"));
    assert!(!outline.contains("mant.outline/v0.10"));

    let explain = successful_text(reply(replies, 7));
    assert_page_header(explain);
    assert!(explain.contains("Query registry data."));
    assert!(
        explain.contains("[explain: matched=1, missed=1, failed=0]"),
        "{explain}"
    );

    let ambiguity = successful_text(reply(replies, 8));
    assert!(ambiguity.contains("option-f"), "{ambiguity}");
    assert!(ambiguity.contains("option-f-2"), "{ambiguity}");

    let probe = successful_text(reply(replies, 15));
    assert!(probe.contains("has no semantic entry 'VISUAL'"), "{probe}");
    assert!(probe.contains("outline node 1 (Query) at line"), "{probe}");
    assert!(
        probe.contains("[explain: matched=0, missed=0, failed=1]"),
        "{probe}"
    );

    assert_empty_outline(replies);
    assert_missing_explain_guidance(replies);

    let bounded = successful_text(reply(replies, 10));
    assert!(
        bounded.contains("[scope: documents=1, unresolved-roots=0, unresolved-links=0, depth-frontier=1, document-frontier=0, content-frontier=0]"),
        "{bounded}"
    );

    let compatible_search = successful_text(reply(replies, 11));
    assert!(compatible_search.contains("needle"), "{compatible_search}");
    assert!(
        compatible_search.contains("[search: offset=1, returned=1, totalMatchingLineGroups=2]"),
        "{compatible_search}"
    );
    let compatible_read = successful_text(reply(replies, 12));
    assert!(
        compatible_read.contains("Read the MCP needle."),
        "{compatible_read}"
    );
    assert!(
        compatible_read.contains("General query behavior."),
        "{compatible_read}"
    );
    let compatible_explain = successful_text(reply(replies, 13));
    assert!(
        compatible_explain.contains("Query registry data."),
        "{compatible_explain}"
    );
    let random_page = successful_text(reply(replies, 14));
    assert!(
        random_page.starts_with("[mant-page chars=3..10 totalChars="),
        "{random_page}"
    );
    assert_eq!(
        random_page
            .split_once("\n\n")
            .expect("page body")
            .1
            .chars()
            .count(),
        7
    );

    #[cfg(windows)]
    assert!(successful_text(reply(replies, 9)).contains("Suffix details"));

    for response in replies {
        assert!(response["result"].get("structuredContent").is_none());
        assert!(response.to_string().len() < 34_000);
        let encoded = response.to_string();
        assert!(!encoded.contains("/home/"));
        assert!(!encoded.contains(r"C:\\Users"));
        assert!(!encoded.contains("sourcePath"));
        assert!(!encoded.contains('\u{1b}'));
    }
}

fn assert_empty_outline(replies: &[Value]) {
    let empty_outline = successful_text(reply(replies, 16));
    assert!(
        empty_outline.contains("0 matching semantic entries for: environment variables"),
        "{empty_outline}"
    );
    assert!(!empty_outline.contains("Suffix details"), "{empty_outline}");
}

fn assert_missing_explain_guidance(replies: &[Value]) {
    let missing = successful_text(reply(replies, 17));
    assert!(
        missing.contains("call mant_outline(document=\"documents/mcp-suffix.exe\"")
            && missing.contains("entries={\"kind\":\"all\"}")
            && missing.contains("call mant_search"),
        "{missing}"
    );
    assert!(!missing.contains("--format json"), "{missing}");
    assert!(
        missing.contains("[explain: matched=0, missed=1, failed=0]"),
        "{missing}"
    );
}

fn assert_page_header(text: &str) {
    assert!(text.starts_with("[mant-page chars="), "{text}");
    assert!(
        text.lines()
            .next()
            .is_some_and(|line| line.contains("totalChars="))
    );
}

fn successful_text(reply: &Value) -> &str {
    assert_ne!(reply["result"]["isError"], true);
    result_text(reply)
}

fn result_text(reply: &Value) -> &str {
    let content = reply["result"]["content"].as_array().expect("tool content");
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    content[0]["text"].as_str().expect("text result")
}

fn reply(replies: &[Value], id: u8) -> &Value {
    replies
        .iter()
        .find(|reply| reply["id"] == id)
        .unwrap_or_else(|| panic!("missing reply {id}"))
}

fn initialize(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "mant-test", "version": "0" }
            }
        }),
    );
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
    );
}

fn request_tool_list(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );
}

fn call_tool(input: &mut impl Write, id: u8, name: &str, arguments: &Value) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }),
    );
}

fn write_message(input: &mut impl Write, message: &Value) {
    writeln!(input, "{message}").expect("write MCP request");
}

fn assert_silent_shutdown(
    mut child: std::process::Child,
    input: std::process::ChildStdin,
    diagnostics: std::process::ChildStderr,
    data_home: PathBuf,
) {
    drop(input);
    let status = child.wait().expect("MCP server exit");
    let diagnostics = BufReader::new(diagnostics)
        .lines()
        .collect::<Result<Vec<_>, _>>()
        .expect("read MCP stderr");
    fs::remove_dir_all(data_home).expect("remove registered document fixture");
    assert!(status.success(), "MCP server should stop cleanly: {status}");
    assert!(
        diagnostics.is_empty(),
        "MCP must not emit lowering or transport noise: {diagnostics:?}"
    );
}

fn registered_document_fixture() -> PathBuf {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-mcp-registered-document-{}",
        std::process::id()
    ));
    let documents = registered_documents_dir(&fixture_root);
    fs::create_dir_all(&documents).expect("create document directory");
    fs::write(
        documents.join("mcp-registered.md"),
        "# MCP registered\n\nRead the MCP needle.\n\n[Linked details](mcp-linked.md)\n\nA second needle stays in the same outline node.\n\n> preserved unsupported quote\n\n## Query\n\nGeneral query behavior. The VISUAL name appears only in prose.\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force a query.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Query registry data.\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/S COMPUTER`: Select a remote computer.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n\n## Invalid declaration\n\n<!-- mant:entries role=option case=insensitive -->\n- `/driver..exclude`: Keep malformed entries out of the outline.\n",
    )
    .expect("write registered document");
    fs::write(
        documents.join("mcp-linked.md"),
        "# MCP linked\n\nA linked document beyond the requested depth.\n",
    )
    .expect("write linked registered document");
    fs::write(
        documents.join("mcp-suffix.exe.md"),
        "# MCP suffixed executable\n\n## Suffix details\n\nWindows suffix fallback.\n",
    )
    .expect("write suffixed registered document");
    let manual_section = fixture_root.join("manuals/man1");
    fs::create_dir_all(&manual_section).expect("create manual section");
    fs::write(
        manual_section.join("mcp-manual.1"),
        ".TH MCP-MANUAL 1\n.SH NAME\nmcp-manual \\- native MCP discovery\n",
    )
    .expect("write manual document");
    fixture_root
}

fn parse_reply(line: Result<String, std::io::Error>) -> Value {
    let line = line.expect("MCP reply line");
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("invalid MCP JSON {line:?}: {error}"))
}
