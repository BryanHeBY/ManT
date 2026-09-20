//! Validate original-source tables and spans without mixing rendered coordinates.

use super::document::invariant_at;
use crate::{
    Block, Diagnostic, Document, Inline, SourceCoordinates, SourceIdentity, SourceKey,
    SourceRelationError, SourceSpan, ValueDomain,
    visit::{self, Visit},
};

/// Validate the source table and every source-qualified relation in a document.
///
/// This is a hard structural boundary used by [`serde::Deserialize`] for
/// [`Document`]. It deliberately cannot prove that a producer selected the
/// factually correct *valid* source; producer/oracle tests own that check.
///
/// # Errors
///
/// Returns an error when the source table is invalid or any source-qualified
/// relation in the document references an unknown or incompatible source.
pub fn validate_document_sources(document: &Document) -> Result<(), SourceRelationError> {
    validate_source_table(&document.sources, document.root_source)?;

    for diagnostic in &document.diagnostics {
        if let Some(span) = diagnostic.source {
            validate_relation_span(document, span)?;
        }
    }

    let mut collector = SourceRelationCollector {
        document,
        error: None,
        saw_span: false,
    };
    collector.visit_document(document);
    match collector.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Return whether any document diagnostic or content node retains a source span.
#[must_use]
pub fn document_has_source_spans(document: &Document) -> bool {
    if document
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.source.is_some())
    {
        return true;
    }
    let mut collector = SourceRelationCollector {
        document,
        error: None,
        saw_span: false,
    };
    collector.visit_document(document);
    collector.saw_span
}

/// Validate a standalone source table and its root key.
///
/// Protocol envelopes that carry source-qualified spans without a complete
/// [`Document`] use this same boundary before validating their own spans.
///
/// # Errors
///
/// Returns an error when the table is empty or non-dense, the root is not the
/// first source, an identity is invalid, or participating formats disagree.
pub fn validate_source_table(
    sources: &[crate::SourceRecord],
    root_source: SourceKey,
) -> Result<(), SourceRelationError> {
    let Some(root) = sources.first() else {
        return Err(relation("document source table must not be empty"));
    };

    for (index, source) in sources.iter().enumerate() {
        let one_based = index
            .checked_add(1)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(SourceKey::new)
            .ok_or_else(|| relation("document source table exceeds the SourceKey domain"))?;
        if source.key != one_based {
            return Err(relation(format!(
                "source table key {} is not dense key {}",
                source.key.get(),
                one_based.get()
            )));
        }
        validate_identity(&source.identity)?;
        if source.format != root.format {
            return Err(relation(format!(
                "source {} format does not match the active root format",
                source.key.get()
            )));
        }
    }

    if root.key != SourceKey::FIRST || root_source != SourceKey::FIRST {
        return Err(relation(
            "the root source must enter first and use SourceKey 1",
        ));
    }
    Ok(())
}

fn validate_identity(identity: &SourceIdentity) -> Result<(), SourceRelationError> {
    let name = identity.name();
    if name.is_empty() {
        return Err(relation("source identity must not be empty"));
    }
    if name
        .chars()
        .any(|character| character == '\0' || character.is_control())
    {
        return Err(relation(
            "source identity must not contain control characters",
        ));
    }
    if let SourceIdentity::BundleMember { name } = identity
        && (name.starts_with('/')
            || name.ends_with('/')
            || name.contains('\\')
            || name
                .split('/')
                .any(|component| component.is_empty() || component == "." || component == ".."))
    {
        return Err(relation(
            "bundle member identity must be a normalized relative path",
        ));
    }
    Ok(())
}

fn validate_relation_span(
    document: &Document,
    span: SourceSpan,
) -> Result<(), SourceRelationError> {
    validate_source_span_relation(&document.sources, span)
}

