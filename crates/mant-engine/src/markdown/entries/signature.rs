//! Read-only head recognition; content remains with its original list item.
use super::{
    AttachedValuePolicy,
    diagnostics::{EntryRejection, EntryRejectionReason},
    names::entry_names,
};
use mant_ir::geometry::block_source;
use mant_ir::{Block, EntryKind, Inline, LinkTarget, ListItem};
#[derive(Clone)]
pub(super) struct EntrySignature {
    pub(super) inline_index: usize,
    pub(super) byte_index: usize,
    pub(super) names: Vec<String>,
    pub(super) name_occurrences: Vec<(usize, crate::definitions::RecognizedName)>,
    pub(super) form_breaks: Vec<usize>,
}

/// Validate the unchanged leading paragraph and record its name/form boundaries.
pub(super) fn entry_signature(
    item: &ListItem,
    role: EntryKind,
    explicitly_declared: bool,
    attached: AttachedValuePolicy,
) -> Result<EntrySignature, EntryRejection> {
    let source = item.blocks.first().and_then(block_source);
    let Some(Block::Paragraph { children, .. }) = item.blocks.first() else {
        return Err(EntryRejection::new(
            EntryRejectionReason::MissingLeadingParagraph,
            None,
            source,
        ));
    };
    let mut names = Vec::new();
    let mut name_occurrences = Vec::new();
    let mut leading_term = None;
    let mut form_breaks = Vec::new();
    let mut form_has_term = false;
    for (delimiter_inline, inline) in children.iter().enumerate() {
        if let Some(value) = entry_term_text(inline) {
            form_has_term = true;
            leading_term.get_or_insert(value);
            let parsed = entry_names(value, role, explicitly_declared, attached)
                .map_err(|reason| EntryRejection::new(reason, Some(value), source))?;
            extend_unique(
                &mut names,
                parsed.iter().map(|found| found.name.clone()).collect(),
            );
            name_occurrences.extend(parsed.into_iter().map(|found| (delimiter_inline, found)));
            continue;
        }
        match inline {
            Inline::Text { value } => {
                if let Some((delimiter_byte, _)) = delimiter_location(value) {
                    if names.is_empty() {
                        return Err(EntryRejection::new(
                            EntryRejectionReason::MissingLeadingCode,
                            leading_term,
                            source,
                        ));
                    }
                    if !form_has_term
                        || value[..delimiter_byte].contains('|')
                        || !is_alias_separator(&value[..delimiter_byte])
                    {
                        return Err(EntryRejection::new(
                            EntryRejectionReason::InvalidAliasSeparator,
                            leading_term,
                            source,
                        ));
                    }
                    return Ok(EntrySignature {
                        inline_index: delimiter_inline,
                        byte_index: delimiter_byte,
                        names,
                        name_occurrences,
                        form_breaks,
                    });
                }
                if names.is_empty() {
                    return Err(EntryRejection::new(
                        EntryRejectionReason::MissingLeadingCode,
                        leading_term,
                        source,
                    ));
                }
                if value.trim() == "|" && form_has_term {
                    form_breaks.push(delimiter_inline);
                    form_has_term = false;
                } else if value.contains('|') || !is_alias_separator(value) {
                    return Err(EntryRejection::new(
                        EntryRejectionReason::InvalidAliasSeparator,
                        leading_term,
                        source,
                    ));
                }
            }
            _ => {
                return Err(EntryRejection::new(
                    EntryRejectionReason::UnsupportedInline,
                    leading_term,
                    source,
                ));
            }
        }
    }
    Err(EntryRejection::new(
        if names.is_empty() {
            EntryRejectionReason::MissingLeadingCode
        } else {
            EntryRejectionReason::MissingDescription
        },
        leading_term,
        source,
    ))
}

/// Linked and unlinked terms share exactly the same name/form grammar.
pub(super) fn entry_term_text(inline: &Inline) -> Option<&str> {
    match inline {
        Inline::Code { value } => Some(value),
        Inline::Link {
            target: LinkTarget::Document { .. } | LinkTarget::Manual { .. },
            children,
            ..
        } => match children.as_slice() {
            [Inline::Code { value }] => Some(value),
            _ => None,
        },
        _ => None,
    }
}

fn extend_unique(output: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !output.contains(&value) {
            output.push(value);
        }
    }
}

fn is_alias_separator(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, ',' | '/' | '|'))
}

fn delimiter_location(value: &str) -> Option<(usize, usize)> {
    value.char_indices().find_map(|(index, character)| {
        matches!(character, ':' | '—' | '–').then_some((index, character.len_utf8()))
    })
}
