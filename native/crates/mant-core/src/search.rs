//! Searches deterministic Markdown while retaining addressable manual nodes.
//!
//! Section and semantic-entry anchors emitted by the Markdown renderer form
//! an internal source map. pulldown-cmark supplies a visible-text projection
//! whose byte ranges map back into that exact Markdown document.

use std::{error::Error, fmt, ops::Range};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use mant_ast::{
    Block, DefinitionItem, MarkdownSchema, QueryBundle, QuerySearch, SearchCase, SearchContextLine,
    SearchMarkdownRange, SearchMatch, SearchNode, SearchQuery, SearchRender, SearchRenderFormat,
    SearchRenderScope, SearchSchema, SearchScope, SearchSectionReference, SearchSyntax, Section,
    SourceSpan,
};
use pulldown_cmark::{Event, Parser, TagEnd};

use crate::output::{html_anchor, render_markdown};

const MAX_PATTERN_BYTES: usize = 4096;
const MAX_CONTEXT_LINES: u16 = 100;
const MAX_SEARCH_LIMIT: u32 = 10_000;

/// Invalid search input or matcher construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    EmptyPattern,
    PatternTooLong,
    InvalidLimit,
    ContextTooLarge,
    InvalidPattern(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => formatter.write_str("search pattern must not be empty"),
            Self::PatternTooLong => write!(
                formatter,
                "search pattern exceeds the {MAX_PATTERN_BYTES}-byte limit"
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
    query: &QueryBundle,
    request: &SearchQuery,
) -> Result<QuerySearch, SearchError> {
    validate_request(request)?;
    let markdown = render_markdown(query);
    let lines = LineIndex::new(&markdown);
    let owners = OwnerIndex::new(query, &markdown);
    let searchable = SearchableText::new(&markdown, request.scope);
    let matcher = build_matcher(request)?;
    let mut raw_matches = Vec::new();

    matcher
        .find_iter(searchable.text.as_bytes(), |found| {
            let markdown_start = searchable.markdown_start(found.start());
            let markdown_end = searchable.markdown_end(found.end());
            if let Some(owner) = owners.owner(markdown_start) {
                raw_matches.push(RawMatch {
                    searchable: found.start()..found.end(),
                    markdown: markdown_start..markdown_end,
                    owner: owner.clone(),
                });
            }
            true
        })
        .map_err(|error| SearchError::InvalidPattern(error.to_string()))?;

    let total = u32::try_from(raw_matches.len()).unwrap_or(u32::MAX);
    let offset = usize::try_from(request.offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(request.limit).unwrap_or(usize::MAX);
    let selected = raw_matches
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(index, found)| {
            build_match(
                index,
                found,
                &searchable.text,
                &markdown,
                &lines,
                request.context_lines,
            )
        })
        .collect::<Vec<_>>();
    let returned = u32::try_from(selected.len()).unwrap_or(u32::MAX);
    let consumed = request.offset.saturating_add(returned);
    let truncated = consumed < total;

    Ok(QuerySearch {
        schema: SearchSchema::V1,
        topic: query.topic.clone(),
        manual_section: resolved_manual_section(query),
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
    if request.pattern.len() > MAX_PATTERN_BYTES {
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
    let mut builder = RegexMatcherBuilder::new();
    builder
        .fixed_strings(request.syntax == SearchSyntax::Literal)
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
        return Err(SearchError::InvalidPattern(
            "pattern must not match empty text".to_owned(),
        ));
    }
    Ok(matcher)
}

#[derive(Clone)]
struct RawMatch {
    searchable: Range<usize>,
    markdown: Range<usize>,
    owner: Owner,
}

fn build_match(
    index: usize,
    found: &RawMatch,
    searchable: &str,
    markdown: &str,
    lines: &LineIndex,
    context_lines: u16,
) -> SearchMatch {
    let start = lines.position(markdown, found.markdown.start);
    let end = lines.position(markdown, found.markdown.end);
    let preview = display_markdown_line(lines.line(markdown, start.line_index));
    let context_start = start.line_index.saturating_sub(usize::from(context_lines));
    let context_end = end
        .line_index
        .saturating_add(usize::from(context_lines))
        .min(lines.count().saturating_sub(1));
    let context = if context_lines == 0 {
        Vec::new()
    } else {
        (context_start..=context_end)
            .map(|line_index| SearchContextLine {
                line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
                text: display_markdown_line(lines.line(markdown, line_index)),
                matched: (start.line_index..=end.line_index).contains(&line_index),
            })
            .collect()
    };

    SearchMatch {
        ordinal: u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX),
        node: found.owner.node.clone(),
        section: found.owner.section.clone(),
        matched_text: searchable[found.searchable.clone()].to_owned(),
        markdown: SearchMarkdownRange {
            start_byte: u64::try_from(found.markdown.start).unwrap_or(u64::MAX),
            end_byte: u64::try_from(found.markdown.end).unwrap_or(u64::MAX),
            start_line: u32::try_from(start.line_index.saturating_add(1)).unwrap_or(u32::MAX),
            start_column: u32::try_from(start.column).unwrap_or(u32::MAX),
            end_line: u32::try_from(end.line_index.saturating_add(1)).unwrap_or(u32::MAX),
            end_column: u32::try_from(end.column).unwrap_or(u32::MAX),
        },
        source: found.owner.source,
        preview,
        context,
    }
}

/// Hide `ManT`'s zero-width source-map anchors from human-facing snippets.
fn display_markdown_line(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut remaining = line.trim_end();
    while let Some(start) = remaining.find("<a id=\"") {
        output.push_str(&remaining[..start]);
        let anchor = &remaining[start..];
        let Some(end) = anchor.find("</a>") else {
            output.push_str(anchor);
            return output;
        };
        remaining = &anchor[end + "</a>".len()..];
    }
    output.push_str(remaining);
    output
}

fn resolved_manual_section(query: &QueryBundle) -> Option<String> {
    query
        .manual
        .as_ref()
        .and_then(|manual| manual.meta.section.clone())
        .or_else(|| query.section.clone())
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
                Event::Text(value)
                | Event::Code(value)
                | Event::InlineMath(value)
                | Event::DisplayMath(value) => visible.push_aligned(&value, source),
                Event::SoftBreak | Event::HardBreak => visible.push_break(source.start),
                Event::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::Item
                    | TagEnd::CodeBlock
                    | TagEnd::TableRow,
                )
                | Event::Rule => visible.push_break(source.end),
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
            if segment.visible.len() == segment.markdown.len() {
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
            if segment.visible.len() == segment.markdown.len() {
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

    fn push_aligned(&mut self, value: &str, source: Range<usize>) {
        let mut markdown_cursor = source.start.min(self.markdown.len());
        let markdown_end = source.end.min(self.markdown.len());
        for character in value.chars() {
            let found = self.markdown[markdown_cursor..markdown_end]
                .find(character)
                .map_or(markdown_cursor, |relative| markdown_cursor + relative);
            let visible_start = self.text.len();
            self.text.push(character);
            let visible_end = self.text.len();
            let source_end = found.saturating_add(character.len_utf8()).min(markdown_end);
            self.push_segment(OffsetSegment {
                visible: visible_start..visible_end,
                markdown: found..source_end,
            });
            markdown_cursor = source_end;
        }
    }

    fn push_break(&mut self, markdown_offset: usize) {
        if self.text.ends_with('\n') || self.text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push('\n');
        self.push_segment(OffsetSegment {
            visible: start..self.text.len(),
            markdown: markdown_offset..markdown_offset,
        });
    }

    fn push_segment(&mut self, segment: OffsetSegment) {
        if let Some(previous) = self.segments.last_mut() {
            let contiguous = previous.visible.end == segment.visible.start
                && previous.markdown.end == segment.markdown.start
                && previous.visible.len() == previous.markdown.len()
                && segment.visible.len() == segment.markdown.len();
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

#[derive(Clone)]
struct Owner {
    start: usize,
    end: usize,
    node: SearchNode,
    section: Option<SearchSectionReference>,
    source: Option<SourceSpan>,
}

struct OwnerIndex {
    sections: Vec<Owner>,
    entries: Vec<Owner>,
    entry_prefix_max_end: Vec<usize>,
    tldr: Option<Owner>,
}

impl OwnerIndex {
    fn new(query: &QueryBundle, markdown: &str) -> Self {
        let mut sections = Vec::new();
        let mut entries = Vec::new();
        if let Some(manual) = &query.manual {
            collect_section_owners(&manual.sections, &[], markdown, &mut sections, &mut entries);
            sections.sort_by_key(|owner| owner.start);
            for index in 0..sections.len() {
                sections[index].end = sections
                    .get(index + 1)
                    .map_or(markdown.len(), |next| next.start);
            }
            for entry in &mut entries {
                if let Some(section_end) = entry.section.as_ref().and_then(|reference| {
                    sections
                        .iter()
                        .find(|section| {
                            section.section.as_ref().map(|value| value.id.as_str())
                                == Some(reference.id.as_str())
                        })
                        .map(|section| section.end)
                }) {
                    entry.end = entry.end.min(section_end);
                }
            }
            entries.sort_by_key(|owner| owner.start);
        }
        let manual_start = sections.first().map_or(markdown.len(), |owner| owner.start);
        let tldr = query.tldr.as_ref().and_then(|_| {
            markdown.find("## TLDR").map(|start| Owner {
                start,
                end: manual_start,
                node: SearchNode::Tldr {
                    path: "0".to_owned(),
                    id: "tldr".to_owned(),
                    title: "TLDR QUICK REFERENCE".to_owned(),
                },
                section: None,
                source: None,
            })
        });
        let mut maximum_end = 0;
        let entry_prefix_max_end = entries
            .iter()
            .map(|entry| {
                maximum_end = maximum_end.max(entry.end);
                maximum_end
            })
            .collect();
        Self {
            sections,
            entries,
            entry_prefix_max_end,
            tldr,
        }
    }

    fn owner(&self, offset: usize) -> Option<&Owner> {
        if let Some(entry) = self.entry_owner(offset) {
            return Some(entry);
        }
        let section_index = self.sections.partition_point(|owner| owner.start <= offset);
        if let Some(section) = section_index
            .checked_sub(1)
            .and_then(|index| self.sections.get(index))
            .filter(|owner| offset < owner.end)
        {
            return Some(section);
        }
        self.tldr
            .as_ref()
            .filter(|owner| owner.start <= offset && offset < owner.end)
    }

    fn entry_owner(&self, offset: usize) -> Option<&Owner> {
        let mut index = self.entries.partition_point(|owner| owner.start <= offset);
        while let Some(candidate_index) = index.checked_sub(1) {
            let candidate = &self.entries[candidate_index];
            if offset < candidate.end {
                return Some(candidate);
            }
            if candidate_index == 0 || self.entry_prefix_max_end[candidate_index - 1] <= offset {
                break;
            }
            index = candidate_index;
        }
        None
    }
}

fn collect_section_owners(
    sections: &[Section],
    parent: &[usize],
    markdown: &str,
    section_owners: &mut Vec<Owner>,
    entry_owners: &mut Vec<Owner>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent.to_vec();
        coordinates.push(index + 1);
        let path = format_path(&coordinates);
        let anchor = html_anchor(&section.id);
        let Some(start) = markdown.find(&anchor) else {
            continue;
        };
        let section_reference = SearchSectionReference {
            path: path.clone(),
            id: section.id.clone(),
            title: section.title.clone(),
        };
        section_owners.push(Owner {
            start,
            end: markdown.len(),
            node: SearchNode::ManualSection {
                path: path.clone(),
                id: section.id.clone(),
                title: section.title.clone(),
            },
            section: Some(section_reference.clone()),
            source: section.source,
        });

        let mut entries = Vec::new();
        collect_entries(&section.blocks, &mut entries);
        for (entry_index, (entry, source)) in entries.into_iter().enumerate() {
            let Some(identity) = &entry.identity else {
                continue;
            };
            let entry_anchor = html_anchor(&identity.id);
            let Some(entry_start) = markdown[start..]
                .find(&entry_anchor)
                .map(|relative| start + relative)
            else {
                continue;
            };
            entry_owners.push(Owner {
                start: entry_start,
                end: definition_item_end(markdown, entry_start),
                node: SearchNode::ManualEntry {
                    path: format!("{path}/o{}", entry_index + 1),
                    id: identity.id.clone(),
                    title: identity.names.join(", "),
                    role: identity.role,
                    names: identity.names.clone(),
                },
                section: Some(section_reference.clone()),
                source,
            });
        }
        collect_section_owners(
            &section.children,
            &coordinates,
            markdown,
            section_owners,
            entry_owners,
        );
    }
}

fn collect_entries<'a>(
    blocks: &'a [Block],
    output: &mut Vec<(&'a DefinitionItem, Option<SourceSpan>)>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_entries(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, source, .. } => {
                for item in items {
                    if item.identity.is_some() {
                        output.push((item, *source));
                    }
                    collect_entries(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_entries(&cell.blocks, output);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn definition_item_end(markdown: &str, anchor_start: usize) -> usize {
    let line_start = markdown[..anchor_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let prefix = &markdown[line_start..anchor_start];
    if prefix.is_empty() {
        return markdown.len();
    }
    let content_indent = prefix.chars().count();
    let mut cursor = markdown[anchor_start..]
        .find('\n')
        .map_or(markdown.len(), |relative| anchor_start + relative + 1);
    let mut after_blank = false;

    while cursor < markdown.len() {
        let end = markdown[cursor..]
            .find('\n')
            .map_or(markdown.len(), |relative| cursor + relative);
        let line = &markdown[cursor..end];
        if line.starts_with(prefix) {
            return cursor;
        }
        if line.trim().is_empty() {
            after_blank = true;
        } else {
            let indent = line
                .chars()
                .take_while(|character| *character == ' ')
                .count();
            if after_blank && indent < content_indent {
                return cursor;
            }
            after_blank = false;
        }
        cursor = end.saturating_add(1);
    }
    markdown.len()
}

fn format_path(coordinates: &[usize]) -> String {
    coordinates
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use mant_ast::{
        Block, DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta, DocumentSchema,
        DocumentSource, Inline, LayoutHint, MantDocument, Producer, QueryBundle, QuerySchema,
        SearchCase, SearchQuery, SearchScope, SearchSyntax, Section, SourceFormat,
    };

    use super::search_query;

    fn query() -> QueryBundle {
        QueryBundle {
            schema: QuerySchema::V2,
            topic: "demo".to_owned(),
            section: Some("1".to_owned()),
            manual: Some(MantDocument {
                schema: DocumentSchema::V2,
                producer: Producer {
                    name: "test".to_owned(),
                    version: "1".to_owned(),
                    engine: None,
                },
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: None,
                    renderer: None,
                },
                meta: DocumentMeta {
                    section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                diagnostics: Vec::new(),
                sections: vec![Section {
                    id: "options-1".to_owned(),
                    title: "OPTIONS".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![Block::DefinitionList {
                        items: vec![DefinitionItem {
                            identity: Some(DefinitionIdentity {
                                id: "option-acls".to_owned(),
                                role: DefinitionRole::Option,
                                names: vec!["--acls".to_owned()],
                            }),
                            terms: vec![vec![
                                Inline::Anchor {
                                    id: "option-acls".to_owned(),
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
        assert_eq!(result.matches[0].node.path(), "1/o1");
        assert_eq!(result.matches[0].matched_text, "access control");
        assert!(result.matches[0].markdown.start_line > 1);
        assert!(result.matches[0].preview.contains("**access control**"));
        assert!(!result.matches[0].preview.contains("<a id="));
        assert!(!result.matches[0].context.is_empty());
    }

    #[test]
    fn semantic_entry_ownership_ends_before_a_following_section_paragraph() {
        let mut query = query();
        query.manual.as_mut().expect("manual").sections[0]
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
            &result.matches[0].node,
            mant_ast::SearchNode::ManualSection { path, .. } if path == "1"
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
        let mut request = request("$");
        request.syntax = SearchSyntax::Regex;
        let error = search_query(&query(), &request).expect_err("empty regex match");
        assert!(error.to_string().contains("must not match empty text"));
    }
}
