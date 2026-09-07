//! Finite literal boundaries, not an executable argv grammar or fuzzy matcher.
use std::ops::Range;

pub(super) fn block_text(block: &mant_ir::Block) -> Option<String> {
    let raw = match block {
        mant_ir::Block::Paragraph { children, .. }
        | mant_ir::Block::Preformatted { children, .. } => crate::inline::plain_text(children),
        mant_ir::Block::Equation { value, .. }
        | mant_ir::Block::Unsupported { text: value, .. } => value.clone(),
        _ => return None,
    };
    Some(
        raw.chars()
            .map(|c| {
                if c.is_control() && !matches!(c, '\n' | '\t') {
                    '\u{fffd}'
                } else {
                    c
                }
            })
            .collect(),
    )
}

pub(super) fn first_match(text: &str, query: &str) -> Option<Range<usize>> {
    text.match_indices(query).find_map(|(start, found)| {
        let end = start + found.len();
        (text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !name_continuation(c))
            && right_boundary(&text[end..]))
        .then_some(start..end)
    })
}

fn right_boundary(tail: &str) -> bool {
    let mut chars = tail.chars();
    match chars.next() {
        // Dot/colon may terminate a sentence, but remain internal in names
        // such as -ca.cert and /F:Y. '=' deliberately admits parameter forms.
        Some('.' | ':') => chars.next().is_none_or(|c| {
            c.is_whitespace()
                || unicode_prose_punctuation(c)
                || matches!(c, ')' | ']' | '}' | ',' | ';' | '"' | '\'')
        }),
        Some(c) => !name_continuation(c),
        None => true,
    }
}

fn name_continuation(c: char) -> bool {
    // Use a closed set of prose separators, not an alphabetic whitelist:
    // #, %, ?, *, ! and other executable punctuation must not shorten names.
    !c.is_whitespace()
        && !unicode_prose_punctuation(c)
        && !matches!(
            c,
            '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | '`' | ',' | ';' | '='
        )
}

/// Typographic quotation/enclosure and sentence punctuation delimit prose,
/// including punctuation adjacent to CJK text without whitespace. This is a
/// finite policy, not "all non-ASCII/non-alphanumeric characters": Unicode
/// letters, combining marks, symbols and name-internal dashes remain intact.
fn unicode_prose_punctuation(c: char) -> bool {
    matches!(
        c,
        '‘' | '’'
            | '‚'
            | '‛'
            | '“'
            | '”'
            | '„'
            | '‟'
            | '«'
            | '»'
            | '‹'
            | '›'
            | '「'
            | '」'
            | '『'
            | '』'
            | '〈'
            | '〉'
            | '《'
            | '》'
            | '【'
            | '】'
            | '〔'
            | '〕'
            | '〖'
            | '〗'
            | '〘'
            | '〙'
            | '〚'
            | '〛'
            | '〝'
            | '〞'
            | '〟'
            | '（'
            | '）'
            | '［'
            | '］'
            | '｛'
            | '｝'
            | '、'
            | '。'
            | '，'
            | '．'
            | '：'
            | '；'
            | '！'
            | '？'
            | '｡'
            | '､'
            | '،'
            | '؛'
            | '؟'
    )
}

#[cfg(test)]
mod tests {
    use super::first_match;

    #[test]
    fn punctuation_and_case_do_not_create_prefix_evidence() {
        for (text, query) in [
            ("-###", "-#"),
            ("--%", "--"),
            ("--all -ab dir/-a", "-a"),
            ("-ca.cert", "-ca"),
            ("-i", "-I"),
            ("日本語", "日本"),
        ] {
            assert_eq!(first_match(text, query), None, "{text} / {query}");
        }
        for (text, query) in [
            ("-###", "-###"),
            ("--%", "--%"),
            ("Use (-a), then -I.", "-a"),
            ("Use -I.", "-I"),
            ("Use --help:", "--help"),
            ("Use --help=CLASS", "--help"),
            ("日本語", "日本語"),
        ] {
            let range = first_match(text, query).expect(text);
            assert_eq!(&text[range], query);
        }
    }
}
