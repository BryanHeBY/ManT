//! Validate and compile one request before executing it against documents.
use super::SearchError;
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use mant_protocol::{MAX_SEARCH_PATTERN_CHARS, SearchCase, SearchQuery, SearchSyntax};
use regex_syntax::ParserBuilder;
use std::fmt;
pub(super) const MAX_CONTEXT_LINES: u16 = 100;
pub(super) const MAX_SEARCH_LIMIT: u32 = 10_000;
pub(super) const MAX_REGEX_COMPILED_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_REGEX_DFA_CACHE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct SearchPlan<'a> {
    pub(super) request: &'a SearchQuery,
    pub(super) matcher: grep_regex::RegexMatcher,
}
impl<'a> SearchPlan<'a> {
    pub(crate) fn new(request: &'a SearchQuery) -> Result<Self, SearchError> {
        validate_request(request)?;
        Ok(Self {
            request,
            matcher: build_matcher(request)?,
        })
    }
    pub(crate) fn execute(
        &self,
        document: &crate::ResolvedContent,
        offset: u32,
        limit: u32,
    ) -> Result<mant_protocol::QuerySearch, SearchError> {
        let request = SearchQuery {
            offset,
            limit,
            ..self.request.clone()
        };
        validate_request(&request)?;
        super::search_with_matcher(document, &request, &self.matcher)
    }
}

/// Validate search limits and compile its matcher without loading a manual.
///
/// # Errors
///
/// Returns the same [`SearchError`] variants as [`super::search_query`].
pub fn validate_search_query(request: &SearchQuery) -> Result<(), SearchError> {
    validate_request(request)?;
    build_matcher(request).map(|_| ())
}

pub(super) fn validate_request(request: &SearchQuery) -> Result<(), SearchError> {
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

pub(super) fn build_matcher(
    request: &SearchQuery,
) -> Result<grep_regex::RegexMatcher, SearchError> {
    validate_pattern_semantics(request)?;
    let mut builder = RegexMatcherBuilder::new();
    builder
        .fixed_strings(request.syntax == SearchSyntax::Literal)
        .multi_line(true)
        .size_limit(MAX_REGEX_COMPILED_BYTES)
        .dfa_size_limit(MAX_REGEX_DFA_CACHE_BYTES)
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
    let matcher = builder.build(&request.pattern).map_err(matcher_error)?;
    if matcher.is_match(b"").map_err(matcher_error)? {
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

pub(super) fn empty_match_error() -> SearchError {
    SearchError::InvalidPattern("pattern must not match empty text".to_owned())
}

pub(super) fn non_utf8_pattern_error() -> SearchError {
    SearchError::InvalidPattern(
        "regular expressions must preserve UTF-8 character boundaries; Unicode mode cannot be disabled"
            .to_owned(),
    )
}

pub(super) fn matcher_error(error: impl fmt::Display) -> SearchError {
    let message = error.to_string();
    if message.contains("compiled regex exceeds size limit") {
        SearchError::InvalidPattern(
            "regular expression exceeds ManT's compiled-size limit".to_owned(),
        )
    } else {
        SearchError::InvalidPattern(message)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mant_protocol::SearchScope;

    fn request() -> SearchQuery {
        SearchQuery {
            pattern: "(?i)\\b(alpha|beta|gamma|delta|epsilon)\\b".into(),
            syntax: SearchSyntax::Regex,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            offset: 0,
            limit: 10,
        }
    }

    #[test]
    fn reused_plan_preserves_paging_and_validates_execution_limits() {
        let document = crate::query_fixture::markdown("# Plan\n\nalpha\n\nbeta\n", None).unwrap();
        let request = request();
        let plan = SearchPlan::new(&request).unwrap();
        for offset in [0, 1, 9] {
            let result = plan.execute(&document, offset, 1).unwrap();
            assert_eq!(result.total, 2);
            assert_eq!(result.returned, u32::from(offset < 2));
            assert_eq!(result.query.offset, offset);
            assert_eq!(result.query.limit, 1);
            if offset == 1 {
                assert_eq!(result.matches[0].preview, "beta");
            }
        }
        assert_eq!(
            plan.execute(&document, 0, 0),
            Err(SearchError::InvalidLimit)
        );
    }

    #[test]
    #[ignore = "manual request-local matcher reuse timing"]
    fn measure_request_local_matcher_reuse() {
        use std::{hint::black_box, time::Instant};
        let document =
            crate::query_fixture::markdown("# Plan\n\nalpha beta gamma delta epsilon\n", None)
                .unwrap();
        let request = request();
        let start = Instant::now();
        for _ in 0..1000 {
            black_box(super::super::search_query(&document, &request).unwrap());
        }
        let rebuilt = start.elapsed();
        let start = Instant::now();
        let plan = SearchPlan::new(&request).unwrap();
        for _ in 0..1000 {
            black_box(plan.execute(&document, 0, request.limit).unwrap());
        }
        eprintln!(
            "1000 small documents: rebuild={rebuilt:?}, reuse={:?}",
            start.elapsed()
        );
    }
}
