//! Verifies the public `mant --mcp` stdio handshake without a local man page.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

/// Start the real binary, negotiate MCP, and inspect its discoverable tools.
#[test]
fn stdio_mode_exposes_read_only_document_tools_and_queries_markdown() {
    let executable = env!("CARGO_BIN_EXE_mant");
    let mut child = Command::new(executable)
        .arg("--mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant MCP server");
    let mut input = child.stdin.take().expect("MCP stdin");
    let output = child.stdout.take().expect("MCP stdout");

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
        ]
    );
    for tool in tools {
        assert!(tool["inputSchema"]["properties"].is_object());
        assert!(tool["inputSchema"]["properties"]["target"].is_object());
        assert!(tool["outputSchema"].is_object());
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
    }

    // Exercise the new source branch without depending on whichever manuals
    // the host provides.
    let markdown_path = markdown_fixture_path();
    fs::write(
        &markdown_path,
        "Read the MCP needle.\n\n# Guide\n\nDocument body.\n",
    )
    .expect("write Markdown fixture");
    request_markdown_search(&mut input, &markdown_path);
    input.flush().expect("flush tool call");

    let search = parse_reply(lines.next().expect("tool search reply"));
    assert_eq!(search["id"], 3);
    assert_ne!(search["result"]["isError"], true);
    assert_eq!(search["result"]["structuredContent"]["total"], 1);
    assert_eq!(
        search["result"]["structuredContent"]["matches"][0]["node"]["kind"],
        "document-root"
    );

    drop(input);
    let status = child.wait().expect("MCP server exit");
    fs::remove_file(markdown_path).expect("remove Markdown fixture");
    assert!(status.success(), "MCP server should stop cleanly: {status}");
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

fn request_markdown_search(input: &mut impl Write, path: &PathBuf) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "mant_document_search",
                "arguments": {
                    "target": {
                        "kind": "markdown-file",
                        "path": path,
                    },
                    "pattern": "needle"
                }
            }
        }),
    );
}

fn write_message(input: &mut impl Write, message: &Value) {
    writeln!(input, "{message}").expect("write MCP request");
}

fn markdown_fixture_path() -> PathBuf {
    std::env::temp_dir().join(format!("mant-mcp-markdown-{}.md", std::process::id()))
}

fn parse_reply(line: Result<String, std::io::Error>) -> Value {
    let line = line.expect("MCP reply line");
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("invalid MCP JSON {line:?}: {error}"))
}
