//! Verifies topic-only MCP queries without depending on a local man page.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

/// Start the real binary, negotiate MCP, and inspect its discoverable tools.
#[test]
fn stdio_mode_lists_and_queries_registered_markdown_topics() {
    let executable = env!("CARGO_BIN_EXE_mant");
    let data_home = registered_topic_fixture();
    let mut child = Command::new(executable)
        .arg("--mcp")
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_DATA_DIRS", data_home.join("empty-system-data"))
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
            "mant_topics_list",
        ]
    );
    for tool in tools {
        assert!(tool["inputSchema"]["properties"].is_object());
        assert!(tool["outputSchema"].is_object());
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
    }

    request_topic_list(&mut input);
    request_topic_search(&mut input);
    request_topic_get(&mut input);
    input.flush().expect("flush tool call");

    // JSON-RPC permits concurrent requests to complete out of order. Select
    // replies by ID instead of treating stdio arrival order as a contract.
    let replies = [
        parse_reply(lines.next().expect("first tool reply")),
        parse_reply(lines.next().expect("second tool reply")),
        parse_reply(lines.next().expect("third tool reply")),
    ];
    let catalog = replies
        .iter()
        .find(|reply| reply["id"] == 3)
        .expect("topic list reply");
    assert_eq!(
        catalog["result"]["structuredContent"]["topics"][0]["name"],
        "mcp-registered"
    );
    assert_eq!(
        catalog["result"]["structuredContent"]["topics"][0]["origin"],
        "user"
    );
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

    drop(input);
    let status = child.wait().expect("MCP server exit");
    fs::remove_dir_all(data_home).expect("remove registered topic fixture");
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

fn request_topic_list(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "mant_topics_list",
                "arguments": {}
            }
        }),
    );
}

fn request_topic_search(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "mant_document_search",
                "arguments": {
                    "topic": "mcp-registered",
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

fn request_topic_get(input: &mut impl Write) {
    write_message(
        input,
        &json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "mant_document_get",
                "arguments": {
                    "topic": "mcp-registered",
                    "nodes": "[\"root\"]"
                }
            }
        }),
    );
}

fn write_message(input: &mut impl Write, message: &Value) {
    writeln!(input, "{message}").expect("write MCP request");
}

fn registered_topic_fixture() -> PathBuf {
    let data_home =
        std::env::temp_dir().join(format!("mant-mcp-registered-topic-{}", std::process::id()));
    let topics = data_home.join("mant/topics");
    fs::create_dir_all(&topics).expect("create topic directory");
    fs::write(
        topics.join("mcp-registered.md"),
        "Read the MCP needle.\n\n# Guide\n\nDocument body.\n",
    )
    .expect("write registered topic");
    data_home
}

fn parse_reply(line: Result<String, std::io::Error>) -> Value {
    let line = line.expect("MCP reply line");
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("invalid MCP JSON {line:?}: {error}"))
}
