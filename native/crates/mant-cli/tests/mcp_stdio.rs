//! Verifies the public `mant-cli --mcp` stdio handshake without a local man page.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use serde_json::{Value, json};

/// Start the real binary, negotiate MCP, and inspect its discoverable tools.
#[test]
fn stdio_mode_exposes_only_read_only_manual_tools() {
    let executable = env!("CARGO_BIN_EXE_mant-cli");
    let mut child = Command::new(executable)
        .arg("--mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant-cli MCP server");
    let mut input = child.stdin.take().expect("MCP stdin");
    let output = child.stdout.take().expect("MCP stdout");

    writeln!(
        input,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "mant-test", "version": "0" }
            }
        })
    )
    .expect("write initialize request");
    writeln!(
        input,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        })
    )
    .expect("write initialized notification");
    writeln!(
        input,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        })
    )
    .expect("write tools list request");
    input.flush().expect("flush MCP requests");

    let mut lines = BufReader::new(output).lines();
    let initialization = parse_reply(lines.next().expect("initialization reply"));
    assert_eq!(initialization["id"], 1);
    assert_eq!(initialization["result"]["serverInfo"]["name"], "mant-cli");

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
            "mant_manual_explain",
            "mant_manual_get",
            "mant_manual_outline",
            "mant_manual_search",
        ]
    );
    for tool in tools {
        assert!(tool["inputSchema"]["properties"].is_object());
        assert!(tool["inputSchema"]["properties"]["topic"].is_object());
        assert!(tool["outputSchema"].is_object());
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
    }

    // Use a guaranteed-missing topic: this reaches the native query worker
    // without making the test depend on whichever manuals the host provides.
    writeln!(
        input,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "mant_manual_search",
                "arguments": {
                    "topic": "__mant_missing_manual_for_mcp_test__",
                    "pattern": "needle"
                }
            }
        })
    )
    .expect("write tool call");
    input.flush().expect("flush tool call");

    let failed_query = parse_reply(lines.next().expect("tool error reply"));
    assert_eq!(failed_query["id"], 3);
    assert_eq!(failed_query["result"]["isError"], true);
    assert!(failed_query["result"]["content"].is_array());

    drop(input);
    let status = child.wait().expect("MCP server exit");
    assert!(status.success(), "MCP server should stop cleanly: {status}");
}

fn parse_reply(line: Result<String, std::io::Error>) -> Value {
    let line = line.expect("MCP reply line");
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("invalid MCP JSON {line:?}: {error}"))
}
