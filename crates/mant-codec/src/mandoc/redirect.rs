//! Recognize standalone roff alias syntax without resolving or opening a path.

/// A standalone alias request did not contain exactly one nonempty target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedirectSyntaxError;

impl std::fmt::Display for RedirectSyntaxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("manual .so/.soquiet redirect must contain exactly one target path")
    }
}

impl std::error::Error for RedirectSyntaxError {}

/// Return a standalone `.so` or `.soquiet` alias target, not a path authorization
/// or resolved file.
///
/// # Errors
/// Returns [`RedirectSyntaxError`] when an alias has missing or extra operands.
pub fn redirect_target(source: &[u8]) -> Result<Option<Vec<u8>>, RedirectSyntaxError> {
    // read.c skips the UTF-8 BOM only at the beginning of the input.
    let source = source.strip_prefix(b"\xef\xbb\xbf").unwrap_or(source);
    let mut payload = None;

    for raw_line in source.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let Some(line) = static_line(line) else {
            return Ok(None);
        };
        if line.iter().copied().all(horizontal_space) {
            continue;
        }
        // roff.c::roff_getcontrol accepts . / ' / \. and skips horizontal
        // space *after* the control character. Source that changes the
        // control character or executes another request stays with the parser.
        let Some(request) = line
            .strip_prefix(b".")
            .or_else(|| line.strip_prefix(b"'"))
            .or_else(|| line.strip_prefix(b"\\."))
        else {
            return Ok(None);
        };
        let request = trim_horizontal(request);
        if request.is_empty() {
            continue;
        }
        let name_end = request
            .iter()
            .copied()
            .position(horizontal_space)
            .unwrap_or(request.len());
        if !matches!(&request[..name_end], b"so" | b"soquiet")
            || payload.replace(&request[name_end..]).is_some()
        {
            return Ok(None);
        }
    }

    // An embedded include stays in the source, so libmandoc can apply its
    // include policy while preserving the surrounding content.
    // Interpolated paths belong to roff execution.
    match payload {
        Some(payload) if !payload.contains(&b'\\') => parse_so_target(payload).map(Some),
        _ => Ok(None),
    }
}

fn static_line(line: &[u8]) -> Option<&[u8]> {
    // roff_parse_comment runs before the control lookup. A hash comment
    // or a trailing escape joins physical lines, so neither proves an
    // independent static redirect. The parser owns those cases.
    let mut rest = line;
    while let Some(index) = rest.iter().position(|byte| *byte == b'\\') {
        match rest.get(index + 1) {
            Some(b'"') => return Some(&line[..line.len() - rest.len() + index]),
            Some(b'#') | None => return None,
            _ => rest = &rest[index + 2..],
        }
    }
    Some(line)
}

fn parse_so_target(payload: &[u8]) -> Result<Vec<u8>, RedirectSyntaxError> {
    let target = trim_horizontal(payload);
    if target.is_empty()
        || target
            .iter()
            .any(|byte| byte.is_ascii_control() || *byte == b' ')
    {
        return Err(RedirectSyntaxError);
    }
    Ok(target.to_vec())
}

fn horizontal_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn trim_horizontal(mut value: &[u8]) -> &[u8] {
    while value.first().copied().is_some_and(horizontal_space) {
        value = &value[1..];
    }
    while value.last().copied().is_some_and(horizontal_space) {
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
            b"\xef\xbb\xbf. \t\n\\\" comment\n. so man1/target.1\\\" trailing comment\n",
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
            b".\\# comment\n.so man1/target.1\n",
            b".so man1/target.1 \\# joins the subsequent line\n",
            b".de alias\n.so man1/target.1\n..\n.alias\n",
            b".so \\*[path]\n",
            b" .so man1/target.1\n",
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

    #[test]
    fn quiet_alias_requests_use_the_same_standalone_and_operand_rules() {
        for source in [
            b".soquiet man1/target.1\n".as_slice(),
            b"'soquiet\tman1/target.1\r\n",
            b".\\\" comment\n.soquiet man1/target.1 \\\" trailing comment\n",
        ] {
            assert_eq!(
                redirect_target(source).unwrap().as_deref(),
                Some(b"man1/target.1".as_slice()),
            );
        }
        for source in [
            b".TH INLINE 1\n.soquiet target.1\n".as_slice(),
            b".soquiet first\n.so second\n",
            b".so first\n.soquiet second\n",
            b".soquietly target.1\n",
            b".soquietx target.1\n",
            b".msoquiet target.1\n",
            b".soquiet/target.1\n",
        ] {
            assert_eq!(redirect_target(source).unwrap(), None);
        }
        for source in [
            b".soquiet\n".as_slice(),
            b"'soquiet\t\r\n",
            b".soquiet two names\n",
            b".soquiet bad\0name\n",
        ] {
            assert_eq!(redirect_target(source), Err(RedirectSyntaxError));
        }
    }
}
