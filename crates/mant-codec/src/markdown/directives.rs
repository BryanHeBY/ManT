//! Collect declarations from original events without reparsing masked Markdown.
use super::bindings::{ItemBindings, OriginalItemId, OriginalListId};
mod syntax;
use mant_ir::{Diagnostic, DiagnosticLevel, EntryKind, NameCase, SourceSpan, ValueDomain};
use pulldown_cmark::{Event, Tag, TagEnd};
use std::collections::BTreeMap;
use syntax::{
    directive_source, is_semantic_directive, mask_directive, read_declaration,
    read_domain_declaration, source_line_index,
};

/// Events and declarations always describe this exact borrowed source.
pub(super) struct PreparedMarkdown<'a> {
    pub(super) source: &'a str,
    pub(super) display_source: String,
    pub(super) events: Vec<super::SpannedEvent<'a>>,
    pub(super) declarations: SemanticDeclarations,
}
impl<'a> PreparedMarkdown<'a> {
    pub(super) fn new(source: &'a str, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let (events, declarations, display_source) =
            extract_semantic_directives(source, diagnostics);
        Self {
            source,
            display_source,
            events,
            declarations,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EntryDeclaration {
    pub(super) role: EntryKind,
    pub(super) case: NameCase,
    pub(super) attached: AttachedValuePolicy,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SemanticDeclarations {
    pub(super) entries: BTreeMap<OriginalListId, EntryDeclaration>,
    pub(super) bindings: ItemBindings,
}

impl SemanticDeclarations {
    /// Close collection only after every root and section has been normalized.
    pub(super) fn report_unattached(&mut self, diagnostics: &mut Vec<Diagnostic>) {
        for declaration in std::mem::take(&mut self.entries).into_values() {
            semantic_diagnostic(
                diagnostics,
                declaration.source,
                "semantic-entry directive did not resolve to a Markdown list".into(),
            );
        }
        for declaration in std::mem::take(&mut self.bindings.domains)
            .into_values()
            .filter_map(DomainDeclarationState::into_unique)
        {
            domain_diagnostic(
                diagnostics,
                declaration.source,
                "semantic value-domain directive did not resolve to a semantic entry".into(),
            );
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct DomainDeclaration {
    pub(super) value: ValueDomain,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone)]
pub(super) enum DomainDeclarationState {
    Unique(DomainDeclaration),
    Ambiguous,
}

impl DomainDeclarationState {
    pub(super) fn into_unique(self) -> Option<DomainDeclaration> {
        match self {
            Self::Unique(declaration) => Some(declaration),
            Self::Ambiguous => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) enum AttachedValuePolicy {
    #[default]
    Infer,
    Fixed,
}

/// Consume directives in the original event stream. Never reparse masked
/// Markdown: HTML blocks can be the only boundary between adjacent lists.
fn extract_semantic_directives<'a>(
    source: &'a str,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<super::SpannedEvent<'a>>, SemanticDeclarations, String) {
    let mut masked = source.as_bytes().to_vec();
    let mut declarations = SemanticDeclarations::default();
    let lines = super::source::physical_lines(source).collect::<Vec<_>>();
    let line_starts = lines
        .iter()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset = offset.saturating_add(line.len());
            Some(start)
        })
        .collect::<Vec<_>>();
    let events = super::source::parser_events(source);
    declarations.bindings = ItemBindings::collect(&events);
    declarations.bindings.metadata =
        super::metadata::collect(&events, source, &mut masked, diagnostics);

    collect_entry_declarations(
        &events,
        &lines,
        &line_starts,
        &mut masked,
        &mut declarations,
        diagnostics,
    );
    collect_domain_declarations(
        &events,
        &lines,
        &line_starts,
        &mut masked,
        &mut declarations,
        diagnostics,
    );

    let mut output = Vec::with_capacity(events.len());
    let mut events = events.into_iter();
    while let Some((event, range)) = events.next() {
        if let Event::InlineHtml(raw) = &event
            && super::metadata::payload(raw).is_some()
        {
            continue;
        }
        if event != Event::Start(Tag::HtmlBlock) {
            output.push((event, range));
            continue;
        }
        let start = (event, range);
        let mut body = Vec::new();
        for (event, range) in events.by_ref() {
            if event == Event::End(TagEnd::HtmlBlock) {
                if !body.is_empty() {
                    output.push(start);
                    output.append(&mut body);
                    output.push((event, range));
                }
                break;
            }
            let retained =
                std::str::from_utf8(&masked[range.clone()]).expect("ASCII masking preserves UTF-8");
            if !retained.trim().is_empty() {
                body.push((Event::Html(retained.to_owned().into()), range));
            }
        }
    }
    (
        output,
        declarations,
        String::from_utf8(masked).expect("ASCII masking preserves UTF-8"),
    )
}

fn collect_entry_declarations(
    events: &[(Event<'_>, std::ops::Range<usize>)],
    lines: &[&str],
    line_starts: &[usize],
    masked: &mut [u8],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut item_offsets = Vec::new();
    for (event_index, (event, range)) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::Item) => {
                item_offsets.push(OriginalItemId(range.start));
            }
            Event::End(TagEnd::Item) => {
                item_offsets.pop();
            }
            _ => {}
        }
        let Event::Html(raw) = event else {
            continue;
        };
        if !is_semantic_directive(raw, "mant:entries") {
            continue;
        }
        let Some(block_end_index) = events[event_index + 1..]
            .iter()
            .position(|(event, _)| matches!(event, Event::End(TagEnd::HtmlBlock)))
            .map(|relative| event_index + relative + 1)
        else {
            continue;
        };
        let index = source_line_index(line_starts, range.start);
        let line = lines[index];
        let without_newline = line.trim_end_matches(['\r', '\n']);
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let Some(declaration) = read_declaration(
            without_newline,
            line_starts[index],
            line_number,
            masked,
            diagnostics,
        ) else {
            declarations
                .bindings
                .incomplete_entry_children
                .extend(item_offsets.last().copied());
            continue;
        };
        let source_span = declaration.source;
        let Some((Event::Start(Tag::List(_)), target_range)) = events.get(block_end_index + 1)
        else {
            declarations
                .bindings
                .incomplete_entry_children
                .extend(item_offsets.last().copied());
            semantic_diagnostic(
                diagnostics,
                source_span,
                "semantic-entry directive must immediately precede a list".to_owned(),
            );
            continue;
        };
        if declarations
            .entries
            .insert(OriginalListId(target_range.start), declaration)
            .is_some()
        {
            declarations
                .bindings
                .incomplete_entry_children
                .extend(item_offsets.last().copied());
            semantic_diagnostic(
                diagnostics,
                source_span,
                "more than one semantic-entry directive targets the same list".to_owned(),
            );
        }
    }
}

fn collect_domain_declarations(
    events: &[(Event<'_>, std::ops::Range<usize>)],
    lines: &[&str],
    line_starts: &[usize],
    masked: &mut [u8],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut item_offsets = Vec::new();
    for (event, range) in events {
        match event {
            Event::Start(Tag::Item) => {
                item_offsets.push(OriginalItemId(range.start));
            }
            Event::End(TagEnd::Item) => {
                item_offsets.pop();
            }
            Event::Html(raw) if is_semantic_directive(raw, "mant:domain") => {
                let index = source_line_index(line_starts, range.start);
                let line = lines[index].trim_end_matches(['\r', '\n']);
                let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
                let Some(item_offset) = item_offsets.last().copied() else {
                    let source_span = directive_source(line, line_starts[index], line_number);
                    mask_directive(line, line_starts[index], masked);
                    domain_diagnostic(
                        diagnostics,
                        source_span,
                        "semantic value-domain directive must be inside a list item".to_owned(),
                    );
                    continue;
                };
                let Some(declaration) = read_domain_declaration(
                    line,
                    line_starts[index],
                    line_number,
                    masked,
                    diagnostics,
                ) else {
                    continue;
                };
                let source_span = declaration.source;
                let duplicate = match declarations.bindings.domains.entry(item_offset) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(DomainDeclarationState::Unique(declaration));
                        false
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        entry.insert(DomainDeclarationState::Ambiguous);
                        true
                    }
                };
                if duplicate {
                    domain_diagnostic(
                        diagnostics,
                        source_span,
                        "more than one semantic value-domain directive targets the same entry"
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
    }
}

pub(super) fn semantic_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    source: SourceSpan,
    message: String,
) {
    diagnostics.push(Diagnostic {
        impact: mant_ir::DiagnosticImpact::SemanticCoverage,
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-entry-list".to_owned()),
        message,
        source: Some(source),
    });
}

pub(super) fn domain_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    source: SourceSpan,
    message: String,
) {
    diagnostics.push(Diagnostic {
        impact: mant_ir::DiagnosticImpact::SemanticCoverage,
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-value-domain".to_owned()),
        message,
        source: Some(source),
    });
}