/// Validate one span against its selected record in a previously validated table.
///
/// # Errors
///
/// Returns an error when the key is unknown, coordinates are not one-based or
/// ordered, or an exact byte range is incompatible with its selected source.
pub fn validate_source_span_relation(
    sources: &[crate::SourceRecord],
    span: SourceSpan,
) -> Result<(), SourceRelationError> {
    let index = usize::try_from(span.source.get() - 1)
        .map_err(|_| relation("source key does not fit this platform"))?;
    let source = sources
        .get(index)
        .filter(|source| source.key == span.source)
        .ok_or_else(|| {
            relation(format!(
                "source span references unknown SourceKey {}",
                span.source.get()
            ))
        })?;
    if span.line == 0 || span.column == 0 {
        return Err(relation("source span lines and columns must be one-based"));
    }
    if span.end_line.is_none() != span.end_column.is_none() {
        return Err(relation(
            "source span end line and column must be supplied together",
        ));
    }
    if let (Some(end_line), Some(end_column)) = (span.end_line, span.end_column) {
        if end_line == 0 || end_column == 0 {
            return Err(relation(
                "source span end lines and columns must be one-based",
            ));
        }
        if end_line < span.line || (end_line == span.line && end_column < span.column) {
            return Err(relation("source span end position precedes its start"));
        }
    }
    if let Some(range) = span.byte_range {
        if source.coordinates != SourceCoordinates::DecodedUtf8Bytes {
            return Err(relation(
                "native-normalized source coordinates cannot carry an exact byte range",
            ));
        }
        if range.end < range.start {
            return Err(relation("source byte range ends before it starts"));
        }
        if range.end.get() > source.decoded_byte_length {
            return Err(relation(format!(
                "source byte range ends at {}, beyond source {} length {}",
                range.end.get(),
                source.key.get(),
                source.decoded_byte_length
            )));
        }
    }
    Ok(())
}

fn relation(detail: impl Into<String>) -> SourceRelationError {
    SourceRelationError::new(detail)
}

struct SourceRelationCollector<'document> {
    document: &'document Document,
    error: Option<SourceRelationError>,
    saw_span: bool,
}

impl SourceRelationCollector<'_> {
    fn span(&mut self, source: Option<SourceSpan>) {
        self.saw_span |= source.is_some();
        if self.error.is_none()
            && let Some(source) = source
            && let Err(error) = validate_relation_span(self.document, source)
        {
            self.error = Some(error);
        }
    }

    fn entry(&mut self, entry: crate::EntryOwner<'_>) {
        self.span(entry.source());
        if let Some(ValueDomain::EntrySet { source, .. }) =
            entry.facts().and_then(|facts| facts.value_domain.as_ref())
        {
            self.span(*source);
        }
    }
}

impl<'ir> Visit<'ir> for SourceRelationCollector<'_> {
    fn visit_heading(&mut self, heading: &'ir crate::Heading) {
        self.span(heading.source);
        visit::walk_heading(self, heading);
    }

    fn visit_section(&mut self, section: &'ir crate::Section) {
        self.span(section.source);
        visit::walk_section(self, section);
    }

    fn visit_block(&mut self, block: &'ir Block) {
        let source = match block {
            Block::Paragraph { source, .. }
            | Block::Preformatted { source, .. }
            | Block::List { source, .. }
            | Block::DefinitionList { source, .. }
            | Block::Table { source, .. }
            | Block::Equation { source, .. }
            | Block::VerticalSpace { source, .. }
            | Block::ThematicBreak { source }
            | Block::Unsupported { source, .. } => *source,
        };
        self.span(source);
        visit::walk_block(self, block);
    }

    fn visit_definition_item(&mut self, item: &'ir crate::DefinitionItem) {
        self.entry(crate::EntryOwner::Definition(item));
        visit::walk_definition_item(self, item);
    }

    fn visit_list_item(&mut self, item: &'ir crate::ListItem) {
        self.entry(crate::EntryOwner::List(item));
        visit::walk_list_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Anchor { owner_source, .. } = inline {
            self.span(*owner_source);
        }
        visit::walk_inline(self, inline);
    }
}

pub(super) fn validate_source_span(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan) {
    if source.line == 0 || source.column == 0 {
        diagnostics.push(invariant_at(
            "ir.invalid-source-position",
            "source lines and columns must be one-based".to_owned(),
            source,
        ));
    }
    if source.end_line == Some(0) || source.end_column == Some(0) {
        diagnostics.push(invariant_at(
            "ir.invalid-source-position",
            "source end lines and columns must be one-based".to_owned(),
            source,
        ));
    }
    if source.end_line.is_none() != source.end_column.is_none() {
        diagnostics.push(invariant_at(
            "ir.incomplete-source-end",
            "source end line and column must be supplied together".to_owned(),
            source,
        ));
    }
    if let (Some(end_line), Some(end_column)) = (source.end_line, source.end_column)
        && (end_line < source.line || (end_line == source.line && end_column < source.column))
    {
        diagnostics.push(invariant_at(
            "ir.reverse-source-position",
            "source end position precedes its start".to_owned(),
            source,
        ));
    }
    if source
        .byte_range
        .is_some_and(|range| range.end < range.start)
    {
        diagnostics.push(invariant_at(
            "ir.reverse-source-range",
            "source byte range ends before it starts".to_owned(),
            source,
        ));
    }
}
