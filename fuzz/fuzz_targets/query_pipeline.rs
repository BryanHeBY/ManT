use mant_engine::{
    build_outline_with_detail, render_excerpt_json, render_excerpt_markdown, render_excerpt_text,
    render_markdown, render_outline_json, render_outline_markdown, render_outline_text,
    render_query_json, render_query_man, render_query_text, render_search_json,
    render_search_markdown, render_search_text, search_query, select_excerpt, select_explanation,
};
use mant_protocol::{
    OutlineDetail, OutlineNode, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

pub const MAX_INPUT_BYTES: usize = 64 * 1024;

pub fn exercise(query: &mant_engine::ResolvedContent, pattern_seed: &str) {
    let _ = render_markdown(query);
    let _ = render_query_text(query);
    let _ = render_query_man(query);
    let _ = render_query_json(query, false);
    let _ = render_query_json(query, true);

    for detail in [OutlineDetail::Sections, OutlineDetail::Entries] {
        let Ok(outline) = build_outline_with_detail(query, detail) else {
            continue;
        };
        let _ = render_outline_text(&outline);
        let _ = render_outline_markdown(&outline);
        let _ = render_outline_json(&outline, false);
        let _ = render_outline_json(&outline, true);

        let mut selectors = Vec::new();
        collect_paths(&outline.nodes, &mut selectors);
        if selectors.is_empty() {
            continue;
        }
        let excerpt = select_excerpt(query, &selectors).expect("outline paths select");
        let _ = render_excerpt_markdown(&excerpt);
        let _ = render_excerpt_text(&excerpt);
        let _ = render_excerpt_json(&excerpt, false);
        let _ = render_excerpt_json(&excerpt, true);

        for selector in selectors.iter().take(8) {
            let _ = select_explanation(query, selector);
        }
    }

    // Keep regex compilation and result generation bounded while preserving
    // arbitrary Unicode and metacharacters from the fuzzer input.
    let pattern: String = pattern_seed.chars().take(32).collect();
    for syntax in [SearchSyntax::Literal, SearchSyntax::Regex] {
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            let request = SearchQuery {
                pattern: pattern.clone(),
                syntax,
                case: SearchCase::Smart,
                scope,
                word: false,
                context_lines: 2,
                limit: 32,
                offset: 0,
            };
            if let Ok(result) = search_query(query, &request) {
                let _ = render_search_text(&result);
                let _ = render_search_markdown(&result);
                let _ = render_search_json(&result, false);
                let _ = render_search_json(&result, true);
            }
        }
    }
}

fn collect_paths(nodes: &[OutlineNode], selectors: &mut Vec<String>) {
    for node in nodes {
        selectors.push(node.path().to_owned());
        collect_paths(node.children(), selectors);
    }
}
