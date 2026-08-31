//! Iterative top-level JSON field inspection for bounded process framing.

/// Read one top-level field without recursively materializing the surrounding
/// JSON value. Selected scalar fields are still validated by `serde_json`.
pub(crate) fn top_level_value(input: &[u8], field: &str) -> Option<serde_json::Value> {
    let mut cursor = skip_whitespace(input, 0);
    if input.get(cursor) != Some(&b'{') {
        return None;
    }
    cursor += 1;
    loop {
        cursor = skip_whitespace(input, cursor);
        if input.get(cursor) == Some(&b'}') {
            return None;
        }
        let key_end = string_end(input, cursor)?;
        let key = serde_json::from_slice::<String>(&input[cursor..key_end]).ok()?;
        cursor = skip_whitespace(input, key_end);
        if input.get(cursor) != Some(&b':') {
            return None;
        }
        cursor = skip_whitespace(input, cursor + 1);
        let value_end = value_end(input, cursor)?;
        if key == field {
            return serde_json::from_slice(&input[cursor..value_end]).ok();
        }
        cursor = skip_whitespace(input, value_end);
        match input.get(cursor) {
            Some(b',') => cursor += 1,
            _ => return None,
        }
    }
}

pub(crate) fn top_level_string(input: &[u8], field: &str) -> Option<String> {
    top_level_value(input, field)?.as_str().map(str::to_owned)
}

fn skip_whitespace(input: &[u8], mut cursor: usize) -> usize {
    while input
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        cursor += 1;
    }
    cursor
}

fn string_end(input: &[u8], start: usize) -> Option<usize> {
    if input.get(start) != Some(&b'"') {
        return None;
    }
    let mut escaped = false;
    for (relative, byte) in input.get(start + 1..)?.iter().enumerate() {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Some(start + relative + 2);
        }
    }
    None
}

fn value_end(input: &[u8], start: usize) -> Option<usize> {
    match *input.get(start)? {
        b'"' => string_end(input, start),
        b'{' | b'[' => compound_end(input, start),
        _ => {
            let mut cursor = start;
            while input.get(cursor).is_some_and(|byte| {
                !matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n')
            }) {
                cursor += 1;
            }
            (cursor > start).then_some(cursor)
        }
    }
}

fn compound_end(input: &[u8], start: usize) -> Option<usize> {
    let mut stack = vec![*input.get(start)?];
    let mut cursor = start + 1;
    while let Some(byte) = input.get(cursor).copied() {
        match byte {
            b'"' => cursor = string_end(input, cursor)?,
            b'{' | b'[' => {
                stack.push(byte);
                cursor += 1;
            }
            b'}' if stack.pop() == Some(b'{') => {
                cursor += 1;
                if stack.is_empty() {
                    return Some(cursor);
                }
            }
            b']' if stack.pop() == Some(b'[') => {
                cursor += 1;
                if stack.is_empty() {
                    return Some(cursor);
                }
            }
            b'}' | b']' => return None,
            _ => cursor += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{top_level_string, top_level_value};

    #[test]
    fn extracts_scalars_without_descending_recursively() {
        let deep = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":17,\"params\":{} }}",
            "[".repeat(512) + &"]".repeat(512)
        );
        assert_eq!(top_level_value(deep.as_bytes(), "id"), Some(17.into()));
        assert_eq!(
            top_level_string(br#"{"schema":"mant.query/v0.10","body":{}}"#, "schema").as_deref(),
            Some("mant.query/v0.10")
        );
    }

    #[test]
    fn rejects_malformed_top_level_shapes_and_non_scalar_selected_values() {
        assert_eq!(top_level_value(br"[1,2,3]", "id"), None);
        assert_eq!(top_level_value(br#"{"id": [1,}"#, "id"), None);
    }
}
