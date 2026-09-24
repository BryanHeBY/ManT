//! One display item per exact search occurrence; no line-group surrogate identity.

use mant_ir::EntryKind;
use mant_protocol::{
    OutlineNodeReference, OutlineTrail, QuerySearch, ScopedSearchDocument, SearchLocation,
    SearchMatch, SearchQuery,
};

use crate::sanitize_terminal_text;

mod markdown;
pub(super) use markdown::render_scoped_search_markdown;
pub use markdown::render_search_markdown;

/// Semantic roles in the grep-like search presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchTextRole {
    /// Ordinary prose and punctuation.
    Plain,
    /// The logical document label.
    Document,
    /// A typed coordinate or result count.
    Coordinate,
    /// A stable semantic-node path.
    Path,
    /// A document, section, or tldr node title.
    Heading,
    /// A semantic entry title.
    Definition(EntryKind),
    /// Exact text matched by the query.
    Match,
    /// Secondary guides and context markers.
    Muted,
}

/// Render one paginated document result as terminal-safe plain text.
#[must_use]
pub fn render_search_text(search: &QuerySearch) -> String {
    render_search_text_with(search, |_, value| value.to_owned())
}

/// Decorate a plain search report without changing exact match boundaries.
#[must_use]
pub fn render_search_text_with(
    search: &QuerySearch,
    decorate: impl FnMut(SearchTextRole, &str) -> String,
) -> String {
    let label = document_label(search);
    let mut output = SearchTextRenderer::new(decorate);
    if search.total == 0 {
        output.plain("No matches for \"");
        output.push(SearchTextRole::Match, &search.query.pattern);
        output.plain("\" in ");
        output.push(SearchTextRole::Document, &label);
        output.plain(".");
    } else if search.matches.is_empty() {
        output.plain("No occurrences returned at offset ");
        output.push(SearchTextRole::Coordinate, &search.offset.to_string());
        output.plain(" for \"");
        output.push(SearchTextRole::Match, &search.query.pattern);
        output.plain("\" in ");
        output.push(SearchTextRole::Document, &label);
        output.plain(" (");
        output.push(SearchTextRole::Coordinate, &search.total.to_string());
        output.plain(" total).");
    } else {
        render_matches(&mut output, &label, &search.matches);
    }
    if let Some(next_offset) = search.next_offset {
        output.paragraph_break();
        output.push(SearchTextRole::Coordinate, &search.total.to_string());
        output.plain(" total occurrences; next occurrence offset ");
        output.push(SearchTextRole::Coordinate, &next_offset.to_string());
        output.plain(".");
    }
    append_coverage(
        &mut output,
        search.semantics_complete,
        search.coverage_details_omitted,
    );
    output.finish()
}

/// Render one globally paginated scope group, without invented local counts.
pub(super) fn render_scoped_search_text_with(
    found: &ScopedSearchDocument,
    query: &SearchQuery,
    decorate: impl FnMut(SearchTextRole, &str) -> String,
) -> String {
    let label = scoped_label(found);
    let mut output = SearchTextRenderer::new(decorate);
    if found.matches.is_empty() {
        output.plain("No retained occurrences for \"");
        output.push(SearchTextRole::Match, &query.pattern);
        output.plain("\" in ");
        output.push(SearchTextRole::Document, &label);
        output.plain(".");
    } else {
        render_matches(&mut output, &label, &found.matches);
    }
    output.finish()
}

fn render_matches<F>(output: &mut SearchTextRenderer<F>, label: &str, matches: &[SearchMatch])
where
    F: FnMut(SearchTextRole, &str) -> String,
{
    for (index, found) in matches.iter().enumerate() {
        if index > 0 {
            output.paragraph_break();
        }
        output.push(SearchTextRole::Document, label);
        output.plain("  ");
        render_outline_trail(output, &found.outline);
        output.line();
        output.plain("  #");
        output.push(SearchTextRole::Coordinate, &found.ordinal.to_string());
        output.plain("  ");
        output.push(SearchTextRole::Coordinate, &coordinate(found));
        output.line();
        output.plain("  Match: ");
        output.push(SearchTextRole::Match, &found.matched_text);
        // The exact match above is the highlighted occurrence.  A preview
        // containing the same bytes is a duplicate, not a second hit.
        if !found.preview.is_empty() && !found.preview.contains(&found.matched_text) {
            output.line();
            output.plain("  Preview: ");
            output.plain(&found.preview);
        }
        for context in &found.context {
            if context.matched && context.text.contains(&found.matched_text) {
                continue;
            }
            output.line();
            output.plain(if context.matched { "  > " } else { "    " });
            output.push(SearchTextRole::Coordinate, &context.line.to_string());
            output.plain(" ");
            output.push(SearchTextRole::Muted, &context.text);
        }
    }
}

