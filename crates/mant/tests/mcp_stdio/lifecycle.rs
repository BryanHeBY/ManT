//! Real-process lifecycle coverage for the SDK boundary, not an SDK mock.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use rmcp::model::ProtocolVersion;
use serde_json::{Value, json};

use super::{assert_tool_catalog, support};

const TIMEOUT: Duration = Duration::from_secs(15);

struct Session {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<Result<Value, String>>,
    stderr: Receiver<Vec<u8>>,
    root: PathBuf,
}

impl Session {
    fn start(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mant-mcp-lifecycle-{label}-{}", std::process::id()));
        let documents = support::registered_documents_dir(&root);
        fs::create_dir_all(&documents).expect("create lifecycle fixture");
        fs::write(
            documents.join("lifecycle.md"),
            "# Lifecycle\n\nSession is still usable.\n",
        )
        .expect("write lifecycle fixture");
        let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
        support::configure_registered_documents(&mut command, &root);
        let mut child = command
            .arg("--mcp")
            .env("MANT_MANPATH", root.join("empty-manuals"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start lifecycle MCP process");
        let input = child.stdin.take().expect("MCP stdin");
        let stdout = child.stdout.take().expect("MCP stdout");
        let mut stderr = child.stderr.take().expect("MCP stderr");
        let (reply_tx, replies) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let reply = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str(&line)
                        .map_err(|error| format!("non-protocol stdout: {error}: {line}"))
                });
                if reply_tx.send(reply).is_err() {
                    break;
                }
            }
        });
        let (stderr_tx, stderr_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr
                .read_to_end(&mut bytes)
                .expect("read lifecycle stderr");
            let _ = stderr_tx.send(bytes);
        });
        Self {
            child,
            input: Some(input),
            replies,
            stderr: stderr_rx,
            root,
        }
    }

    fn send(&mut self, message: &Value) {
        let input = self.input.as_mut().expect("open MCP input");
        writeln!(input, "{message}").expect("write lifecycle message");
        input.flush().expect("flush lifecycle message");
    }

    fn receive(&self) -> Value {
        let reply = self
            .replies
            .recv_timeout(TIMEOUT)
            .expect("bounded MCP response wait")
            .expect("stdout must contain only JSON-RPC");
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        reply
    }

    fn initialize(&mut self, requested: &ProtocolVersion, expected: &ProtocolVersion) {
        self.send(
            &json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
                "protocolVersion":requested.as_str(), "capabilities":{},
                "clientInfo":{"name":"mant-lifecycle-test", "version":"0"}
            }}),
        );
        let reply = self.receive();
        assert_eq!(reply["id"], 1);
        assert_eq!(
            reply["result"]["protocolVersion"],
            expected.as_str(),
            "{reply}"
        );
        assert_eq!(reply["result"]["serverInfo"]["name"], "mant");
        self.send(&json!({"jsonrpc":"2.0", "method":"notifications/initialized"}));
    }

    fn list_tools(&mut self, id: u32) {
        self.send(&json!({"jsonrpc":"2.0", "id":id, "method":"tools/list", "params":{}}));
    }

    fn read_document(&mut self, id: u32) {
        self.send(
            &json!({"jsonrpc":"2.0", "id":id, "method":"tools/call", "params":{
            "name":"mant_read", "arguments":{"document":"lifecycle",
                "selectors":[{"kind":"path", "path":"root"}], "maxChars":256}
            }}),
        );
    }

    fn cancel(&mut self, id: u32) {
        self.send(
            &json!({"jsonrpc":"2.0", "method":"notifications/cancelled", "params":{
                "requestId":id, "reason":"lifecycle regression test"
            }}),
        );
    }

    fn close(mut self) -> Vec<Value> {
        drop(self.input.take());
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("poll MCP shutdown") {
                break status;
            }
            assert!(Instant::now() < deadline, "MCP must exit after stdin EOF");
            thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(), "MCP EOF shutdown: {status}");
        let diagnostics = self
            .stderr
            .recv_timeout(TIMEOUT)
            .expect("stderr closes after EOF");
        assert!(
            diagnostics.is_empty(),
            "MCP stderr must stay silent: {diagnostics:?}"
        );
        let mut remaining = Vec::new();
        loop {
            match self.replies.recv_timeout(TIMEOUT) {
                Ok(reply) => {
                    let reply = reply.expect("remaining stdout must be JSON-RPC");
                    assert_eq!(reply["jsonrpc"], "2.0");
                    remaining.push(reply);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("MCP stdout must close after EOF"),
            }
        }
        remaining
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A failed assertion or bounded wait must not leave a server behind.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn supported_legacy_versions_keep_stdio_usable_after_cancellation() {
    // These are the SDK's known initialization-era revisions, not guessed
    // future protocol dates. The 2026 revision uses a different lifecycle.
    for version in [
        ProtocolVersion::V_2024_11_05,
        ProtocolVersion::V_2025_03_26,
        ProtocolVersion::V_2025_06_18,
        ProtocolVersion::V_2025_11_25,
    ] {
        let mut session = Session::start(version.as_str());
        session.initialize(&version, &version);
        session.list_tools(2);
        let tools = session.receive();
        assert_eq!(tools["id"], 2);
        assert_tool_catalog(
            tools["result"]["tools"]
                .as_array()
                .expect("legacy tools/list"),
        );

        session.read_document(3);
        session.cancel(3);
        session.cancel(2); // A completed request must not cancel the session.
        session.cancel(999); // Unknown request cancellation is also harmless.
        session.list_tools(4);
        let mut raced_replies = Vec::new();
        loop {
            let reply = session.receive();
            if reply["id"] == 4 {
                assert_tool_catalog(
                    reply["result"]["tools"]
                        .as_array()
                        .expect("tools after cancellation"),
                );
                break;
            }
            assert_eq!(
                reply["id"], 3,
                "unexpected response while listing tools: {reply}"
            );
            assert!(
                raced_replies.is_empty(),
                "duplicate cancelled-request response"
            );
            raced_replies.push(reply);
        }
        session.read_document(5);
        loop {
            let reply = session.receive();
            if reply["id"] == 5 {
                assert_ne!(reply["result"]["isError"], true, "{reply}");
                let text = super::result_text(&reply);
                assert!(text.contains("Session is still usable."), "{text}");
                break;
            }
            assert_eq!(reply["id"], 3, "unexpected response while reading: {reply}");
            assert!(
                raced_replies.is_empty(),
                "duplicate cancelled-request response"
            );
            raced_replies.push(reply);
        }
        raced_replies.extend(session.close());
        // Request 3 may finish before cancellation reaches it. We test session
        // recovery, not scheduler timing or interruption of a blocking parser.
        assert!(raced_replies.len() <= 1, "{raced_replies:?}");
        assert!(
            raced_replies.iter().all(|reply| reply["id"] == 3),
            "{raced_replies:?}"
        );
    }
}

#[test]
fn initialize_with_modern_version_still_negotiates_legacy_lifecycle() {
    // rmcp 3.2.0 / upstream #1228: initialize always selects legacy semantics,
    // even when the requested date is a known discovery-era version.
    let mut session = Session::start("modern-initialize");
    session.initialize(
        &ProtocolVersion::V_2026_07_28,
        &ProtocolVersion::V_2025_11_25,
    );
    session.list_tools(2);
    let reply = session.receive();
    assert_eq!(reply["id"], 2);
    assert_tool_catalog(
        reply["result"]["tools"]
            .as_array()
            .expect("negotiated tools/list"),
    );
    assert!(session.close().is_empty());
}
