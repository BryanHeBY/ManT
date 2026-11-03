//! Frozen full responses captured from the 7bd6222d query implementation.
//! The fixture constructs IR directly and does not depend on roff execution.
use super::{query, request};
use crate::{ResolvedContent, search_query};
use mant_ir::{Block, Inline, LayoutHint, Section};
use mant_protocol::{QuerySearch, SearchCase, SearchQuery, SearchScope, SearchSyntax};

fn compatibility_content() -> ResolvedContent {
    let mut content = query();
    let document = content.document.as_mut().unwrap();
    document.sections[0].blocks.push(Block::Preformatted {
        children: vec![Inline::Text {
            value: "a A a  \n中e\u{301} a\nlast  \n".into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    });
    document.sections.push(Section {
        id: "next".into(),
        fragment_aliases: Vec::new(),
        heading: "NEXT".into(),
        spacing_before_lines: 0,
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: "a following owner".into(),
            }],
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    });
    document.diagnostics.push(mant_ir::Diagnostic {
        impact: mant_ir::DiagnosticImpact::ContentCoverage,
        level: mant_ir::DiagnosticLevel::Warning,
        code: Some("test.coverage".into()),
        message: "coverage summary survives pagination".into(),
        source: None,
    });
    content
}

fn compatibility_requests() -> Vec<SearchQuery> {
    let mut requests = Vec::new();
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        for (pattern, syntax) in [
            ("a", SearchSyntax::Literal),
            ("中e\u{301}", SearchSyntax::Literal),
            (r"a|中|e\x{301}", SearchSyntax::Regex),
            (r"\s+", SearchSyntax::Regex),
            ("a id=", SearchSyntax::Literal),
            ("\"></a>`--acls", SearchSyntax::Literal),
            (r"lists(?s:.*?)NEXT", SearchSyntax::Regex),
            ("  \n", SearchSyntax::Literal),
            ("a  \n中", SearchSyntax::Literal),
        ] {
            for offset in [0, 1, 20] {
                requests.push(SearchQuery {
                    scope,
                    syntax,
                    case: SearchCase::Sensitive,
                    offset,
                    limit: 1,
                    ..request(pattern)
                });
            }
        }
    }
    requests.push(SearchQuery {
        case: SearchCase::Insensitive,
        context_lines: 0,
        limit: 2,
        ..request("A")
    });
    requests.push(SearchQuery {
        case: SearchCase::Smart,
        context_lines: 2,
        limit: 10,
        ..request("A")
    });
    requests
}

#[test]
fn search_responses_match_the_pre_optimization_baseline_field_for_field() {
    let expected: Vec<QuerySearch> =
        serde_json::from_str(include_str!("compatibility.json")).unwrap();
    let content = compatibility_content();
    let requests = compatibility_requests();
    assert_eq!(requests.len(), expected.len());
    for (request, expected) in requests.iter().zip(expected) {
        let actual = search_query(&content, request).unwrap();
        assert_eq!(actual, expected, "request: {request:?}");
        let wire = serde_json::to_string(&actual).unwrap();
        assert_eq!(serde_json::from_str::<QuerySearch>(&wire).unwrap(), actual);
    }
}