fn append_coverage<F>(output: &mut SearchTextRenderer<F>, complete: bool, omitted: u32)
where
    F: FnMut(SearchTextRole, &str) -> String,
{
    if complete {
        return;
    }
    output.paragraph_break();
    output.push(SearchTextRole::Muted, "Semantic coverage incomplete");
    if omitted != 0 {
        output.plain("; ");
        output.push(SearchTextRole::Coordinate, &omitted.to_string());
        output.push(SearchTextRole::Muted, " details omitted");
    }
    output.plain(".");
}

pub(super) fn coordinate(found: &SearchMatch) -> String {
    match found.location {
        SearchLocation::VisibleFlow {
            unit,
            start_scalar,
            end_scalar,
        } => format!("visible-flow/u{}:{start_scalar}..{end_scalar}", unit.get()),
        SearchLocation::VisibleFixed {
            unit,
            start_scalar,
            end_scalar,
        } => format!("visible-fixed/u{}:{start_scalar}..{end_scalar}", unit.get()),
        SearchLocation::MarkdownArtifact {
            start_line,
            start_column,
            end_line,
            end_column,
            ..
        } => format!("{start_line}:{start_column}..{end_line}:{end_column}"),
    }
}

struct SearchTextRenderer<F> {
    rendered: String,
    decorate: F,
}

impl<F> SearchTextRenderer<F>
where
    F: FnMut(SearchTextRole, &str) -> String,
{
    fn new(decorate: F) -> Self {
        Self {
            rendered: String::new(),
            decorate,
        }
    }

    fn plain(&mut self, value: &str) {
        self.push(SearchTextRole::Plain, value);
    }

    fn push(&mut self, role: SearchTextRole, value: &str) {
        self.rendered
            .push_str(&(self.decorate)(role, &sanitize_terminal_text(value)));
    }

    fn line(&mut self) {
        self.rendered.push('\n');
    }

    fn paragraph_break(&mut self) {
        if !self.rendered.is_empty() {
            self.rendered.push_str("\n\n");
        }
    }

    fn finish(self) -> String {
        self.rendered.trim_end().to_owned()
    }
}

fn render_outline_trail<F>(output: &mut SearchTextRenderer<F>, trail: &OutlineTrail)
where
    F: FnMut(SearchTextRole, &str) -> String,
{
    output.push(SearchTextRole::Muted, "Outline ");
    output.push(SearchTextRole::Path, trail.path());
    output.push(SearchTextRole::Muted, ": ");
    for (index, ancestor) in trail.ancestors.iter().enumerate() {
        if index > 0 {
            output.push(SearchTextRole::Muted, " > ");
        }
        output.push(SearchTextRole::Heading, &ancestor.title);
    }
    if !trail.ancestors.is_empty() {
        output.push(SearchTextRole::Muted, " > ");
    }
    output.push(search_node_role(&trail.node), trail.title());
}

const fn search_node_role(node: &OutlineNodeReference) -> SearchTextRole {
    match node {
        OutlineNodeReference::DocumentEntry { entry_kind, .. } => {
            SearchTextRole::Definition(*entry_kind)
        }
        OutlineNodeReference::Tldr { .. }
        | OutlineNodeReference::DocumentRoot { .. }
        | OutlineNodeReference::DocumentSection { .. } => SearchTextRole::Heading,
    }
}

fn document_label(search: &QuerySearch) -> String {
    search
        .meta
        .as_ref()
        .and_then(|meta| meta.manual_section.as_deref())
        .map_or_else(
            || search.label.clone(),
            |section| format!("{}({section})", search.label),
        )
}

pub(super) fn scoped_label(found: &ScopedSearchDocument) -> String {
    match &found.address {
        mant_protocol::DocumentAddress::Manual {
            name,
            manual_section,
        } => format!("{name}({manual_section})"),
        mant_protocol::DocumentAddress::Markdown { path, .. } => path.clone(),
    }
}

#[cfg(test)]
mod tests;
