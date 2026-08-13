//! Verifies MCP discovery and registered-document queries over real stdio.

mod support;

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

use support::{configure_registered_documents, registered_documents_dir};

/// Start the real binary, negotiate MCP, and inspect its discoverable tools.
#[test]
fn stdio_mode_lists_and_queries_registered_markdown_documents() {
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

    let tools = parse_reply(lines.next().expect("tools list reply"));
    assert_eq!(tools["id"], 2);
    let tools = tools["result"]["tools"].as_array().expect("tool list");
    assert_tool_catalog(tools);

    request_document_list(&mut input);
    request_document_search(&mut input);
    request_document_get(&mut input);
    request_document_outline(&mut input);
    request_document_explain(&mut input, 7, "query");
    request_document_explain(&mut input, 8, "/s");
    request_document_explain(&mut input, 9, "path");
    request_document_explain(&mut input, 10, "/f");
    request_document_explain(&mut input, 11, "option-f-2");
    #[cfg(windows)]
    request_named_document_outline(&mut input, 12, "mcp-suffix");
    input.flush().expect("flush tool call");

    // JSON-RPC permits concurrent requests to complete out of order. Select
    // replies by ID instead of treating stdio arrival order as a contract.
    let replies = (0..(9 + usize::from(cfg!(windows))))
        .map(|_| parse_reply(lines.next().expect("tool reply")))
        .collect::<Vec<_>>();
    assert_tool_replies(&replies);

    assert_silent_shutdown(child, input, diagnostics, fixture_root);
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
            "mant_document_explain",
            "mant_document_get",
            "mant_document_outline",
            "mant_document_search",
            "mant_documents_list",
        ]
    );
    for tool in tools {
        assert!(tool["inputSchema"]["properties"].is_object());
        assert!(tool["outputSchema"].is_object());
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
    }
}

fn assert_tool_replies(replies: &[Value]) {
    assert_document_catalog(replies);
    assert_windows_suffix_reply(replies);
    let search = replies
        .iter()
        .find(|reply| reply["id"] == 4)
        .expect("tool search reply");
    assert_eq!(search["id"], 4);
    assert_ne!(search["result"]["isError"], true);
    assert_eq!(search["result"]["structuredContent"]["total"], 1);
    assert_eq!(
        search["result"]["structuredContent"]["matches"][0]["node"]["kind"],
        "document-root"
    );

    let excerpt = replies
        .iter()
        .find(|reply| reply["id"] == 5)
        .expect("tool get reply");
    assert_eq!(excerpt["id"], 5);
    assert_ne!(excerpt["result"]["isError"], true);
    assert_eq!(
        excerpt["result"]["structuredContent"]["selections"][0]["kind"],
        "document-root"
    );
    assert_eq!(
        excerpt["result"]["structuredContent"]["selections"][0]["path"],
        "root"
    );
    assert!(
        excerpt["result"]["structuredContent"]
            .get("diagnostics")
            .is_none(),
        "MCP excerpts must discard lowering diagnostics"
    );

    assert_semantic_replies(replies);
}

fn assert_document_catalog(replies: &[Value]) {
    let catalog = replies
        .iter()
        .find(|reply| reply["id"] == 3)
        .expect("document list reply");
    let documents = catalog["result"]["structuredContent"]["documents"]
        .as_array()
        .expect("document catalog");
    assert_eq!(documents.len(), 3);
    assert!(documents.iter().any(|document| {
        document["address"]["path"] == "mcp-registered"
            && document["address"]["kind"] == "markdown"
            && document["address"]["origin"]["kind"] == "documents"
    }));
    assert!(documents.iter().any(|document| {
        document["address"]["name"] == "mcp-manual"
            && document["address"]["kind"] == "manual"
            && document["address"]["manualSection"] == "1"
    }));
    assert_eq!(
        documents
            .iter()
            .filter(|document| document["address"]["path"] == "mcp-suffix.exe")
            .count(),
        1,
        "listing must expose one canonical suffixed name"
    );
}

