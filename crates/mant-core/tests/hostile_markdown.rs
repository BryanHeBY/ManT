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
    MarkdownOptions, build_outline_with_detail, parse_markdown, query_markdown_text,
    render_excerpt_markdown, render_excerpt_text, render_markdown, render_markdown_with_options,
    render_outline_text, render_query_json, render_query_man, render_query_text,
    render_search_text, search_query, select_excerpt,
};

fn hostile_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/markdown/hostile")
}

/// Run one source through every projection and renderer, asserting result
/// invariants along the way; controlled errors are the only accepted failure.
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

    let rendered = render_markdown(&query);
    let _ = render_query_text(&query);
    let _ = render_query_man(&query);
    render_query_json(&query, true).expect(label);
    // Rendered CommonMark is a public projection: feeding it back through
    // the parser must stay inside controlled behavior too.
    let _ = parse_markdown(&rendered, None);

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
        let mut unique = selectors.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            selectors.len(),
            "{label}: outline paths must be unique"
        );
        assert!(
            selectors.iter().all(|selector| !selector.is_empty()),
            "{label}: outline paths must be non-empty"
        );
        let excerpt = select_excerpt(&query, &selectors)
            .unwrap_or_else(|error| panic!("{label}: outline path must be selectable: {error}"));
        let _ = render_excerpt_markdown(&excerpt);
        let _ = render_excerpt_text(&excerpt);
    }

    let addressable = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
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
            let Ok(result) = search_query(&query, &request) else {
                continue;
            };
            let _ = render_search_text(&result);
            verify_search_result(label, &query, &result, &addressable, scope);
        }
    }

    // A sample taken from the rendered body must always be findable; the
    // leading label header is presentation and owns no search node.
    let body = addressable
        .split_once('\n')
        .map_or("", |(_, remainder)| remainder);
    if let Some(word) = first_ascii_word(body) {
        let request = SearchQuery {
            pattern: word.clone(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Markdown,
            word: false,
            context_lines: 0,
            limit: default_search_limit(),
            offset: 0,
        };
        let found = search_query(&query, &request)
            .unwrap_or_else(|error| panic!("{label}: sampled search failed: {error}"));
        assert!(
            found.total >= 1,
            "{label}: sampled word {word:?} from the render must be found"
        );
    }
}

fn verify_search_result(
    label: &str,
    query: &mant_ast::QueryBundle,
    result: &mant_ast::QuerySearch,
    addressable: &str,
    scope: SearchScope,
) {
    assert_eq!(
        result.returned as usize,
        result.matches.len(),
        "{label}: returned count must match the match list"
    );
    assert!(
        result.total >= result.returned,
        "{label}: total covers returned matches"
    );
    for found in &result.matches {
        let start = usize::try_from(found.markdown.start_byte).expect("start fits usize");
        let end = usize::try_from(found.markdown.end_byte).expect("end fits usize");
        assert!(
            start <= end && end <= addressable.len(),
            "{label}: match byte range must stay inside the render"
        );
        assert!(
            addressable.is_char_boundary(start) && addressable.is_char_boundary(end),
            "{label}: match byte range must sit on char boundaries"
        );
        if scope == SearchScope::Markdown {
            assert_eq!(
                &addressable[start..end],
                found.matched_text,
                "{label}: markdown-scope coordinates must slice the matched text"
            );
        }
        assert!(
            found.markdown.start_line >= 1 && found.markdown.start_line <= result.render.line_count,
            "{label}: match line must exist in the render"
        );
        let selector = vec![found.node.path().to_owned()];
        select_excerpt(query, &selector).unwrap_or_else(|error| {
            panic!("{label}: match node {selector:?} must be selectable: {error}")
        });
    }
}

fn first_ascii_word(text: &str) -> Option<String> {
    let word: String = text
        .chars()
        .skip_while(|character| !character.is_ascii_alphanumeric())
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    (!word.is_empty()).then_some(word)
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
    let crlf = "<!-- mant:tldr:start -->\r\n# tool\r\n\r\n> CRLF quick reference.\r\n\r\n- Run:\r\n\r\n`tool {{x}}`\r\n<!-- mant:tldr:end -->\r\n\r\n# CRLF Document\r\n\r\nLine one\r\nLine with a lone \r carriage return inside\r\n\r\n- `-a`: option with CRLF ending\r\n";
    exercise("crlf-mixed", crlf);

    let controls = "# BOM and controls\n\n\u{feff}Text after a BOM mid-document.\n\nControls: \u{1} \u{2} \u{7} \u{1b}[31mANSI\u{1b}[0m \u{7f} end.\n\nNul\u{0}byte, vertical\u{b}tab, and form\u{c}feed.\n";
    exercise("control-characters", controls);

    exercise("lone-cr-only", "line one\rline two\rline three\r");
}

#[test]
#[allow(clippy::single_element_loop)]
fn fuzz_minimized_regressions_never_panic() {
    // Each entry is a minimized crash input found by the fuzz targets.
    for (label, source) in [
        // Search visible-text mapping advanced into the middle of a
        // multibyte character when a code span crossed a line break.
        ("codespan-linebreak-multibyte", "`\n: 节`"),
    ] {
        exercise(label, source);
    }
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

    let marker_flood = "<!-- mant:tldr:start -->\n".repeat(2_000);
    exercise("tldr-marker-flood", &marker_flood);

    let fence_flood = format!("# Fences\n\n{}", "```\n".repeat(2_000));
    exercise("unclosed-fence-flood", &fence_flood);
}
