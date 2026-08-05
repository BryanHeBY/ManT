#![no_main]

use libfuzzer_sys::fuzz_target;
use mant_ast::{
    OutlineDetail, OutlineNode, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    default_search_limit,
};
use mant_core::{
    build_outline_with_detail, query_markdown_text, render_excerpt_markdown, render_excerpt_text,
    render_markdown, render_outline_text, render_query_man, render_query_text, render_search_text,
    search_query, select_excerpt,
};

fn collect_paths(nodes: &[OutlineNode], selectors: &mut Vec<String>) {
    for node in nodes {
        selectors.push(node.path().to_owned());
        collect_paths(node.children(), selectors);
    }
}

fuzz_target!(|data: &str| {
    let Ok(query) = query_markdown_text(data, None) else {
        return;
    };

    let _ = render_markdown(&query);
    let _ = render_query_text(&query);
    let _ = render_query_man(&query);

    for detail in [OutlineDetail::Sections, OutlineDetail::Options] {
        let Ok(outline) = build_outline_with_detail(&query, detail) else {
            continue;
        };
        let _ = render_outline_text(&outline);
        let mut selectors = Vec::new();
        collect_paths(&outline.nodes, &mut selectors);
        if selectors.is_empty() {
            continue;
        }
        let excerpt = select_excerpt(&query, &selectors).expect("outline paths select");
        let _ = render_excerpt_markdown(&excerpt);
        let _ = render_excerpt_text(&excerpt);
    }

    // Reuse a slice of the input as the pattern to search generated output.
    let pattern: String = data.chars().take(16).collect();
    for syntax in [SearchSyntax::Literal, SearchSyntax::Regex] {
        let request = SearchQuery {
            pattern: pattern.clone(),
            syntax,
            case: SearchCase::Smart,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 2,
            limit: default_search_limit(),
            offset: 0,
        };
        if let Ok(result) = search_query(&query, &request) {
            let _ = render_search_text(&result);
        }
    }
});
