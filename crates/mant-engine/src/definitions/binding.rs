//! Bind already recognized native names to exact visible term ranges.
//! This does not discover names or treat description mentions as names.
use mant_ir::{
    DefinitionItem, EntryContentSlice, EntryForm, EntryInlineRoot, EntryNameBinding,
    EntryNameEvidence, Inline,
};
use std::ops::Range;

struct Leaf {
    path: Vec<usize>,
    range: Range<usize>,
}

pub(super) fn native_name_bindings(
    item: &DefinitionItem,
    names: &[String],
) -> Vec<EntryNameBinding> {
    let terms = item
        .terms
        .iter()
        .map(|term| {
            let mut text = String::new();
            let mut leaves = Vec::new();
            collect(term, &mut Vec::new(), &mut text, &mut leaves);
            (text, leaves)
        })
        .collect::<Vec<_>>();
    names
        .iter()
        .enumerate()
        .map(|(name, spelling)| {
            let mut occurrences = Vec::new();
            if !spelling.is_empty() {
                for (index, (text, leaves)) in terms.iter().enumerate() {
                    // The native grammar recognizes this exact finite prefix
                    // alternation. Bind the selected sign and shared suffix,
                    // rather than inventing a contiguous spelling in the term.
                    let token = text.split_whitespace().next().unwrap_or_default();
                    if let Some(body) = token.strip_prefix("[-+]") {
                        for (sign, offset) in [('-', 1), ('+', 2)] {
                            if spelling.strip_prefix(sign) == Some(body) {
                                let parts = [offset..offset + 1, 4..token.len()]
                                    .into_iter()
                                    .flat_map(|range| slices(leaves, index, range))
                                    .collect();
                                occurrences.push(EntryForm { parts });
                            }
                        }
                    }
                    for (start, _) in text.match_indices(spelling) {
                        let end = start + spelling.len();
                        let begins = text[..start].chars().next_back().is_none_or(separator)
                            || leaves.iter().any(|leaf| leaf.range.start == start);
                        let ends = leaves.iter().any(|leaf| leaf.range.end == end)
                            || spelling.ends_with(['=', ':'])
                            || text[end..]
                                .chars()
                                .next()
                                .is_none_or(|c| separator(c) || matches!(c, '=' | ':' | '['));
                        if !begins || !ends {
                            continue;
                        }
                        let parts = slices(leaves, index, start..end).collect();
                        occurrences.push(EntryForm { parts });
                    }
                }
            }
            EntryNameBinding {
                name,
                occurrences,
                evidence: EntryNameEvidence::Lexical,
            }
        })
        .collect()
}

fn slices(
    leaves: &[Leaf],
    index: usize,
    range: Range<usize>,
) -> impl Iterator<Item = EntryContentSlice> + '_ {
    leaves.iter().filter_map(move |leaf| {
        let begin = range.start.max(leaf.range.start);
        let finish = range.end.min(leaf.range.end);
        (begin < finish).then(|| EntryContentSlice {
            root: EntryInlineRoot::Term { index },
            path: leaf.path.clone(),
            bytes: Some(begin - leaf.range.start..finish - leaf.range.start),
        })
    })
}

fn separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | '/' | '|' | '(' | ')' | '[' | ']')
}

fn collect(nodes: &[Inline], path: &mut Vec<usize>, text: &mut String, leaves: &mut Vec<Leaf>) {
    for (index, node) in nodes.iter().enumerate() {
        path.push(index);
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                let start = text.len();
                text.push_str(value);
                leaves.push(Leaf {
                    path: path.clone(),
                    range: start..text.len(),
                });
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect(children, path, text, leaves),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => text.push('\n'),
        }
        path.pop();
    }
}
