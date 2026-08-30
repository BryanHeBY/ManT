//! Searches deterministic Markdown while retaining addressable manual nodes.
//!
//! Section and semantic-entry anchors emitted by the Markdown renderer form
//! an internal source map. pulldown-cmark supplies a visible-text projection
//! whose byte ranges map back into that exact Markdown document.

use std::{error::Error, fmt, ops::Range};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use mant_protocol::{
    MAX_SEARCH_PATTERN_CHARS, MarkdownSchema, QuerySearch, SearchCase, SearchContextLine,
    SearchHit, SearchLineRange, SearchMarkdownRange, SearchOccurrence, SearchQuery, SearchRender,
    SearchRenderFormat, SearchRenderScope, SearchSchema, SearchScope, SearchSyntax,
};
use pulldown_cmark::{Event, Parser, TagEnd};
use regex_syntax::ParserBuilder;

use crate::markdown_mapping::{InlineMappingKind, map_inline_characters};
use crate::{ResolvedContent, output::render_addressable_markdown};

mod owners;

use owners::{Owner, OwnerIndex};

const MAX_CONTEXT_LINES: u16 = 100;
const MAX_SEARCH_LIMIT: u32 = 10_000;
const MAX_OCCURRENCES_PER_MATCH: usize = 256;

/// Invalid search input or matcher construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// Search pattern contained no bytes.
    EmptyPattern,
    /// Search pattern exceeded the request bound.
    PatternTooLong,
    /// Result limit was zero or exceeded the protocol maximum.
    InvalidLimit,
    /// Requested context exceeded the protocol maximum.
    ContextTooLarge,
    /// Regular-expression compilation or execution failed.
    InvalidPattern(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => formatter.write_str("search pattern must not be empty"),
            Self::PatternTooLong => write!(
                formatter,
                "search pattern exceeds the {MAX_SEARCH_PATTERN_CHARS}-character limit"
            ),
            Self::InvalidLimit => write!(
                formatter,
                "search limit must be between 1 and {MAX_SEARCH_LIMIT}"
            ),
            Self::ContextTooLarge => write!(
                formatter,
                "search context must not exceed {MAX_CONTEXT_LINES} lines"
            ),
            Self::InvalidPattern(message) => write!(formatter, "invalid search pattern: {message}"),
        }
    }
}

impl Error for SearchError {}

