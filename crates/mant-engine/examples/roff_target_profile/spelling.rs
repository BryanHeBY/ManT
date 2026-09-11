//! Independent, deliberately bounded spelling oracle for automatic targets.
//!
//! Native `tag_move_id()` can copy the original escaped child text to a target,
//! even when `tag_put()` used its shortened spelling for the native index. The
//! codec's generated identities instead describe the visible source spelling.
//! Keep raw target strings in the audit evidence and decode only these reviewed
//! source forms before computing the expected canonical identity. Unsupported
//! escapes must become unclassified obligations, not guessed matches. Do not
//! call the production target planner or its roff decoder from this oracle.

pub(super) fn automatic_target_spelling(raw: &str) -> Option<String> {
    let mut visible = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            visible.push(character);
            continue;
        }
        match characters.next()? {
            '&' => {}
            'e' | '\\' => visible.push('\\'),
            '-' => visible.push('-'),
            'f' => match characters.next()? {
                '(' => {
                    characters.next()?;
                    characters.next()?;
                }
                '[' => loop {
                    if characters.next()? == ']' {
                        break;
                    }
                },
                _ => {}
            },
            _ => return None,
        }
    }
    Some(visible)
}

#[cfg(test)]
mod tests {
    use super::automatic_target_spelling;

    #[test]
    fn native_target_escapes_have_visible_spelling_without_recursive_decoding() {
        for (raw, visible) in [
            (r"?\&", "?"),
            (r"!\&", "!"),
            (r"b\eB", r"b\B"),
            (r"f\eB\eL", r"f\B\L"),
            (r#"%({}Q"E\e)"#, r#"%({}Q"E\)"#),
            (r"F\fR|\fPf", "F|f"),
            (r"A\f(BIB\f[Roman]C", "ABC"),
            (r"a\-b", "a-b"),
            (r"a\\&b", r"a\&b"),
        ] {
            assert_eq!(
                automatic_target_spelling(raw).as_deref(),
                Some(visible),
                "{raw}"
            );
        }
    }

    #[test]
    fn unsupported_or_incomplete_escapes_are_not_silently_accepted() {
        for raw in [
            r"name\",
            r"name\f",
            r"name\f(B",
            r"name\f[BI",
            r"\[alpha]",
            r"\h'2m'name",
        ] {
            assert_eq!(automatic_target_spelling(raw), None, "{raw}");
        }
    }
}
