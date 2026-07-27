//! Arbitrary or hostile Markdown input must never panic anywhere in the
//! pipeline: parse, outline, excerpt, search, and every textual renderer.
//!
//! Checked-in fixtures under `tests/fixtures/markdown/hostile/` capture
//! byte-level and structural edge cases; programmatic inputs cover shapes
//! that would be unreadable or wasteful as files. Controlled `Err` results
//! are acceptable; panics and lost input are not.

use std::{fs, path::PathBuf};

use mant_ast::{
    OutlineDetail, OutlineNode, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    default_search_limit,
};
use mant_core::{
    build_outline_with_detail, query_markdown_text, render_excerpt_markdown, render_excerpt_text,
    render_markdown, render_outline_text, render_query_json, render_query_man, render_query_text,
    render_search_text, search_query, select_excerpt,
};

fn hostile_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/markdown/hostile")
}

/// Run one source through every projection and renderer without panicking.
fn exercise(label: &str, source: &str) {
    let query = match query_markdown_text(source, Some(format!("hostile/{label}"))) {
        Ok(query) => query,
        Err(error) => {
            assert!(
                !error.to_string().is_empty(),
                "{label}: errors must render a message"
            );
            return;
        }
    };

    let _ = render_markdown(&query);
    let _ = render_query_text(&query);
    let _ = render_query_man(&query);
    render_query_json(&query, true).expect(label);

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
        let excerpt = select_excerpt(&query, &selectors)
            .unwrap_or_else(|error| panic!("{label}: outline path must be selectable: {error}"));
        let _ = render_excerpt_markdown(&excerpt);
        let _ = render_excerpt_text(&excerpt);
    }

    for (pattern, syntax) in [
        ("a", SearchSyntax::Literal),
        ("—", SearchSyntax::Literal),
        (":::", SearchSyntax::Literal),
        ("{{", SearchSyntax::Literal),
        (".*", SearchSyntax::Regex),
        ("^", SearchSyntax::Regex),
        ("(a+)+$", SearchSyntax::Regex),
        ("[", SearchSyntax::Regex),
    ] {
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            let request = SearchQuery {
                pattern: pattern.to_owned(),
                syntax,
                case: SearchCase::Smart,
                scope,
                word: false,
                context_lines: 2,
                limit: default_search_limit(),
                offset: 0,
            };
            if let Ok(result) = search_query(&query, &request) {
                let _ = render_search_text(&result);
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

#[test]
fn hostile_fixtures_never_panic_across_the_pipeline() {
    let directory = hostile_fixture_dir();
    let mut entries: Vec<PathBuf> = fs::read_dir(&directory)
        .expect("hostile fixture directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    entries.sort();
    assert!(
        entries.len() >= 8,
        "hostile fixture corpus unexpectedly small: {entries:?}"
    );

    for path in entries {
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture name")
            .to_owned();
        let source = fs::read_to_string(&path).expect("hostile fixtures are UTF-8");
        exercise(&label, &source);
    }
}

#[test]
fn line_ending_and_control_byte_variants_never_panic() {
    let crlf = ":::tldr\r\n# tool\r\n\r\n> CRLF quick reference.\r\n\r\n- Run:\r\n\r\n`tool {{x}}`\r\n:::\r\n\r\n# CRLF Document\r\n\r\nLine one\r\nLine with a lone \r carriage return inside\r\n\r\n- `-a`: option with CRLF ending\r\n";
    exercise("crlf-mixed", crlf);

    let controls = "# BOM and controls\n\n\u{feff}Text after a BOM mid-document.\n\nControls: \u{1} \u{2} \u{7} \u{1b}[31mANSI\u{1b}[0m \u{7f} end.\n\nNul\u{0}byte, vertical\u{b}tab, and form\u{c}feed.\n";
    exercise("control-characters", controls);

    exercise("lone-cr-only", "line one\rline two\rline three\r");
}

#[test]
fn pathological_structures_never_panic() {
    let deep_quotes = format!("{} deep quote\n", ">".repeat(2048));
    exercise("deep-blockquotes", &deep_quotes);

    let mut deep_list = String::new();
    for depth in 0..512 {
        deep_list.push_str(&"  ".repeat(depth));
        deep_list.push_str("- item\n");
    }
    exercise("deep-lists", &deep_list);

    let deep_emphasis = format!(
        "# Deep emphasis\n\n{}x{}\n",
        "*_".repeat(512),
        "_*".repeat(512)
    );
    exercise("deep-emphasis", &deep_emphasis);

    let mut deep_headings = String::new();
    for _ in 0..256 {
        deep_headings.push_str("# a\n## b\n### c\n#### d\n##### e\n###### f\n");
    }
    exercise("many-headings", &deep_headings);

    let long_line = format!("# Long\n\n{}\n", "word ".repeat(50_000));
    exercise("long-line", &long_line);

    let unclosed_inline = format!("# Unclosed\n\n{}\n", "`*_[".repeat(10_000));
    exercise("unclosed-inline-runs", &unclosed_inline);

    let marker_flood = ":::tldr\n".repeat(2_000);
    exercise("tldr-marker-flood", &marker_flood);

    let fence_flood = format!("# Fences\n\n{}", "```\n".repeat(2_000));
    exercise("unclosed-fence-flood", &fence_flood);
}