/// Search one complete query and report coordinates in its canonical Markdown.
///
/// # Errors
///
/// Returns [`SearchError`] for empty or excessive inputs and invalid regular
/// expressions. A valid search with no matches is a successful empty result.
pub fn search_query(
    query: &ResolvedContent,
    request: &SearchQuery,
) -> Result<QuerySearch, SearchError> {
    validate_request(request)?;
    let artifact = render_addressable_markdown(query);
    let markdown = &artifact.text;
    let lines = LineIndex::new(markdown);
    let owners = OwnerIndex::new(&artifact);
    let searchable = SearchableText::new(markdown, request.scope);
    let matcher = build_matcher(request)?;
    let offset = usize::try_from(request.offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(request.limit).unwrap_or(usize::MAX);
    let mut collector = SearchCollector::new(markdown, &lines, offset, limit);
    let mut invalid_utf8_match = false;
    let mut invalid_zero_width_match = false;
    let mut invalid_mapped_range = false;

    matcher
        .find_iter(searchable.text.as_bytes(), |found| {
            if found.start() == found.end() {
                invalid_zero_width_match = true;
                return false;
            }
            if !searchable.text.is_char_boundary(found.start())
                || !searchable.text.is_char_boundary(found.end())
            {
                invalid_utf8_match = true;
                return false;
            }
            let markdown_start = searchable.markdown_start(found.start());
            let markdown_end = searchable.markdown_end(found.end());
            if !markdown.is_char_boundary(markdown_start)
                || !markdown.is_char_boundary(markdown_end)
            {
                invalid_utf8_match = true;
                return false;
            }
            if markdown_start >= markdown_end {
                invalid_mapped_range = true;
                return false;
            }
            let owner = owners.owner(markdown_start);
            let end_owner = owners.owner(markdown_end - 1);
            if let (Some(owner), Some(end_owner)) = (owner, end_owner)
                && owner.key == end_owner.key
            {
                collector.push(
                    RawOccurrence {
                        searchable: found.start()..found.end(),
                        markdown: markdown_start..markdown_end,
                    },
                    owner,
                );
            }
            true
        })
        .map_err(|error| SearchError::InvalidPattern(error.to_string()))?;
    if invalid_utf8_match {
        return Err(non_utf8_pattern_error());
    }
    if invalid_zero_width_match {
        return Err(empty_match_error());
    }
    if invalid_mapped_range {
        return Err(SearchError::InvalidPattern(
            "pattern produced a range that cannot be mapped to canonical Markdown".to_owned(),
        ));
    }

    let (raw_groups, total) = collector.finish();
    let selected = raw_groups
        .iter()
        .map(|found| {
            build_match(
                found,
                &searchable.text,
                markdown,
                &lines,
                request.context_lines,
            )
        })
        .collect::<Vec<_>>();
    let returned = u32::try_from(selected.len()).unwrap_or(u32::MAX);
    let consumed = request.offset.saturating_add(returned);
    let truncated = consumed < total;

    Ok(QuerySearch {
        schema: SearchSchema::V0Dot10,
        label: query.label.clone(),
        source: query
            .document
            .as_ref()
            .map(|document| document.source.clone()),
        meta: query
            .document
            .as_ref()
            .map(|document| document.meta.clone()),
        query: request.clone(),
        render: SearchRender {
            schema: MarkdownSchema::V1,
            format: SearchRenderFormat::Markdown,
            scope: SearchRenderScope::Full,
            line_base: 1,
            column_base: 1,
            line_count: u32::try_from(lines.count()).unwrap_or(u32::MAX),
        },
        total,
        returned,
        offset: request.offset,
        truncated,
        next_offset: truncated.then_some(consumed),
        matches: selected,
    })
}

/// Validate search limits and compile its matcher without loading a manual.
///
/// # Errors
///
/// Returns the same [`SearchError`] variants as [`search_query`].
pub fn validate_search_query(request: &SearchQuery) -> Result<(), SearchError> {
    validate_request(request)?;
    build_matcher(request).map(|_| ())
}

fn validate_request(request: &SearchQuery) -> Result<(), SearchError> {
    if request.pattern.is_empty() {
        return Err(SearchError::EmptyPattern);
    }
    if request.pattern.chars().count() > MAX_SEARCH_PATTERN_CHARS {
        return Err(SearchError::PatternTooLong);
    }
    if request.limit == 0 || request.limit > MAX_SEARCH_LIMIT {
        return Err(SearchError::InvalidLimit);
    }
    if request.context_lines > MAX_CONTEXT_LINES {
        return Err(SearchError::ContextTooLarge);
    }
    Ok(())
}

fn build_matcher(request: &SearchQuery) -> Result<grep_regex::RegexMatcher, SearchError> {
    validate_pattern_semantics(request)?;
    let mut builder = RegexMatcherBuilder::new();
    builder
        .fixed_strings(request.syntax == SearchSyntax::Literal)
        .multi_line(true)
        .word(request.word);
    match request.case {
        SearchCase::Insensitive => {
            builder.case_insensitive(true);
        }
        SearchCase::Sensitive => {
            builder.case_insensitive(false);
        }
        SearchCase::Smart => {
            builder.case_smart(true);
        }
    }
    let matcher = builder
        .build(&request.pattern)
        .map_err(|error| SearchError::InvalidPattern(error.to_string()))?;
    if matcher
        .is_match(b"")
        .map_err(|error| SearchError::InvalidPattern(error.to_string()))?
    {
        return Err(empty_match_error());
    }
    Ok(matcher)
}

fn validate_pattern_semantics(request: &SearchQuery) -> Result<(), SearchError> {
    if request.syntax == SearchSyntax::Literal {
        return Ok(());
    }
    let hir = ParserBuilder::new()
        .utf8(true)
        .unicode(true)
        .build()
        .parse(&request.pattern)
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("pattern can match invalid UTF-8") {
                non_utf8_pattern_error()
            } else {
                SearchError::InvalidPattern(message)
            }
        })?;
    if hir.properties().minimum_len() == Some(0) {
        return Err(empty_match_error());
    }
    Ok(())
}

fn empty_match_error() -> SearchError {
    SearchError::InvalidPattern("pattern must not match empty text".to_owned())
}

fn non_utf8_pattern_error() -> SearchError {
    SearchError::InvalidPattern(
        "regular expressions must preserve UTF-8 character boundaries; Unicode mode cannot be disabled"
            .to_owned(),
    )
}

struct RawOccurrence {
    searchable: Range<usize>,
    markdown: Range<usize>,
}

struct RawMatchGroup {
    ordinal: u32,
    occurrences: Vec<RawOccurrence>,
    occurrence_count: u32,
    owner: Owner,
    start_line_index: usize,
    end_line_index: usize,
}

struct PendingRawMatchGroup {
    ordinal: u32,
    occurrences: Vec<RawOccurrence>,
    occurrence_count: u32,
    owner: PendingOwner,
    start_line_index: usize,
    end_line_index: usize,
}

enum PendingOwner {
    Retained(Owner),
    CountOnly(usize),
}

impl PendingOwner {
    const fn key(&self) -> usize {
        match self {
            Self::Retained(owner) => owner.key,
            Self::CountOnly(key) => *key,
        }
    }
}

