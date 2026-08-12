//! Conservative compatibility for newer GNU man hyperlink macros.
//!
//! The pinned mandoc 1.14.6 parser understands `UR`/`UE` blocks but predates
//! `MR` and `MT`/`ME`. Rewriting only their strict, portable forms preserves
//! line count and lets the ordinary typed lowering path handle them.

use std::borrow::Cow;

pub(super) fn normalize_groff_navigation_macros(source: &[u8]) -> Cow<'_, [u8]> {
    let mut output = Vec::with_capacity(source.len());
    let mut changed = false;
    for line in source.split_inclusive(|byte| *byte == b'\n') {
        let (content, newline) = line
            .strip_suffix(b"\n")
            .map_or((line, &b""[..]), |content| (content, &b"\n"[..]));
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        let carriage_return = line.ends_with(b"\r\n");
        if let Some(replacement) = rewrite_line(content) {
            output.extend_from_slice(replacement.as_bytes());
            if carriage_return {
                output.push(b'\r');
            }
            output.extend_from_slice(newline);
            changed = true;
        } else {
            output.extend_from_slice(line);
        }
    }
    if changed {
        Cow::Owned(output)
    } else {
        Cow::Borrowed(source)
    }
}

fn rewrite_line(line: &[u8]) -> Option<String> {
    let line = std::str::from_utf8(line).ok()?;
    let (control, request) = line.split_at_checked(1)?;
    if !matches!(control, "." | "'") {
        return None;
    }
    let mut words = request.split_ascii_whitespace();
    match words.next()? {
        "MR" => {
            let name = words.next()?;
            let section = words.next()?;
            if !valid_manual_name(name) || !valid_manual_section(section) {
                return None;
            }
            let trailing = words.collect::<Vec<_>>().join(" ");
            Some(if trailing.is_empty() {
                format!("{control}BR {name} ({section})")
            } else {
                format!("{control}BR {name} ({section}) {trailing}")
            })
        }
        "MT" => {
            let address = words.next()?;
            if words.next().is_some() || !valid_link_target(address) {
                return None;
            }
            Some(format!("{control}UR mailto:{address}"))
        }
        "ME" => Some(format!(
            "{control}UE{}",
            request.strip_prefix("ME").unwrap_or_default()
        )),
        _ => None,
    }
}

fn valid_manual_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | ':' | '-')
        })
}

fn valid_manual_section(section: &str) -> bool {
    !section.is_empty()
        && section.len() <= 16
        && section
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn valid_link_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= 4096
        && !target.contains(['\'', '"', '\\'])
        && !target.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::normalize_groff_navigation_macros;

    #[test]
    fn rewrites_only_strict_modern_man_navigation_macros() {
        let source = b".MR git-add 1 ,\n.MT docs@example.test\n.ME .\n.MR bad/path 1\n";
        let normalized = normalize_groff_navigation_macros(source);

        assert_eq!(
            normalized.as_ref(),
            b".BR git-add (1) ,\n.UR mailto:docs@example.test\n.UE .\n.MR bad/path 1\n"
        );
    }
}
