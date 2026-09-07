//! JSONL transport only: profile schemas and semantic oracles stay at callers.
use serde_json::{Value, json};
use std::io::{self, BufRead, BufWriter, Write};

pub(super) fn run(
    schema: &str,
    profile: impl FnMut(&str) -> Result<Value, String>,
    flush_each: bool,
) -> Result<(), String> {
    process(
        io::stdin().lock(),
        BufWriter::new(io::stdout().lock()),
        schema,
        profile,
        flush_each,
    )
}

fn process(
    input: impl BufRead,
    mut output: impl Write,
    schema: &str,
    mut profile: impl FnMut(&str) -> Result<Value, String>,
    flush_each: bool,
) -> Result<(), String> {
    for (index, line) in input.lines().enumerate() {
        let line = line.map_err(|error| format!("read request {}: {error}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = profile(&line).unwrap_or_else(
            |error| json!({ "schema": schema, "id": request_id(&line), "error": error }),
        );
        serde_json::to_writer(&mut output, &response)
            .map_err(|error| format!("encode response {}: {error}", index + 1))?;
        output
            .write_all(b"\n")
            .map_err(|error| format!("write response {}: {error}", index + 1))?;
        if flush_each {
            output
                .flush()
                .map_err(|error| format!("flush response {}: {error}", index + 1))?;
        }
    }
    output.flush().map_err(|error| {
        if flush_each {
            format!("flush stdout: {error}")
        } else {
            error.to_string()
        }
    })
}
fn request_id(line: &str) -> Value {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_requests_keep_their_envelope_and_do_not_stop_later_requests() {
        let mut output = Vec::new();
        process(
            &b"\n{bad\n{\"id\":\"bad\"}\n{\"id\":\"ok\"}\n"[..],
            &mut output,
            "test/v1",
            |line| {
                if line.contains("ok") {
                    Ok(json!({"id":"ok", "result":true}))
                } else {
                    Err("rejected".into())
                }
            },
            false,
        )
        .unwrap();
        let values = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![
                json!({"schema":"test/v1","id":null,"error":"rejected"}),
                json!({"schema":"test/v1","id":"bad","error":"rejected"}),
                json!({"id":"ok","result":true})
            ]
        );
    }
    #[test]
    fn streaming_policy_preserves_flush_count() {
        #[derive(Default)]
        struct Sink {
            flushes: usize,
        }
        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }
        for (flush_each, expected) in [(false, 1), (true, 3)] {
            let mut sink = Sink::default();
            process(
                &b"{}\n\n{}\n"[..],
                &mut sink,
                "test/v1",
                |_| Ok(Value::Null),
                flush_each,
            )
            .unwrap();
            assert_eq!(sink.flushes, expected);
        }
    }
}