struct SearchCollector<'a> {
    markdown: &'a str,
    lines: &'a LineIndex,
    offset: usize,
    limit: usize,
    total: usize,
    selected: Vec<RawMatchGroup>,
    current: Option<PendingRawMatchGroup>,
}

impl<'a> SearchCollector<'a> {
    fn new(markdown: &'a str, lines: &'a LineIndex, offset: usize, limit: usize) -> Self {
        Self {
            markdown,
            lines,
            offset,
            limit,
            total: 0,
            selected: Vec::with_capacity(limit.min(256)),
            current: None,
        }
    }

    fn push(&mut self, occurrence: RawOccurrence, owner: &Owner) {
        let start_line_index = self
            .lines
            .position(self.markdown, occurrence.markdown.start)
            .line_index;
        let end_line_index = self
            .lines
            .line_index_at_byte(occurrence.markdown.end.saturating_sub(1));
        if let Some(group) = self.current.as_mut().filter(|group| {
            group.start_line_index == start_line_index
                && group.end_line_index == end_line_index
                && group.owner.key() == owner.key
        }) {
            group.occurrence_count = group.occurrence_count.saturating_add(1);
            if matches!(group.owner, PendingOwner::Retained(_))
                && group.occurrences.len() < MAX_OCCURRENCES_PER_MATCH
            {
                group.occurrences.push(occurrence);
            }
            return;
        }

        self.flush();
        let retained = self.total >= self.offset && self.selected.len() < self.limit;
        let occurrences = retained.then_some(occurrence).into_iter().collect();
        self.current = Some(PendingRawMatchGroup {
            ordinal: u32::try_from(self.total.saturating_add(1)).unwrap_or(u32::MAX),
            occurrences,
            occurrence_count: 1,
            owner: if retained {
                PendingOwner::Retained(owner.clone())
            } else {
                PendingOwner::CountOnly(owner.key)
            },
            start_line_index,
            end_line_index,
        });
    }

    fn flush(&mut self) {
        let Some(group) = self.current.take() else {
            return;
        };
        self.total = self.total.saturating_add(1);
        if let PendingOwner::Retained(owner) = group.owner {
            self.selected.push(RawMatchGroup {
                ordinal: group.ordinal,
                occurrences: group.occurrences,
                occurrence_count: group.occurrence_count,
                owner,
                start_line_index: group.start_line_index,
                end_line_index: group.end_line_index,
            });
        }
    }

    fn finish(mut self) -> (Vec<RawMatchGroup>, u32) {
        self.flush();
        (self.selected, u32::try_from(self.total).unwrap_or(u32::MAX))
    }
}

impl RawMatchGroup {
    fn occurrences_truncated(&self) -> bool {
        usize::try_from(self.occurrence_count).map_or(true, |count| count > self.occurrences.len())
    }
}

fn build_match(
    found: &RawMatchGroup,
    searchable: &str,
    markdown: &str,
    lines: &LineIndex,
    context_lines: u16,
) -> SearchHit {
    let first = &found.occurrences[0];
    let start = lines.position(markdown, first.markdown.start);
    let preview = display_markdown_line(lines.line(markdown, start.line_index));
    let context_start = found
        .start_line_index
        .saturating_sub(usize::from(context_lines));
    let context_end = found
        .end_line_index
        .saturating_add(usize::from(context_lines))
        .min(lines.count().saturating_sub(1));
    let context = if context_lines == 0 {
        Vec::new()
    } else {
        (context_start..=context_end)
            .map(|line_index| SearchContextLine {
                line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
                text: display_markdown_line(lines.line(markdown, line_index)),
                matched: (found.start_line_index..=found.end_line_index).contains(&line_index),
            })
            .collect()
    };

    SearchHit {
        ordinal: found.ordinal,
        outline: found.owner.outline.clone(),
        occurrences: found
            .occurrences
            .iter()
            .map(|occurrence| {
                let start = lines.position(markdown, occurrence.markdown.start);
                let end = lines.position(markdown, occurrence.markdown.end);
                SearchOccurrence {
                    matched_text: searchable[occurrence.searchable.clone()].to_owned(),
                    markdown: SearchMarkdownRange {
                        start_byte: u64::try_from(occurrence.markdown.start).unwrap_or(u64::MAX),
                        end_byte: u64::try_from(occurrence.markdown.end).unwrap_or(u64::MAX),
                        start_line: u32::try_from(start.line_index.saturating_add(1))
                            .unwrap_or(u32::MAX),
                        start_column: u32::try_from(start.column).unwrap_or(u32::MAX),
                        end_line: u32::try_from(end.line_index.saturating_add(1))
                            .unwrap_or(u32::MAX),
                        end_column: u32::try_from(end.column).unwrap_or(u32::MAX),
                    },
                    line_ranges: occurrence_line_ranges(occurrence, markdown, lines),
                }
            })
            .collect(),
        occurrence_count: found.occurrence_count,
        occurrences_truncated: found.occurrences_truncated(),
        node_source: found.owner.source,
        preview,
        context,
    }
}

