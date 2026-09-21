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
    recognized: &[Vec<super::RecognizedName>],
    native: Option<&super::NativeDeclarationEvidence>,
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
            let mut native_markup = false;
            if !spelling.is_empty() {
                for (index, ((_, leaves), candidates)) in terms.iter().zip(recognized).enumerate() {
                    for candidate in candidates
                        .iter()
                        .filter(|candidate| candidate.name == *spelling)
                    {
                        native_markup |= native.is_some_and(|native| {
                            native
                                .name_hints
                                .iter()
                                .any(|hint| hint.term == index && hint.parts == candidate.parts)
                        });
                        let parts = candidate
                            .parts
                            .iter()
                            .cloned()
                            .flat_map(|range| slices(leaves, index, range))
                            .collect();
                        occurrences.push(EntryForm { parts });
                    }
                }
            }
            EntryNameBinding {
                name,
                occurrences,
                evidence: if native_markup {
                    EntryNameEvidence::NativeMarkup
                } else {
                    EntryNameEvidence::Lexical
                },
            }
        })
        .collect()
}

pub(super) fn native_forms(
    item: &DefinitionItem,
    native: &super::NativeDeclarationEvidence,
) -> Vec<EntryForm> {
    let terms = item
        .terms
        .iter()
        .map(|term| {
            let mut text = String::new();
            let mut leaves = Vec::new();
            collect(term, &mut Vec::new(), &mut text, &mut leaves);
            leaves
        })
        .collect::<Vec<_>>();
    native
        .forms
        .iter()
        .filter_map(|form| {
            let leaves = terms.get(form.term)?;
            let parts = form
                .parts
                .iter()
                .cloned()
                .flat_map(|range| slices(leaves, form.term, range))
                .collect::<Vec<_>>();
            (!parts.is_empty()).then_some(EntryForm { parts })
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
