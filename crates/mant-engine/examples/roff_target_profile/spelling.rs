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
    while !characters.as_str().is_empty() {
        if let Some(character) = next_visible_character(&mut characters)? {
            visible.push(character);
        }
    }
    Some(visible)
}

/// Preserve the raw source slice for the first *visible* whitespace-delimited
/// token. In particular, do not split `tilde\ escape` into an invalid `tilde\`.
/// Native tag.c stops its implicit tag at the escape; the codec also uses only
/// the first visible word when no explicit native tag was retained. Unlike an
/// actual tag string such as `show\ all\ procs`, the fallback is not a phrase.
pub(super) fn first_source_token(raw: &str) -> Option<&str> {
    let mut characters = raw.chars();
    let mut token_start = 0;
    let mut has_visible_character = false;
    while !characters.as_str().is_empty() {
        let offset = raw.len() - characters.as_str().len();
        let Some(character) = next_visible_character(&mut characters)? else {
            continue;
        };
        if character.is_whitespace() {
            if has_visible_character {
                return Some(&raw[token_start..offset]);
            }
            token_start = raw.len() - characters.as_str().len();
        } else {
            has_visible_character = true;
        }
    }
    has_visible_character.then_some(&raw[token_start..])
}

fn next_visible_character(characters: &mut std::str::Chars<'_>) -> Option<Option<char>> {
    let character = characters.next()?;
    if character != '\\' {
        return Some(Some(character));
    }
    match characters.next()? {
        // ManT does not turn zero-width or sub-column spacing hints into name
        // characters. These are not the word separators handled below.
        '&' | '^' | '|' => Some(None),
        'e' | '\\' => Some(Some('\\')),
        '-' => Some(Some('-')),
        ' ' | '~' | '0' => Some(Some(' ')),
        'f' => {
            match characters.next()? {
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
            }
            Some(None)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{automatic_target_spelling, first_source_token};

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
            (r"length\^", "length"),
            (r"<\|(\|[", "<(["),
            (r"show\ all\ procs", "show all procs"),
        ] {
            assert_eq!(
                automatic_target_spelling(raw).as_deref(),
                Some(visible),
                "{raw}"
            );
        }
    }

    #[test]
    fn fallback_tokens_stop_at_visible_spaces_without_cutting_escape_sequences() {
        for (raw, token) in [
            (r"(chunk\ size\ is\ the\ number\ of\ bytes read)", "(chunk"),
            (r"tilde\ escape", "tilde"),
            (r"show\ all\ procs", "show"),
            (r"name\~argument", "name"),
            (r"name\0argument", "name"),
            (r"\&\ first second", "first"),
            (r"F\fR|\fPf argument", r"F\fR|\fPf"),
            (r"a\\ b", r"a\\"),
        ] {
            assert_eq!(first_source_token(raw), Some(token), "{raw}");
        }
        assert_eq!(first_source_token(r"\&\ "), None);
        assert_eq!(first_source_token(r"name\"), None);
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