fn occurrence_line_ranges(
    occurrence: &RawOccurrence,
    markdown: &str,
    lines: &LineIndex,
) -> Vec<SearchLineRange> {
    let start = lines
        .position(markdown, occurrence.markdown.start)
        .line_index;
    let end = lines.line_index_at_byte(occurrence.markdown.end.saturating_sub(1));
    (start..=end)
        .flat_map(|line_index| {
            let line_start = lines.start(line_index);
            let line = lines.line(markdown, line_index);
            let line_end = line_start.saturating_add(line.len());
            let intersection =
                occurrence.markdown.start.max(line_start)..occurrence.markdown.end.min(line_end);
            AnchorStrippedLine::new(line)
                .map_range(
                    intersection.start.saturating_sub(line_start)
                        ..intersection.end.saturating_sub(line_start),
                )
                .into_iter()
                .map(move |range| SearchLineRange {
                    line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
                    start_byte: u32::try_from(range.start).unwrap_or(u32::MAX),
                    end_byte: u32::try_from(range.end).unwrap_or(u32::MAX),
                })
        })
        .collect()
}

/// Hide `ManT`'s zero-width source-map anchors from human-facing snippets.
fn display_markdown_line(line: &str) -> String {
    AnchorStrippedLine::new(line.trim_end()).text
}

struct AnchorStrippedLine {
    text: String,
    segments: Vec<OffsetSegment>,
}

impl AnchorStrippedLine {
    fn new(line: &str) -> Self {
        let mut text = String::with_capacity(line.len());
        let mut segments = Vec::new();
        let mut cursor = 0;
        while let Some(relative_start) = line[cursor..].find("<a id=\"") {
            let anchor_start = cursor + relative_start;
            push_retained_line_segment(line, cursor..anchor_start, &mut text, &mut segments);
            let anchor = &line[anchor_start..];
            let Some(relative_end) = anchor.find("\"></a>") else {
                push_retained_line_segment(
                    line,
                    anchor_start..line.len(),
                    &mut text,
                    &mut segments,
                );
                return Self { text, segments };
            };
            cursor = anchor_start + relative_end + "\"></a>".len();
        }
        push_retained_line_segment(line, cursor..line.len(), &mut text, &mut segments);
        Self { text, segments }
    }

    fn map_range(&self, source: Range<usize>) -> Vec<Range<usize>> {
        self.segments
            .iter()
            .filter_map(|segment| {
                let start = source.start.max(segment.markdown.start);
                let end = source.end.min(segment.markdown.end);
                (start < end).then(|| {
                    segment.visible.start + start.saturating_sub(segment.markdown.start)
                        ..segment.visible.start + end.saturating_sub(segment.markdown.start)
                })
            })
            .collect()
    }
}

fn push_retained_line_segment(
    line: &str,
    source: Range<usize>,
    text: &mut String,
    segments: &mut Vec<OffsetSegment>,
) {
    if source.is_empty() {
        return;
    }
    let visible_start = text.len();
    text.push_str(&line[source.clone()]);
    segments.push(OffsetSegment {
        visible: visible_start..text.len(),
        markdown: source,
        linear: true,
    });
}

struct TextPosition {
    line_index: usize,
    column: usize,
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self { starts }
    }

    fn count(&self) -> usize {
        self.starts.len()
    }

    fn position(&self, text: &str, offset: usize) -> TextPosition {
        let offset = offset.min(text.len());
        let line_index = self.starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.starts[line_index];
        TextPosition {
            line_index,
            column: text[line_start..offset].chars().count().saturating_add(1),
        }
    }

    fn line_index_at_byte(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset) - 1
    }

    fn line<'a>(&self, text: &'a str, line_index: usize) -> &'a str {
        let start = self.starts[line_index];
        let end = self
            .starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(text.len());
        text[start..end]
            .strip_suffix('\n')
            .unwrap_or(&text[start..end])
    }

    fn start(&self, line_index: usize) -> usize {
        self.starts[line_index]
    }
}

struct SearchableText {
    text: String,
    segments: Vec<OffsetSegment>,
    direct_markdown: bool,
}

#[derive(Debug)]
struct OffsetSegment {
    visible: Range<usize>,
    markdown: Range<usize>,
    linear: bool,
}

