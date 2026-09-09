//! Recognize standalone roff alias syntax without resolving or opening a path.

/// A standalone alias request did not contain exactly one nonempty target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedirectSyntaxError;

impl std::fmt::Display for RedirectSyntaxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("manual .so redirect must contain exactly one target path")
    }
}

impl std::error::Error for RedirectSyntaxError {}

/// Return a standalone alias target, not a path authorization or resolved file.
///
/// # Errors
/// Returns [`RedirectSyntaxError`] when an alias has missing or extra operands.
pub fn redirect_target(source: &[u8]) -> Result<Option<Vec<u8>>, RedirectSyntaxError> {
    let mut payloads = Vec::new();
    let mut has_other_content = false;

    for raw_line in source.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if trim_ascii(line).is_empty() || is_roff_comment(line) {
            continue;
        }
        let Some(payload) = so_request_payload(line) else {
            has_other_content = true;
            continue;
        };
        payloads.push(payload);
    }

    match (payloads.as_slice(), has_other_content) {
        ([payload], false) => parse_so_target(payload).map(Some),
        // An embedded include is not an alias. Leave it in the source so the
        // safe libmandoc policy emits a diagnostic and preserves surrounding
        // content instead of rejecting the complete page before parsing.
        _ => Ok(None),
    }
}

fn so_request_payload(line: &[u8]) -> Option<&[u8]> {
    let payload = line
        .strip_prefix(b".so")
        .or_else(|| line.strip_prefix(b"'so"))?;
    (payload.is_empty() || payload[0].is_ascii_whitespace()).then_some(payload)
}

fn parse_so_target(payload: &[u8]) -> Result<Vec<u8>, RedirectSyntaxError> {
    let payload = trim_ascii(payload);
    let target_end = payload
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(payload.len());
    let target = &payload[..target_end];
    let trailing = trim_ascii(&payload[target_end..]);
    if target.is_empty()
        || target.contains(&0)
        || (!trailing.is_empty() && !trailing.starts_with(b"\\\"") && !trailing.starts_with(b"\\#"))
    {
        return Err(RedirectSyntaxError);
    }
    Ok(target.to_vec())
}

fn is_roff_comment(line: &[u8]) -> bool {
    [b".\\\"".as_slice(), b"'\\\"", b".\\#", b"'\\#"]
        .into_iter()
        .any(|prefix| line.starts_with(prefix))
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_standalone_requests_are_redirects() {
        for source in [
            b".so man1/target.1\n".as_slice(),
            b".\\\" comment\r\n\r\n'so man1/target.1 \\\" trailing comment\r\n",
            b".\\# comment\n.so man1/target.1 \\# trailing comment\n",
        ] {
            assert_eq!(
                redirect_target(source).unwrap().as_deref(),
                Some(b"man1/target.1".as_slice())
            );
        }
        for source in [
            b".TH INLINE 1\n.so target.1\n".as_slice(),
            b".so first\n.so second\n",
            b".some text\n",
            b"",
        ] {
            assert_eq!(redirect_target(source).unwrap(), None);
        }
    }

    #[test]
    fn syntax_does_not_authorize_paths_or_decode_original_target_bytes() {
        // Path policy belongs to the loader: recognizing this syntax grants no IO.
        for target in [b"../outside.1".as_slice(), b"/absolute.1", b"raw-\xff.1"] {
            let mut source = b".so ".to_vec();
            source.extend(target);
            assert_eq!(redirect_target(&source).unwrap().as_deref(), Some(target));
        }
        for source in [b".so\n".as_slice(), b".so two names\n", b".so bad\0name\n"] {
            assert_eq!(redirect_target(source), Err(RedirectSyntaxError));
        }
    }
}
