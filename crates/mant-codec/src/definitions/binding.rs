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
) -> Vec<EntryNameBinding> {
    name_bindings(
        item.terms.iter().map(mant_ir::DefinitionTerm::as_slice),
        names,
        recognized,
    )
}

/// Map an already recognized paragraph head without copying it into a
/// temporary definition. The owner adapter translates Term(0) to Block(0).
pub(super) fn native_name_bindings_for_head(
    inlines: &[Inline],
    names: &[String],
    recognized: &[Vec<super::RecognizedName>],
) -> Vec<EntryNameBinding> {
    name_bindings(std::iter::once(inlines), names, recognized)
}

fn name_bindings<'a>(
    terms: impl Iterator<Item = &'a [Inline]>,
    names: &[String],
    recognized: &[Vec<super::RecognizedName>],
) -> Vec<EntryNameBinding> {
    let terms = terms
        .map(|term| {
            let mut offset = 0;
            let mut leaves = Vec::new();
            collect(term, &mut Vec::new(), &mut offset, &mut leaves);
            leaves
        })
        .collect::<Vec<_>>();
    names
        .iter()
        .enumerate()
        .map(|(name, spelling)| {
            let mut occurrences = Vec::new();
            if !spelling.is_empty() {
                for (index, (leaves, candidates)) in terms.iter().zip(recognized).enumerate() {
                    for candidate in candidates
                        .iter()
                        .filter(|candidate| candidate.name == *spelling)
                    {
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

fn collect(nodes: &[Inline], path: &mut Vec<usize>, offset: &mut usize, leaves: &mut Vec<Leaf>) {
    for (index, node) in nodes.iter().enumerate() {
        path.push(index);
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                let start = *offset;
                *offset += value.len();
                leaves.push(Leaf {
                    path: path.clone(),
                    range: start..*offset,
                });
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect(children, path, offset, leaves),
            Inline::Anchor { .. } => {}
            Inline::LineBreak { .. } => *offset += 1,
        }
        path.pop();
    }
}

#[cfg(test)]
#[path = "binding/ranges_tests.rs"]
mod ranges_tests;
