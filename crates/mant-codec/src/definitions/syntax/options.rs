//! options recognition; complete forms retain their role-specific grammar.
use super::forms;
use crate::definitions::RecognizedName;
#[cfg(test)]
use mant_ir::DefinitionItem;
use mant_ir::inline_plain_text as plain_text;
pub(crate) use mant_ir::option_prefix;
use mant_ir::{
    ContentContext, DeclarationContext, DeclarationView, Inline, is_option_name_body,
    native_option_token, recognize_option_declarations,
};
use std::collections::HashSet;

#[cfg(test)]
pub(in crate::definitions) fn option_names(
    content: ContentContext<'_>,
    item: &DefinitionItem,
) -> Vec<String> {
    option_names_from_terms(content, &item.terms)
}

#[cfg(test)]
pub(crate) fn option_names_from_terms(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<String> {
    let mut names = Vec::new();
    for found in option_occurrences_from_terms(content, terms)
        .into_iter()
        .flatten()
    {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

fn checked_option_occurrences_from_term(
    content: ContentContext<'_>,
    term: &[Inline],
    ranges: &[std::ops::Range<usize>],
) -> Option<Vec<RecognizedName>> {
    let form = plain_text(content, term);
    let style = forms::option_style_evidence(content, term, &form, ranges);
    let result = recognize_option_declarations(
        DeclarationView {
            visible: &form,
            native_operands: ranges,
            plain_italic_ranges: &style.plain_italic_ranges,
            bold_underline_starts: &style.bold_underline_starts,
            numeric_name_starts: &style.numeric_name_starts,
        },
        if ranges.is_empty() {
            DeclarationContext::SingleTextOperand
        } else {
            DeclarationContext::NativeComponents
        },
    )?;
    (!result.over_limit()).then_some(
        result
            .names()
            .iter()
            .map(|(name, range)| RecognizedName::contiguous(name, range.start))
            .collect(),
    )
}

#[cfg(test)]
fn recognize_option_occurrences_from_terms(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
    operand_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
) -> Vec<Vec<RecognizedName>> {
    terms
        .iter()
        .enumerate()
        .map(|(index, term)| {
            let ranges = operand_ranges
                .and_then(|terms| terms.get(index))
                .map_or(&[][..], Vec::as_slice);
            checked_option_occurrences_from_term(content, term, ranges).unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn option_occurrences_from_terms(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<Vec<RecognizedName>> {
    recognize_option_occurrences_from_terms(content, terms, None)
}

/// Recognize the option declarations in one complete literal leaf.
///
/// Markdown entry discovery uses this instead of manufacturing a detached
/// `Inline::Code` node with its own copy of the visible text.
pub(crate) fn option_occurrences_from_literal(value: &str) -> Vec<RecognizedName> {
    forms::literal_option_tokens(value)
        .into_iter()
        .filter_map(|(token, start)| {
            Some(RecognizedName::contiguous(option_prefix(&token)?, start))
        })
        .collect()
}

pub(crate) fn option_names_from_literal(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    for found in option_occurrences_from_literal(value) {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

/// A validated native Fl head proves punctuation is invocation spelling.
/// Read it before generic separator grouping can treat the comma in `-,` as
/// alias punctuation; styled arguments still stop the literal prefix.
pub(super) fn native_option_occurrences(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
    option_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
    operand_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
) -> Vec<Vec<RecognizedName>> {
    terms
        .iter()
        .enumerate()
        .map(|(index, term)| {
            let operands = operand_ranges
                .and_then(|terms| terms.get(index))
                .map_or(&[][..], Vec::as_slice);
            let Some(mut names) = checked_option_occurrences_from_term(content, term, operands)
            else {
                // Invalid or over-limit complete HEAD evidence cannot be
                // partially restored by a later explicit Fl component.
                return Vec::new();
            };
            // A native `.Fl` proves even a digit spelling to be a declaration.
            // The witness identifies this *macro instance's* final visible span;
            // identical bold text from `.Sy -6` is not equivalent evidence.
            let Some(ranges) = option_ranges.and_then(|terms| terms.get(index)) else {
                return names;
            };
            let text = plain_text(content, term);
            let mut seen = names
                .iter()
                .filter_map(|found| {
                    let [range] = found.parts.as_slice() else {
                        return None;
                    };
                    Some((found.name.clone(), range.start))
                })
                .collect::<HashSet<_>>();
            for range in ranges {
                let Some(fragment) = text.get(range.clone()) else {
                    continue;
                };
                let leading = fragment.len() - fragment.trim_start().len();
                let Some(mut token) = fragment.split_whitespace().next() else {
                    continue;
                };
                let offset = range.start + leading;
                // mdoc_macro.c::in_line() creates an empty Fl before a
                // delimiter, and mdoc_term.c::termp_fl_pre() prints only its
                // generated dash. The sibling delimiter is outside this Fl
                // range but directly joins it in the final HEAD. Extend only
                // that empty-instance witness, never an ordinary argument or
                // a nonempty Fl spelling (mdoc.c::mdoc_isdelim()).
                if token == "-"
                    && range.end == offset + 1
                    && let Some(delimiter) = text.as_bytes().get(range.end)
                    // A closing bracket may instead be generated by an
                    // enclosing Oo/Po macro, as in `Oo Fl Oc`: it is not
                    // the empty Fl's sibling punctuation. Without a native
                    // sibling witness, never borrow that glyph as a name.
                    && b"|.,;:?!".contains(delimiter)
                    && let Some(joined) = text.get(offset..range.end + 1)
                {
                    token = joined;
                }
                if native_option_token(token) && seen.insert((token.to_owned(), offset)) {
                    if names.len() == 64 {
                        // Both lexical and explicit Fl occurrences share the
                        // per-head bound. Never publish only its first 64 names.
                        return Vec::new();
                    }
                    names.push(RecognizedName::contiguous(token, offset));
                }
            }
            // Native `.Fl` instances are recovered after the generic candidates;
            // preserve the actual visible head order, not discovery order.
            names.sort_by_key(|found| found.parts.first().map_or(usize::MAX, |part| part.start));
            names
        })
        .collect()
}

pub(in crate::definitions) fn parameter_occurrences(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
    operand_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
) -> Vec<Vec<RecognizedName>> {
    terms
        .iter()
        .enumerate()
        .map(|(index, term)| {
            let operands = operand_ranges
                .and_then(|terms| terms.get(index))
                .map_or(&[][..], Vec::as_slice);
            let Some(mut names) = checked_option_occurrences_from_term(content, term, operands)
            else {
                return Vec::new();
            };
            let text = plain_text(content, term);
            let Some(token) = text.split_whitespace().next() else {
                return names;
            };
            let start = token.as_ptr() as usize - text.as_ptr() as usize;
            if let Some(body) = token.strip_prefix("[-+]")
                && is_option_name_body(body)
            {
                if names.len() > 62 {
                    return Vec::new();
                }
                for (sign, offset) in [('-', 1), ('+', 2)] {
                    names.push(RecognizedName {
                        name: format!("{sign}{body}"),
                        parts: vec![
                            start + offset..start + offset + 1,
                            start + 4..start + token.len(),
                        ],
                    });
                }
            } else if token.strip_prefix('+').is_some_and(is_option_name_body) {
                if names.len() == 64 {
                    return Vec::new();
                }
                names.push(RecognizedName::contiguous(token, start));
            }
            names
        })
        .collect()
}

/// Recognize legacy slash-separated dash options, not general alias syntax.
/// Every non-final segment must be a complete option name: a slash inside an
/// argument path or assignment RHS must never start a new invocation. Only
/// the final invocation may carry an argument (validated by the caller).
pub(crate) fn slash_option_forms(value: &str) -> Option<Vec<&str>> {
    if !value.contains('/') {
        return None;
    }
    let parts: Vec<_> = value.split('/').collect();
    let (last, preceding) = parts.split_last()?;
    if !preceding
        .iter()
        .all(|part| option_prefix(part) == Some(*part))
    {
        return None;
    }
    let token = last.split_whitespace().next()?;
    let name = option_prefix(token)?;
    (token == name || token[name.len()..].starts_with('=')).then_some(parts)
}

#[cfg(test)]
mod literal_tests {
    use super::{
        option_names_from_literal, option_occurrences_from_literal, option_occurrences_from_terms,
    };
    use crate::test_content as fixture;
    use mant_ir::Inline;

    #[test]
    fn literal_entry_api_preserves_alias_argument_and_pair_rules() {
        assert_eq!(option_names_from_literal("-h, --help"), ["-h", "--help"]);
        assert_eq!(option_names_from_literal("--set=KEY,VALUE"), ["--set"]);
        assert_eq!(
            option_names_from_literal("-q or --quiet"),
            ["-q", "--quiet"]
        );
        assert!(option_occurrences_from_literal("ordinary prose").is_empty());
    }

    #[test]
    fn repeated_option_spelling_keeps_each_visible_binding() {
        // The exact TP/BI head ran pinned CVS -Tutf8 first. man_term.c::
        // pre_alternate retains both --output operands in authored order.
        let term = vec![
            Inline::Strong {
                children: vec![fixture::text("-o --output --output ")],
            },
            Inline::Emphasis {
                children: vec![fixture::text("FILE")],
            },
        ];
        let occurrences = option_occurrences_from_terms(fixture::content(), &[term]);
        assert_eq!(
            occurrences[0]
                .iter()
                .map(|found| (found.name.as_str(), found.parts[0].clone()))
                .collect::<Vec<_>>(),
            [("-o", 0..2), ("--output", 3..11), ("--output", 12..20),]
        );
    }
}