impl SearchableText {
    fn new(markdown: &str, scope: SearchScope) -> Self {
        if scope == SearchScope::Markdown {
            return Self {
                text: markdown.to_owned(),
                segments: Vec::new(),
                direct_markdown: true,
            };
        }

        let mut visible = VisibleBuilder::new(markdown);
        for (event, source) in Parser::new(markdown).into_offset_iter() {
            match event {
                Event::Text(value) | Event::InlineMath(value) | Event::DisplayMath(value) => {
                    visible.push_mapped(&value, source, InlineMappingKind::Text);
                }
                Event::Code(value) => {
                    visible.push_mapped(&value, source, InlineMappingKind::Code);
                }
                Event::SoftBreak | Event::HardBreak | Event::Rule => visible.push_break(source),
                Event::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::Item
                    | TagEnd::CodeBlock
                    | TagEnd::TableRow,
                ) => visible.push_break(source.end..source.end),
                Event::Start(_)
                | Event::End(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_)
                | Event::TaskListMarker(_) => {}
            }
        }
        visible.finish()
    }

    fn markdown_start(&self, offset: usize) -> usize {
        if self.direct_markdown {
            return offset;
        }
        self.segment_at(offset).map_or(0, |segment| {
            if segment.linear {
                segment.markdown.start + offset.saturating_sub(segment.visible.start)
            } else {
                segment.markdown.start
            }
        })
    }

    fn markdown_end(&self, offset: usize) -> usize {
        if self.direct_markdown {
            return offset;
        }
        if offset == 0 {
            return 0;
        }
        self.segment_at(offset - 1).map_or(0, |segment| {
            if segment.linear {
                segment.markdown.start + offset.saturating_sub(segment.visible.start)
            } else {
                segment.markdown.end
            }
        })
    }

    fn segment_at(&self, offset: usize) -> Option<&OffsetSegment> {
        let index = self
            .segments
            .partition_point(|segment| segment.visible.end <= offset);
        self.segments
            .get(index)
            .filter(|segment| segment.visible.contains(&offset))
    }
}

struct VisibleBuilder<'a> {
    markdown: &'a str,
    text: String,
    segments: Vec<OffsetSegment>,
}

impl<'a> VisibleBuilder<'a> {
    fn new(markdown: &'a str) -> Self {
        Self {
            markdown,
            text: String::new(),
            segments: Vec::new(),
        }
    }

    fn push_mapped(&mut self, value: &str, source: Range<usize>, kind: InlineMappingKind) {
        for mapped in map_inline_characters(self.markdown, value, source, kind) {
            let visible_start = self.text.len();
            self.text.push(mapped.value);
            let visible_end = self.text.len();
            self.push_segment(OffsetSegment {
                visible: visible_start..visible_end,
                markdown: mapped.source,
                linear: mapped.linear,
            });
        }
    }

    fn push_break(&mut self, markdown: Range<usize>) {
        if self.text.ends_with('\n') || self.text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push('\n');
        self.push_segment(OffsetSegment {
            visible: start..self.text.len(),
            markdown,
            linear: false,
        });
    }

    fn push_segment(&mut self, segment: OffsetSegment) {
        if let Some(previous) = self.segments.last_mut() {
            let contiguous = previous.visible.end == segment.visible.start
                && previous.markdown.end == segment.markdown.start
                && previous.linear
                && segment.linear;
            if contiguous {
                previous.visible.end = segment.visible.end;
                previous.markdown.end = segment.markdown.end;
                return;
            }
        }
        self.segments.push(segment);
    }