fn assert_semantic_replies(replies: &[Value]) {
    let outline = replies
        .iter()
        .find(|reply| reply["id"] == 6)
        .expect("semantic outline reply");
    assert_eq!(
        outline["result"]["structuredContent"]["schema"],
        "mant.outline/v7"
    );
    let encoded = outline["result"]["structuredContent"].to_string();
    for role in ["option", "command", "environment-variable"] {
        assert!(encoded.contains(&format!("\"role\":\"{role}\"")));
    }
    assert_eq!(
        outline["result"]["structuredContent"]["entriesComplete"],
        false
    );
    assert!(
        outline["result"]["structuredContent"]
            .get("diagnostics")
            .is_none(),
        "MCP outlines must expose completeness without lowering diagnostics"
    );

    for (id, role) in [
        (7, "command"),
        (8, "option"),
        (9, "environment-variable"),
        (11, "option"),
    ] {
        let explanation = replies
            .iter()
            .find(|reply| reply["id"] == id)
            .expect("semantic explanation reply");
        assert_ne!(explanation["result"]["isError"], true);
        assert_eq!(
            explanation["result"]["structuredContent"]["selections"][0]["entry"]["identity"]["role"],
            role
        );
    }
    let ambiguity = replies
        .iter()
        .find(|reply| reply["id"] == 10)
        .expect("ambiguous explanation reply");
    assert_eq!(ambiguity["result"]["isError"], true);
    let encoded = ambiguity["result"].to_string();
    assert!(encoded.contains("option-f"));
    assert!(encoded.contains("option-f-2"));
}

#[cfg(windows)]
fn assert_windows_suffix_reply(replies: &[Value]) {
    let suffix = replies
        .iter()
        .find(|reply| reply["id"] == 12)
        .expect("Windows suffix outline reply");
    assert_ne!(suffix["result"]["isError"], true);
    assert!(
        suffix["result"]["structuredContent"]["source"]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("mcp-suffix.exe.md"))
    );
}

#[cfg(not(windows))]
fn assert_windows_suffix_reply(_replies: &[Value]) {}

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

fn request_document_list(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "mant_documents_list",
                "arguments": {
                    "query": "mcp-",
                    "limit": 10
                }
            }
        }),
    );
}

fn request_document_search(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "mant_document_search",
                "arguments": {
                    "name": "mcp-registered",
                    "pattern": "needle",
                    "word": "True",
                    "context_lines": "1",
                    "limit": "10",
                    "offset": "0"
                }
            }
        }),
    );
}

fn request_document_get(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "mant_document_get",
                "arguments": {
                    "name": "mcp-registered",
                    "selectors": "[\"root\"]"
                }
            }
        }),
    );
}

fn request_document_outline(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "mant_document_outline",
                "arguments": {
                    "name": "mcp-registered",
                    "detail": "entries"
                }
            }
        }),
    );
}

#[cfg(windows)]
fn request_named_document_outline(input: &mut impl Write, id: u8, name: &str) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "mant_document_outline",
                "arguments": {
                    "name": name,
                    "detail": "entries"
                }
            }
        }),
    );
}

fn request_document_explain(input: &mut impl Write, id: u8, entry: &str) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "mant_document_explain",
                "arguments": {
                    "name": "mcp-registered",
                    "entry": entry
                }
            }
        }),
    );
}

fn write_message(input: &mut impl Write, message: &Value) {
    writeln!(input, "{message}").expect("write MCP request");
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
        "# MCP registered\n\nRead the MCP needle.\n\n> preserved unsupported quote\n\n## Query\n\nGeneral query behavior.\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force a query.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Query registry data.\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/S COMPUTER`: Select a remote computer.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n\n## Invalid declaration\n\n<!-- mant:entries role=option case=insensitive -->\n- `/driver..exclude`: Keep malformed entries out of the outline.\n",
    )
    .expect("write registered document");
    fs::write(
        documents.join("mcp-suffix.exe.md"),
        "# MCP suffixed executable\n\nWindows suffix fallback.\n",
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
