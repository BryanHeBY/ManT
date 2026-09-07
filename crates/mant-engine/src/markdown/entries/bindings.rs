//! Read-only semantic references into an unchanged ordinary list item.
use super::{AttachedValuePolicy, EntrySignature, entry_names, entry_term_text};
use mant_ir::{
    Block, EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot, EntryKind, EntryNameBinding,
    EntryNameEvidence, Inline, ListItem, NameCase,
};

pub(super) fn entry_facts(
    item: &ListItem,
    signature: &EntrySignature,
    role: EntryKind,
    case: NameCase,
    attached: AttachedValuePolicy,
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
    for (index, inline) in children[..signature.inline_index].iter().enumerate() {
        let Some(value) = entry_term_text(inline) else {
            continue;
        };
        // Parse each visible span once. A same-spelled substring of another
        // name or assignment value is not an additional name occurrence.
        let Ok(names) = entry_names(value, role, declared, attached) else {
            continue;
        };
        for spelling in names {
            let Some(&name) = name_indices.get(spelling.as_str()) else {
                continue;
            };
            for (start, _) in value.match_indices(spelling.as_str()) {
                let end = start + spelling.len();
                if !value[..start]
                    .chars()
                    .next_back()
                    .is_none_or(name_separator)
                    || !(spelling.ends_with(['=', ':'])
                        || value[end..]
                            .chars()
                            .next()
                            .is_none_or(|ch| name_separator(ch) || matches!(ch, '=' | ':')))
                {
                    continue;
                }
                let mut path = vec![index];
                if matches!(inline, Inline::Link { .. }) {
                    path.push(0);
                }
                bindings[name].occurrences.push(EntryForm {
                    parts: vec![EntryContentSlice {
                        root: EntryInlineRoot::Block { index: 0 },
                        path,
                        bytes: Some(start..end),
                    }],
                });
            }
        }
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

fn name_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | '/' | '|')
}