    fn finish(self) -> SearchableText {
        SearchableText {
            text: self.text,
            segments: self.segments,
            direct_markdown: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ResolvedContent;
    use mant_ir::{
        Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Document,
        DocumentMeta, DocumentSource, Inline, LayoutHint, Section, SourceFormat,
    };
    use mant_protocol::{
        MAX_SEARCH_PATTERN_CHARS, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    };

    use super::{
        MAX_OCCURRENCES_PER_MATCH, SearchError, display_markdown_line, render_addressable_markdown,
        search_query, validate_search_query,
    };

    fn query() -> ResolvedContent {
        ResolvedContent {
            address: None,
            label: "demo".to_owned(),
            document: Some(Document {
                parser: None,
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: None,
                },
                meta: DocumentMeta {
                    manual_section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                diagnostics: Vec::new(),
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "options-1".to_owned().into(),
                    title: "OPTIONS".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![Block::DefinitionList {
                        items: vec![DefinitionItem {
                            inline_term: false,
                            identity: Some(DefinitionIdentity {
                                id: "option-acls".to_owned().into(),
                                role: DefinitionRole::Option,
                                case: DefinitionCase::Sensitive,
                                names: vec!["--acls".to_owned()],
                            }),
                            terms: vec![vec![
                                Inline::Anchor {
                                    id: "option-acls".to_owned().into(),
                                },
                                Inline::Code {
                                    value: "--acls".to_owned(),
                                },
                            ]],
                            description: vec![Block::Paragraph {
                                children: vec![
                                    Inline::Text {
                                        value: "Preserve ".to_owned(),
                                    },
                                    Inline::Strong {
                                        children: vec![Inline::Text {
                                            value: "access control".to_owned(),
                                        }],
                                    },
                                    Inline::Text {
                                        value: " lists".to_owned(),
                                    },
                                ],
                                layout: LayoutHint::default(),
                                source: None,
                            }],
                            spacing_before_lines: None,
                        }],
                        compact: true,
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    children: Vec::new(),
                    source: None,
                }],
            }),
            tldr: None,
        }
    }

    fn request(pattern: &str) -> SearchQuery {
        SearchQuery {
            pattern: pattern.to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Insensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 1,
            limit: 100,
            offset: 0,
        }
    }

    #[test]
    fn visible_search_maps_inline_formatting_to_markdown_and_option_nodes() {
        let result = search_query(&query(), &request("access control")).expect("search");

        assert_eq!(result.total, 1);
        assert_eq!(result.matches[0].outline.node.path(), "1/e1");
        assert_eq!(
            result.matches[0].occurrences[0].matched_text,
            "access control"
        );
        assert_eq!(result.matches[0].occurrences[0].line_ranges.len(), 1);
        assert!(result.matches[0].occurrences[0].markdown.start_line > 1);
        assert!(result.matches[0].preview.contains("**access control**"));
        assert!(!result.matches[0].preview.contains("<a id="));
        assert!(!result.matches[0].context.is_empty());
    }

    #[test]
    fn searches_contiguous_text_across_an_unsafe_style_boundary() {
        let mut query = query();
        query.document.as_mut().expect("fixture document").sections[0]
            .blocks
            .push(Block::Paragraph {
                children: vec![
                    Inline::Text {
                        value: "disabled with --".to_owned(),
                    },
                    Inline::Strong {
                        children: vec![Inline::Text {
                            value: "no-".to_owned(),
                        }],
                    },
                    Inline::Text {
                        value: "option".to_owned(),
                    },
                ],
                layout: LayoutHint::default(),
                source: None,
            });

        let visible = search_query(&query, &request("no-option")).expect("visible search");
        assert_eq!(visible.total, 1);
        assert_eq!(visible.matches[0].occurrences[0].matched_text, "no-option");
        assert!(visible.matches[0].preview.contains("--no-option"));
        assert!(!visible.matches[0].preview.contains("**no-**"));

        let markdown = search_query(
            &query,
            &SearchQuery {
                scope: SearchScope::Markdown,
                ..request("no-option")
            },
        )
        .expect("Markdown search");
        assert_eq!(markdown.total, 1);
        assert_eq!(markdown.matches[0].occurrences[0].matched_text, "no-option");
    }

    #[test]
    fn source_map_stripping_accepts_only_complete_empty_anchors() {
        assert_eq!(
            display_markdown_line("before<a id=\"node\"></a>after"),
            "beforeafter"
        );
        assert_eq!(
            display_markdown_line("before<a id=\"node\">payload</a>after"),
            "before<a id=\"node\">payload</a>after"
        );
        assert_eq!(
            display_markdown_line("before<a id=\"node\"after"),
            "before<a id=\"node\"after"
        );
    }

    #[test]
    fn visible_regex_anchors_apply_to_rendered_lines_not_the_whole_document() {
        for pattern in [r"^--acls", r"lists$"] {
            let mut request = request(pattern);
            request.syntax = SearchSyntax::Regex;
            request.case = SearchCase::Sensitive;

            let result = search_query(&query(), &request).expect("search");

            assert_eq!(result.total, 1, "pattern {pattern:?}");
            assert_eq!(result.matches[0].outline.node.path(), "1/e1");
        }
    }

    #[test]
    fn visible_search_maps_padded_code_span_content_not_its_delimiters() {
        for value in ["`x", "x`", " x", "x ", "`x`"] {
            let mut query = query();
            let Block::DefinitionList { items, .. } =
                &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
            else {
                panic!("fixture contains a definition list");
            };
            items[0].description = vec![Block::Paragraph {
                children: vec![Inline::Code {
                    value: value.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }];
            let markdown = render_addressable_markdown(&query).text;

            let result = search_query(&query, &request(value)).expect("search");
            let occurrence = &result.matches[0].occurrences[0];
            let start = usize::try_from(occurrence.markdown.start_byte).expect("small fixture");
            let end = usize::try_from(occurrence.markdown.end_byte).expect("small fixture");

            assert_eq!(&markdown[start..end], value, "code value {value:?}");
            assert_eq!(occurrence.line_ranges.len(), 1, "code value {value:?}");
            let line = &occurrence.line_ranges[0];
            let line_start = usize::try_from(line.start_byte).expect("small fixture");
            let line_end = usize::try_from(line.end_byte).expect("small fixture");
            assert_eq!(
                &result.matches[0].preview[line_start..line_end],
                value,
                "code value {value:?}"
            );
        }
    }

    #[test]
    fn visible_search_maps_an_explicit_line_break_to_its_markdown_byte() {
        let mut query = query();
        let Block::DefinitionList { items, .. } =
            &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
        else {
            panic!("fixture contains a definition list");
        };
        items[0].description = vec![Block::Paragraph {
            children: vec![
                Inline::Text {
                    value: "alpha".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "beta".to_owned(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let markdown = render_addressable_markdown(&query).text;

        let result = search_query(&query, &request("alpha\n")).expect("search");
        let occurrence = &result.matches[0].occurrences[0];
        let start = usize::try_from(occurrence.markdown.start_byte).expect("small fixture");
        let end = usize::try_from(occurrence.markdown.end_byte).expect("small fixture");

        assert_eq!(&markdown[start..end], "alpha  \n");
    }

    #[test]
    fn same_line_occurrences_form_one_paginated_search_result() {
        let mut query = query();
        let Block::DefinitionList { items, .. } =
            &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
        else {
            panic!("fixture contains a definition list");
        };
        items[0].description = vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: "needle, then another needle on one line".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        let mut request = request("needle");
        request.limit = 1;
        let result = search_query(&query, &request).expect("search");

        assert_eq!(result.total, 1);
        assert_eq!(result.returned, 1);
        assert_eq!(result.matches[0].occurrences.len(), 2);
        assert_eq!(
            result.matches[0].occurrences[0].markdown.start_line,
            result.matches[0].occurrences[1].markdown.start_line
        );
        assert!(!result.truncated);
    }

    #[test]
    fn one_repetitive_line_has_bounded_occurrence_details() {
        let mut query = query();
        let Block::DefinitionList { items, .. } =
            &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
        else {
            panic!("fixture contains a definition list");
        };
        let occurrence_count = MAX_OCCURRENCES_PER_MATCH + 7;
        items[0].description = vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: vec!["needle"; occurrence_count].join(" "),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        let result = search_query(&query, &request("needle")).expect("search");

        assert_eq!(result.total, 1);
        assert_eq!(
            result.matches[0].occurrence_count,
            u32::try_from(occurrence_count).expect("small fixture")
        );
        assert_eq!(
            result.matches[0].occurrences.len(),
            MAX_OCCURRENCES_PER_MATCH
        );
        assert!(result.matches[0].occurrences_truncated);
    }

    #[test]
    fn semantic_entry_ownership_ends_before_a_following_section_paragraph() {
        let mut query = query();
        query.document.as_mut().expect("manual").sections[0]
            .blocks
            .push(Block::Paragraph {
                children: vec![Inline::Text {
                    value: "General section tail".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            });

        let result = search_query(&query, &request("section tail")).expect("search");
        assert!(matches!(
            &result.matches[0].outline.node,
            mant_protocol::OutlineNodeReference::DocumentSection { path, .. } if path == "1"
        ));
    }

    #[test]
    fn root_content_search_resolves_to_an_addressable_document_root() {
        let mut query = query();
        let document = query.document.as_mut().expect("document");
        document.source.format = SourceFormat::Markdown;
        document.blocks.push(Block::Paragraph {
            children: vec![Inline::Text {
                value: "Read the preface needle first.".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        });

        let result = search_query(&query, &request("preface needle")).expect("root search");

        assert_eq!(result.total, 1);
        assert!(matches!(
            &result.matches[0].outline.node,
            mant_protocol::OutlineNodeReference::DocumentRoot { path, id, .. }
                if path == "root" && id == "document-overview"
        ));
        assert!(result.matches[0].outline.ancestors.is_empty());
        assert!(result.matches[0].preview.contains("preface needle"));
    }

    #[test]
    fn embedded_tldr_and_markdown_body_keep_distinct_search_owners() {
        let query = crate::query_markdown_text(
            "\
<!-- mant:tldr:start -->
# demo

> Quick needle.

- Run:

`demo quick-command`
<!-- mant:tldr:end -->

# Demo

Read the overview needle.

## Synopsis

Manual needle.
",
            Some("demo.md".to_owned()),
        )
        .expect("Markdown query");

        let quick = search_query(&query, &request("quick needle")).expect("tldr search");
        assert!(matches!(
            &quick.matches[0].outline.node,
            mant_protocol::OutlineNodeReference::Tldr { path, id, .. }
                if path == "0" && id == "tldr"
        ));

        let overview = search_query(&query, &request("overview needle")).expect("root search");
        assert!(matches!(
            &overview.matches[0].outline.node,
            mant_protocol::OutlineNodeReference::DocumentRoot { path, .. } if path == "root"
        ));

        let manual = search_query(&query, &request("manual needle")).expect("section search");
        assert!(matches!(
            &manual.matches[0].outline.node,
            mant_protocol::OutlineNodeReference::DocumentSection { path, id, .. }
                if path == "1" && id == "synopsis"
        ));
    }

    #[test]
    fn regex_case_and_pagination_are_reported_without_losing_global_ordinals() {
        let mut request = request("ACLS|control");
        request.syntax = SearchSyntax::Regex;
        request.case = SearchCase::Insensitive;
        request.limit = 1;
        request.offset = 1;
        let result = search_query(&query(), &request).expect("search");

        assert_eq!(result.total, 2);
        assert_eq!(result.returned, 1);
        assert_eq!(result.matches[0].ordinal, 2);
        assert!(!result.truncated);
    }

    #[test]
    fn regexes_that_match_empty_text_are_rejected() {
        for pattern in ["$", r"\b", r"\B", "a*"] {
            let mut request = request(pattern);
            request.syntax = SearchSyntax::Regex;
            let error = search_query(&query(), &request).expect_err("empty regex match");
            assert!(
                error.to_string().contains("must not match empty text"),
                "pattern {pattern:?}: {error}"
            );
        }
    }

    #[test]
    fn search_results_never_cross_addressable_owner_boundaries() {
        let mut query = query();
        query
            .document
            .as_mut()
            .expect("document")
            .sections
            .push(Section {
                id: "next".into(),
                title: "NEXT".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "Following owner".to_owned(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            });
        let mut request = request(r"lists(?s:.*?)NEXT");
        request.syntax = SearchSyntax::Regex;
        request.scope = SearchScope::Markdown;
        request.case = SearchCase::Sensitive;

        let result = search_query(&query, &request).expect("bounded owner search");

        assert_eq!(result.total, 0);
    }

    #[test]
    fn exclusive_newline_end_does_not_mark_the_following_context_line() {
        let mut query = query();
        query.document.as_mut().expect("document").sections[0]
            .blocks
            .push(Block::Paragraph {
                children: vec![
                    Inline::Text {
                        value: "alpha".to_owned(),
                    },
                    Inline::LineBreak,
                    Inline::Text {
                        value: "beta".to_owned(),
                    },
                ],
                layout: LayoutHint::default(),
                source: None,
            });
        let mut request = request("alpha  \n");
        request.scope = SearchScope::Markdown;
        request.case = SearchCase::Sensitive;
        let result = search_query(&query, &request).expect("newline search");
        let hit = &result.matches[0];
        let occurrence = &hit.occurrences[0];

        assert_eq!(occurrence.line_ranges.len(), 1);
        let following = hit
            .context
            .iter()
            .find(|line| line.line == occurrence.markdown.end_line)
            .expect("following context line");
        assert!(!following.matched);
    }

    #[test]
    fn multibyte_match_ends_remain_valid_coordinate_boundaries() {
        let mut query = query();
        query.document.as_mut().expect("document").sections[0]
            .blocks
            .push(Block::Paragraph {
                children: vec![Inline::Text {
                    value: "café — 日本".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            });
        let mut request = request("—");
        request.case = SearchCase::Sensitive;

        let result = search_query(&query, &request).expect("Unicode search");
        let occurrence = &result.matches[0].occurrences[0];

        assert_eq!(occurrence.matched_text, "—");
        assert_eq!(
            occurrence.markdown.end_column,
            occurrence.markdown.start_column + 1
        );
    }

    #[test]
    fn byte_mode_regexes_are_rejected_before_matching_unicode_text() {
        let mut request = request("(?-u:.)");
        request.syntax = SearchSyntax::Regex;
        let error = search_query(&query(), &request).expect_err("byte-oriented regex");

        assert!(error.to_string().contains("UTF-8 character boundaries"));
    }

    #[test]
    fn regex_syntax_errors_retain_their_actual_cause() {
        for (pattern, expected) in [
            ("(", "unclosed group"),
            ("a{2,1}", "invalid repetition count range"),
            ("[z-a]", "invalid character class range"),
        ] {
            let mut request = request(pattern);
            request.syntax = SearchSyntax::Regex;
            let error = validate_search_query(&request).expect_err("invalid regex");
            let message = error.to_string();
            assert!(message.contains(expected), "{pattern}: {message}");
            assert!(!message.contains("Unicode mode cannot be disabled"));
        }
    }

    #[test]
    fn search_pattern_limit_counts_unicode_scalars() {
        let valid = request(&"界".repeat(MAX_SEARCH_PATTERN_CHARS));
        assert_eq!(validate_search_query(&valid), Ok(()));

        let request = request(&"界".repeat(MAX_SEARCH_PATTERN_CHARS + 1));
        assert_eq!(
            validate_search_query(&request),
            Err(SearchError::PatternTooLong)
        );
    }
}
