//! Read-only semantic references into an unchanged ordinary list item.
use super::{AttachedValuePolicy, EntrySignature};
use mant_ir::{
    Block, EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot, EntryKind, EntryNameBinding,
    EntryNameEvidence, Inline, ListItem, NameCase,
};

pub(super) fn entry_facts(
    item: &ListItem,
    signature: &EntrySignature,
    role: EntryKind,
    case: NameCase,
    _attached: AttachedValuePolicy,
    declared: bool,
) -> EntryFacts {
    let Block::Paragraph { children, .. } = &item.blocks[0] else {
        unreachable!("validated signature has a leading paragraph")
    };
    let mut forms = vec![EntryForm { parts: Vec::new() }];
    let mut breaks = signature.form_breaks.iter().copied().peekable();
    for index in 0..=signature.inline_index {
        if breaks.peek() == Some(&index) {
            breaks.next();
            forms.push(EntryForm { parts: Vec::new() });
            continue;
        }
        let bytes = if index == signature.inline_index {
            if signature.byte_index == 0 {
                break;
            }
            Some(0..signature.byte_index)
        } else {
            None
        };
        forms
            .last_mut()
            .expect("at least one form")
            .parts
            .push(EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![index],
                bytes,
            });
    }
    let name_indices = signature
        .names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut bindings = signature
        .names
        .iter()
        .enumerate()
        .map(|(name, _)| EntryNameBinding {
            name,
            occurrences: Vec::new(),
            evidence: if declared {
                EntryNameEvidence::Declared
            } else {
                EntryNameEvidence::Lexical
            },
        })
        .collect::<Vec<_>>();
    for (index, found) in &signature.name_occurrences {
        let name = name_indices[found.name.as_str()];
        let mut path = vec![*index];
        if matches!(&children[*index], Inline::Link { .. }) {
            path.push(0);
        }
        bindings[name].occurrences.push(EntryForm {
            parts: found
                .parts
                .iter()
                .map(|range| EntryContentSlice {
                    root: EntryInlineRoot::Block { index: 0 },
                    path: path.clone(),
                    bytes: Some(range.clone()),
                })
                .collect(),
        });
    }
    bindings.retain(|binding| !binding.occurrences.is_empty());
    EntryFacts {
        id: "".into(),
        kind: role,
        case,
        names: signature.names.clone(),
        forms,
        name_bindings: bindings,
        value_domain: None,
        alias_groups: Vec::new(),
        alias_of: None,
    }
}
