//! Closed, annotation-only entry metadata over original parser item identities.
use std::collections::{BTreeMap, BTreeSet};

use mant_ir::{Diagnostic, DiagnosticLevel, SourceSpan};
use pulldown_cmark::{Event, Tag, TagEnd};
use serde::Deserialize;

use super::{SpannedEvent, source::MarkdownSource};

mod apply;
pub(super) use apply::apply;

/// Bound the object before JSON allocation. The closed shape permits at most
/// object -> groups -> members nesting; recursive/unknown values are rejected.
const MAX_METADATA_BYTES: usize = 8192;
const MAX_GROUPS: usize = 32;
const MAX_GROUP_MEMBERS: usize = 32;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EntryMetadata {
    #[serde(default, deserialize_with = "present")]
    pub(super) id: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub(super) alias_groups: Option<Vec<Vec<String>>>,
    #[serde(default, deserialize_with = "present")]
    pub(super) alias_of: Option<String>,
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Debug, Clone)]
pub(super) struct MetadataDeclaration {
    pub(super) value: Option<EntryMetadata>,
    pub(super) source: SourceSpan,
}

pub(super) type MetadataDeclarations = BTreeMap<usize, MetadataDeclaration>;

/// Unlike valid syntax recognition, masking also accepts an unclosed comment.
/// It remains one original parser event, never joined to a later event.
pub(super) fn payload(raw: &str) -> Option<&str> {
    let rest = raw
        .trim_start()
        .strip_prefix("<!--")?
        .trim_start()
        .strip_prefix("mant:entry")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with("-->"))
        .then_some(rest)
}

pub(super) fn collect(
    events: &[SpannedEvent<'_>],
    source: &str,
    masked: &mut [u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> MetadataDeclarations {
    let source_map = MarkdownSource::new(source);
    let mut declarations = MetadataDeclarations::new();
    let mut stack = Vec::new();
    let mut first_paragraph = BTreeSet::new();
    let mut completed_head = BTreeSet::new();
    for (index, (event, range)) in events.iter().enumerate() {
        match event {
            Event::Start(tag) => {
                if matches!(tag, Tag::Paragraph)
                    && let Some((TagEnd::Item, item)) = stack.last()
                    && !first_paragraph.insert(*item)
                {
                    completed_head.insert(*item);
                }
                stack.push((tag.to_end(), range.start));
            }
            Event::End(_) => {
                if let Some((TagEnd::Paragraph, _)) = stack.pop()
                    && let Some((TagEnd::Item, item)) = stack.last()
                {
                    completed_head.insert(*item);
                }
            }
            Event::Html(raw) | Event::InlineHtml(raw) if payload(raw).is_some() => {
                let span = source_map.span(range);
                // Preserve line endings and byte coordinates, not comment text.
                for byte in &mut masked[range.clone()] {
                    if !matches!(*byte, b'\r' | b'\n') {
                        *byte = b' ';
                    }
                }
                let owner = metadata_owner(&stack, event);
                let Some(owner) = owner else {
                    diagnostic(
                        diagnostics,
                        span,
                        "metadata must belong directly to an explicitly declared item, at the end of its first paragraph or in a standalone comment block",
                    );
                    continue;
                };
                if let Some(previous) = declarations.get_mut(&owner) {
                    if previous.value.take().is_some() {
                        diagnostic(
                            diagnostics,
                            span,
                            "more than one entry metadata object belongs to this item; all objects were omitted",
                        );
                    }
                    continue;
                }
                let inline_end = !matches!(event, Event::InlineHtml(_))
                    || (!completed_head.contains(&owner) && head_ends_after(events, index));
                let value = if inline_end {
                    read(raw, span, diagnostics)
                } else {
                    diagnostic(
                        diagnostics,
                        span,
                        "inline entry metadata must end the first paragraph",
                    );
                    None
                };
                declarations.insert(
                    owner,
                    MetadataDeclaration {
                        value,
                        source: span,
                    },
                );
            }
            _ => {}
        }
    }
    declarations
}

fn metadata_owner(stack: &[(TagEnd, usize)], event: &Event<'_>) -> Option<usize> {
    if stack.iter().any(|(tag, _)| {
        !matches!(
            tag,
            TagEnd::List(_) | TagEnd::Item | TagEnd::Paragraph | TagEnd::HtmlBlock
        )
    }) {
        return None;
    }
    let (position, (_, owner)) = stack
        .iter()
        .enumerate()
        .rev()
        .find(|(_, (tag, _))| *tag == TagEnd::Item)?;
    let suffix = &stack[position + 1..];
    match event {
        Event::Html(_) if matches!(suffix, [(TagEnd::HtmlBlock, _)]) => Some(*owner),
        Event::InlineHtml(_) if suffix.is_empty() || matches!(suffix, [(TagEnd::Paragraph, _)]) => {
            Some(*owner)
        }
        _ => None,
    }
}

fn head_ends_after(events: &[SpannedEvent<'_>], index: usize) -> bool {
    for (event, _) in &events[index + 1..] {
        match event {
            Event::End(TagEnd::Paragraph | TagEnd::Item) | Event::Start(Tag::List(_)) => {
                return true;
            }
            Event::Text(text) if text.trim().is_empty() => {}
            Event::SoftBreak | Event::HardBreak => {}
            Event::InlineHtml(raw) if payload(raw).is_some() => {}
            _ => return false,
        }
    }
    false
}

fn read(raw: &str, source: SourceSpan, diagnostics: &mut Vec<Diagnostic>) -> Option<EntryMetadata> {
    let rest = payload(raw).expect("recognized directive");
    let Some(json) = rest.trim().strip_suffix("-->").map(str::trim) else {
        diagnostic(
            diagnostics,
            source,
            "entry metadata needs one closed comment containing one JSON object",
        );
        return None;
    };
    if json.len() > MAX_METADATA_BYTES || json.contains(['\r', '\n']) {
        diagnostic(
            diagnostics,
            source,
            "entry metadata must fit one physical line and 8192 bytes",
        );
        return None;
    }
    let value: EntryMetadata = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            diagnostic(
                diagnostics,
                source,
                &format!("invalid entry metadata object: {error}"),
            );
            return None;
        }
    };
    if value.alias_groups.as_ref().is_some_and(|groups| {
        groups.len() > MAX_GROUPS || groups.iter().any(|g| g.len() > MAX_GROUP_MEMBERS)
    }) {
        diagnostic(
            diagnostics,
            source,
            "entry metadata exceeds 32 groups or 32 members per group",
        );
        return None;
    }
    Some(value)
}

pub(super) fn diagnostic(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan, message: &str) {
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-entry-metadata".into()),
        message: message.into(),
        source: Some(source),
    });
}
