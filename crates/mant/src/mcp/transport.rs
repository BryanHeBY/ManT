//! Recoverable, bounded stdio framing for the local MCP subprocess.

use std::io;

use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

use super::{MantMcpServer, presentation::finish_error};

/// Upper bound on one newline-delimited MCP request, in bytes.
pub(super) const MAX_MCP_LINE_BYTES: usize = 256 * 1024;

const PROXY_CAPACITY: usize = 64 * 1024;
const ERROR_QUEUE_CAPACITY: usize = 16;

/// Run the MCP server until the peer closes its standard-input stream.
pub(crate) async fn run_stdio() -> u8 {
    let (input_proxy, server_input) = tokio::io::duplex(PROXY_CAPACITY);
    let (server_output, output_proxy) = tokio::io::duplex(PROXY_CAPACITY);
    let (error_sender, error_receiver) = mpsc::channel(ERROR_QUEUE_CAPACITY);

    let input_task = tokio::spawn(proxy_input(
        tokio::io::stdin(),
        input_proxy,
        error_sender,
        MAX_MCP_LINE_BYTES,
    ));
    let output_task = tokio::spawn(proxy_output(
        output_proxy,
        error_receiver,
        tokio::io::stdout(),
    ));

    let Ok(service) = MantMcpServer::new()
        .serve((server_input, server_output))
        .await
    else {
        input_task.abort();
        output_task.abort();
        return 1;
    };
    let service_result = service.waiting().await;
    let input_result = input_task.await;
    let output_result = output_task.await;
    u8::from(
        !(service_result.is_ok()
            && matches!(input_result, Ok(Ok(())))
            && matches!(output_result, Ok(Ok(())))),
    )
}

async fn proxy_input<R, W>(
    mut source: R,
    mut destination: W,
    errors: mpsc::Sender<Vec<u8>>,
    max_line: usize,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = [0_u8; 8 * 1024];
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let count = source.read(&mut buffer).await?;
        if count == 0 {
            if !line.is_empty() || oversized {
                forward_input_line(&line, oversized, &mut destination, &errors).await?;
            }
            destination.shutdown().await?;
            return Ok(());
        }
        for &byte in &buffer[..count] {
            if byte == b'\n' {
                forward_input_line(&line, oversized, &mut destination, &errors).await?;
                line.clear();
                oversized = false;
            } else if line.len() < max_line {
                line.push(byte);
            } else {
                oversized = true;
            }
        }
    }
}

async fn forward_input_line<W>(
    line: &[u8],
    oversized: bool,
    destination: &mut W,
    errors: &mpsc::Sender<Vec<u8>>,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if oversized {
        return send_input_error(
            errors,
            line,
            -32600,
            "MCP request line exceeds the 262144-byte limit",
        )
        .await;
    }
    match serde_json::from_slice::<Value>(line) {
        Ok(_) => {
            destination.write_all(line).await?;
            destination.write_all(b"\n").await
        }
        Err(error) => {
            let recursion = error.to_string().contains("recursion limit exceeded");
            send_input_error(
                errors,
                line,
                if recursion { -32600 } else { -32700 },
                if recursion {
                    "MCP request exceeds the supported JSON nesting depth"
                } else {
                    "MCP request is not valid JSON"
                },
            )
            .await
        }
    }
}

async fn send_input_error(
    errors: &mpsc::Sender<Vec<u8>>,
    line: &[u8],
    code: i32,
    message: &str,
) -> io::Result<()> {
    let id = crate::json_boundary::top_level_value(line, "id")
        .filter(|id| id.is_null() || id.is_string() || id.is_number())
        .unwrap_or(Value::Null);
    let mut encoded = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    }))
    .expect("JSON-RPC error serialization is infallible");
    encoded.push(b'\n');
    errors
        .send(encoded)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "MCP output proxy closed"))
}

async fn proxy_output<R, W>(
    mut source: R,
    mut errors: mpsc::Receiver<Vec<u8>>,
    mut destination: W,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut source_open = true;
    let mut errors_open = true;
    let mut pending = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    while source_open || errors_open {
        tokio::select! {
            read = source.read(&mut buffer), if source_open => {
                let count = read?;
                if count == 0 {
                    source_open = false;
                    if !pending.is_empty() {
                        destination.write_all(&sanitize_server_line(&pending)).await?;
                        pending.clear();
                    }
                    continue;
                }
                pending.extend_from_slice(&buffer[..count]);
                while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                    let remainder = pending.split_off(end + 1);
                    pending.truncate(end);
                    destination.write_all(&sanitize_server_line(&pending)).await?;
                    pending = remainder;
                }
            }
            error = errors.recv(), if errors_open => {
                match error {
                    Some(error) => destination.write_all(&error).await?,
                    None => errors_open = false,
                }
            }
        }
    }
    destination.flush().await
}

fn sanitize_server_line(line: &[u8]) -> Vec<u8> {
    let Ok(mut response) = serde_json::from_slice::<Value>(line) else {
        let mut retained = line.to_vec();
        retained.push(b'\n');
        return retained;
    };
    if let Some(error) = response.get_mut("error") {
        sanitize_error_strings(error);
    }
    if response.pointer("/result/isError").and_then(Value::as_bool) == Some(true)
        && let Some(result) = response.get_mut("result")
    {
        sanitize_error_strings(result);
    }
    let mut encoded =
        serde_json::to_vec(&response).expect("JSON-RPC response remains serializable");
    encoded.push(b'\n');
    encoded
}

fn sanitize_error_strings(value: &mut Value) {
    match value {
        Value::String(text) => *text = finish_error(text.as_str()),
        Value::Array(values) => values.iter_mut().for_each(sanitize_error_strings),
        Value::Object(values) => values.values_mut().for_each(sanitize_error_strings),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::{proxy_input, sanitize_server_line};

    #[tokio::test]
    async fn oversized_lines_are_reported_and_the_next_line_is_forwarded() {
        let (mut source_writer, source_reader) = tokio::io::duplex(64);
        let (destination_writer, mut destination_reader) = tokio::io::duplex(64);
        let (errors_tx, mut errors_rx) = tokio::sync::mpsc::channel(2);
        let task = tokio::spawn(proxy_input(source_reader, destination_writer, errors_tx, 4));
        source_writer
            .write_all(b"12345\n{}\n")
            .await
            .expect("write input");
        source_writer.shutdown().await.expect("close input");

        let error = errors_rx.recv().await.expect("oversize response");
        let error: serde_json::Value = serde_json::from_slice(&error).expect("error JSON");
        assert_eq!(error["error"]["code"], -32600);
        let mut forwarded = Vec::new();
        destination_reader
            .read_to_end(&mut forwarded)
            .await
            .expect("read forwarded line");
        assert_eq!(forwarded, b"{}\n");
        task.await.expect("join proxy").expect("proxy input");
    }

    #[test]
    fn third_party_tool_errors_share_the_model_text_boundary() {
        let response = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "isError": true,
                "content": [{"type": "text", "text": format!("\u{1b}[31m{}", "x".repeat(40_000))}]
            }
        }))
        .expect("response JSON");

        let sanitized = sanitize_server_line(&response);
        let sanitized: serde_json::Value =
            serde_json::from_slice(&sanitized).expect("sanitized JSON");
        let text = sanitized["result"]["content"][0]["text"]
            .as_str()
            .expect("error text");
        assert!(!text.contains('\u{1b}'));
        assert!(text.chars().count() <= 32_768);
    }
}
