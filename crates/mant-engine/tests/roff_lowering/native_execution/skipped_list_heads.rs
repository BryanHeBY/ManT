//! Plain-list handlers decide whether HEAD children execute before lowering.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    source: String,
    baseline_source: String,
    kind: String,
    rows: Vec<String>,
    baseline_rows: Vec<String>,
    reading_rule: String,
}

fn item_head(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It") {
        return node
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Head);
    }
    node.children.iter().find_map(item_head)
}

fn query(source: &str) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = mant_render::render_query_json(&loaded, false).unwrap();
    assert!(!wire.contains("\\u0000mant:"));
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored: ResolvedContent = decoded.into();
    assert_eq!(restored.document, loaded.document);
    restored
}

fn description_rows(query: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(query);
    let rows = rendered.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let boundary = rows.iter().position(|row| *row == "NEXT").unwrap();
    let spacing = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "NEXT")
        .unwrap()
        .spacing_before_lines;
    rows[start..boundary - usize::from(spacing)]
        .iter()
        .map(|row| row.trim_start_matches(' ').to_owned())
        .collect()
}

#[test]
fn declined_list_heads_have_no_text_font_link_or_line_side_effects() {
    // Every exact source and its baseline ran all five registered pristine
    // profiles before recording the fixture. mdoc_term.c::termp_it_pre()
    // (916-927) returns zero for item/bullet/dash/enum HEAD children, even
    // for parser-recovered Xo subtrees. Generated markers and real It post
    // still execute. The paired pristine rows prove HEAD invariance without
    // substituting a terminal-marker geometry contract for portable lists.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/skipped_list_heads/cases.json")).unwrap();
    assert_eq!(cases.len(), 128);
    let mut failures = Vec::new();
    for case in cases {
        assert_eq!(case.reading_rule, "skipped-head-execution-invariance");
        assert_eq!(case.rows, case.baseline_rows, "{}", case.name);
        let parsed = Parser::default()
            .parse_bytes("skipped-list-head.1", case.source.as_bytes())
            .unwrap();
        let head = item_head(&parsed.document.root).expect("It HEAD ownership witness");
        assert!(!head.children.is_empty(), "{}: {head:?}", case.name);
        let actual = query(&case.source);
        let baseline = query(&case.baseline_source);
        let actual_text = mant_render::render_query_man(&actual);
        let baseline_text = mant_render::render_query_man(&baseline);
        let styled = |query: &ResolvedContent| {
            mant_render::render_query_text_with(query, |presentation, text| {
                format!("[{presentation:?}:{text}]")
            })
        };
        let markdown = mant_codec::encode::render_markdown(&actual);
        if actual_text != baseline_text
            || styled(&actual) != styled(&baseline)
            || markdown != mant_codec::encode::render_markdown(&baseline)
            || markdown.contains("head.invalid")
            || (case.kind == "item" && description_rows(&actual) != case.rows)
        {
            failures.push(format!(
                "{}\n{}\nactual {actual_text:?}\nbaseline {baseline_text:?}\nnative {:?}",
                case.name, case.source, case.rows
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
