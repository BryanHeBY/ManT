//! Extracts `ManT`'s optional document-owned tldr preface.
//!
//! Invisible HTML comments delimit the structural extension so GitHub and
//! other `CommonMark` renderers show only valid Markdown content. The opening
//! marker must be the first non-empty construct, and its contents use the
//! tldr-pages dialect. The returned document text masks the complete preface
//! while preserving byte offsets and line numbers for source diagnostics.

use std::{borrow::Cow, error::Error, fmt, ops::Range};

const OPENING_MARKER: &str = "<!-- mant:tldr:start -->";
const CLOSING_MARKER: &str = "<!-- mant:tldr:end -->";

#[derive(Debug)]
pub(super) struct MarkdownParts<'a> {
    pub(super) document: Cow<'a, str>,
    pub(super) tldr: Option<&'a str>,
}

/// Invalid structure in a document-owned tldr directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TldrDirectiveError {
    /// A leading opening marker has no matching closing marker.
    Unterminated,
}

impl fmt::Display for TldrDirectiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unterminated => {
                formatter.write_str(
                    "top-level <!-- mant:tldr:start --> marker is missing its <!-- mant:tldr:end --> marker",
                )
            }
        }
    }
}

impl Error for TldrDirectiveError {}

pub(super) fn split_markdown(source: &str) -> Result<MarkdownParts<'_>, TldrDirectiveError> {
    let lines = source_lines(source);
    let Some(opening_index) = lines
        .iter()
        .position(|line| !line_text(source, line).trim().is_empty())
    else {
        return Ok(MarkdownParts {
            document: Cow::Borrowed(source),
            tldr: None,
        });
    };
    let opening = &lines[opening_index];
    if line_text(source, opening).trim() != OPENING_MARKER {
        return Ok(MarkdownParts {
            document: Cow::Borrowed(source),
            tldr: None,
        });
    }

    let closing = lines
        .iter()
        .skip(opening_index + 1)
        .find(|line| line_text(source, line).trim() == CLOSING_MARKER)
        .ok_or(TldrDirectiveError::Unterminated)?;
    let tldr_start = opening.end;
    let tldr_end = closing.start;
    let masked_end = closing.end;

    Ok(MarkdownParts {
        document: Cow::Owned(mask_range(source, opening.start..masked_end)),
        tldr: Some(&source[tldr_start..tldr_end]),
    })
}

#[derive(Clone)]
struct SourceLine {
    start: usize,
    content_end: usize,
    end: usize,
}

fn source_lines(source: &str) -> Vec<SourceLine> {
    if source.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for segment in source.split_inclusive('\n') {
        let end = start + segment.len();
        let content_end = if segment.ends_with("\r\n") {
            end - 2
        } else if segment.ends_with('\n') {
            end - 1
        } else {
            end
        };
        lines.push(SourceLine {
            start,
            content_end,
            end,
        });
        start = end;
    }
    // split_inclusive yields every byte, including a final line without a
    // trailing newline, so no trailing-segment fixup is needed here.
    lines
}

fn line_text<'a>(source: &'a str, line: &SourceLine) -> &'a str {
    &source[line.start..line.content_end]
}

fn mask_range(source: &str, range: Range<usize>) -> String {
    let mut masked = String::with_capacity(source.len());
    masked.push_str(&source[..range.start]);
    for character in source[range.clone()].chars() {
        if matches!(character, '\n' | '\r') {
            masked.push(character);
        } else {
            masked.extend(std::iter::repeat_n(' ', character.len_utf8()));
        }
    }
    masked.push_str(&source[range.end..]);
    masked
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use pulldown_cmark::{Event, Parser};

    use super::{TldrDirectiveError, split_markdown};

    #[test]
    fn extracts_only_a_leading_directive_and_preserves_source_coordinates() {
        let source = "\n<!-- mant:tldr:start -->\n# demo\n\n- Run:\n\n`demo`\n<!-- mant:tldr:end -->\n\n# Demo\n\nBody.\n";
        let parts = split_markdown(source).expect("directive");

        assert_eq!(parts.tldr, Some("# demo\n\n- Run:\n\n`demo`\n"));
        assert_eq!(parts.document.len(), source.len());
        assert_eq!(
            parts.document.matches('\n').count(),
            source.matches('\n').count()
        );
        assert_eq!(parts.document.find("# Demo"), source.find("# Demo"));
    }

    #[test]
    fn leaves_later_directives_as_ordinary_markdown() {
        let source = "# Demo\n\n<!-- mant:tldr:start -->\n# late\n<!-- mant:tldr:end -->\n";
        let parts = split_markdown(source).expect("ordinary Markdown");

        assert!(parts.tldr.is_none());
        assert!(matches!(parts.document, Cow::Borrowed(_)));
    }

    #[test]
    fn reports_an_unterminated_leading_directive() {
        let error = split_markdown("<!-- mant:tldr:start -->\n# demo\n").expect_err("unterminated");
        assert_eq!(error, TldrDirectiveError::Unterminated);
    }

    #[test]
    fn does_not_accept_the_obsolete_fenced_container() {
        let source = ":::tldr\n# demo\n:::\n\n# Demo\n";
        let parts = split_markdown(source).expect("ordinary Markdown");

        assert!(parts.tldr.is_none());
        assert!(matches!(parts.document, Cow::Borrowed(_)));
    }

    #[test]
    fn boundary_comments_are_not_visible_commonmark_text() {
        let source = "<!-- mant:tldr:start -->\n# demo\n\n> Quick reference.\n<!-- mant:tldr:end -->\n\n# Demo\n";
        let visible = Parser::new(source)
            .filter_map(|event| match event {
                Event::Text(text) => Some(text.into_string()),
                _ => None,
            })
            .collect::<String>();

        assert!(!visible.contains("mant:tldr"));
        assert!(visible.contains("Quick reference."));
        assert!(visible.contains("Demo"));
    }
}
