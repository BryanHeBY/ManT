//! Bounded logical outline fields, independent of folding and terminal width.

use std::{collections::HashMap, sync::Arc};

use super::references::{ReferenceLabelSource, ReferenceRecord, target_text};
use super::{DocumentView, NavKind};
use crate::text::sanitize_terminal_text;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_FIELD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct OutlineField {
    pub(crate) text: Arc<str>,
    pub(crate) target: bool,
    pub(crate) anchor: String,
}

#[derive(Debug, Clone)]
pub(crate) struct OutlineRecord {
    pub(crate) node_index: usize,
    pub(crate) label: Arc<str>,
    pub(crate) fields: Vec<OutlineField>,
}

impl DocumentView {
    pub(crate) fn outline_search_records(&self) -> (Vec<OutlineRecord>, bool) {
        let references: HashMap<_, _> = self
            .references
            .iter()
            .map(|record| (record.id.as_ref(), record))
            .collect();
        let mut remaining = MAX_BYTES;
        let mut limited = self.references_limited;
        let mut records = Vec::new();
        for (node_index, node) in self.navigation.iter().enumerate() {
            if matches!(
                node.kind,
                NavKind::EntryGroup | NavKind::ReferenceGroup | NavKind::ReferenceNotice
            ) {
                continue;
            }
            let title = node.full_title.as_deref().unwrap_or(&node.title);
            let title = if node.kind == NavKind::Reference {
                title.strip_prefix("↗ ").unwrap_or(title)
            } else {
                title
            };
            let reference = references.get(node.id.as_str());
            let title = if reference
                .is_some_and(|record| record.label_source == ReferenceLabelSource::Truncated)
            {
                title.strip_suffix('…').unwrap_or(title)
            } else {
                title
            };
            limited |= reference
                .is_some_and(|record| record.label_source == ReferenceLabelSource::Truncated);
            let label = bounded(title, &mut remaining, &mut limited);
            if label.is_empty() {
                continue;
            }
            let mut fields = Vec::new();
            if !reference
                .is_some_and(|record| record.label_source == ReferenceLabelSource::Generated)
            {
                fields.push(OutlineField {
                    text: Arc::clone(&label),
                    target: false,
                    anchor: node.target_id.clone(),
                });
            }
            if matches!(node.kind, NavKind::Entry(_)) && node.title != label.as_ref() {
                let compact = bounded(&node.title, &mut remaining, &mut limited);
                if !compact.is_empty() {
                    fields.push(OutlineField {
                        text: compact,
                        target: false,
                        anchor: node.target_id.clone(),
                    });
                }
            }
            if let Some(record) = references.get(node.id.as_str()) {
                reference_fields(record, &mut fields, &mut remaining, &mut limited);
            }
            if let Some(indices) = self.associated_references.get(&node.id) {
                for index in indices {
                    let record = &self.references[*index];
                    reference_fields(record, &mut fields, &mut remaining, &mut limited);
                }
            }
            records.push(OutlineRecord {
                node_index,
                label,
                fields,
            });
        }
        (records, limited)
    }
}

fn reference_fields(
    record: &ReferenceRecord,
    fields: &mut Vec<OutlineField>,
    remaining: &mut usize,
    limited: &mut bool,
) {
    let truncated = record.label_source == ReferenceLabelSource::Truncated;
    *limited |= truncated;
    for (text, target) in [
        (record.label.clone(), false),
        (target_text(&record.target), true),
    ] {
        if !target && record.label_source == ReferenceLabelSource::Generated {
            continue;
        }
        let text = if !target && truncated {
            text.strip_suffix('…').unwrap_or(&text).to_owned()
        } else {
            text
        };
        if fields.iter().any(|field| field.text.as_ref() == text) {
            continue;
        }
        let text = bounded(&text, remaining, limited);
        if !text.is_empty() {
            fields.push(OutlineField {
                text,
                target,
                anchor: record.id.to_string(),
            });
        }
    }
}

fn bounded(text: &str, remaining: &mut usize, limited: &mut bool) -> Arc<str> {
    let text = sanitize_terminal_text(text);
    let limit = (*remaining).min(MAX_FIELD_BYTES);
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    *limited |= end < text.len();
    *remaining -= end;
    Arc::from(&text[..end])
}
