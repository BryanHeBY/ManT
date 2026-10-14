//! Pristine execution rows and portable viewport alignment are separate axes.

use mant_ir::ResolvedContent;
use serde_json::Value;

#[path = "row_presentation/synopsis.rs"]
mod synopsis;

fn cases() -> Vec<Value> {
    let matrix: Value =
        serde_json::from_str(include_str!("row_presentation/alignment.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 124);
    assert_eq!(matrix["header"]["expectations_from_product"], false);
    assert_eq!(
        matrix["header"]["oracle_sha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    matrix["cases"].as_array().unwrap().clone()
}

fn roundtrip(case: &Value) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    assert!(
        !wire.contains("\\u0000mant:"),
        "private receipt escaped: {}",
        case["id"]
    );
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored: ResolvedContent = restored.into();
    assert_eq!(query, restored);
    restored
}

fn body_rows(rendered: &str) -> Vec<String> {
    let lines: Vec<_> = rendered.split_terminator('\n').collect();
    let start = lines
        .iter()
        .position(|line| *line == "PROBE_START")
        .unwrap();
    let end = lines.iter().position(|line| *line == "PROBE_END").unwrap();
    assert_eq!(
        lines[end - 1],
        "",
        "one section separator, not body furniture"
    );
    lines[start + 1..end - 1]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

fn native_rows(case: &Value, profile: &str) -> Vec<String> {
    case["profiles"][profile]["body_rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn aligned_execution_rows_survive_wire_and_two_known_viewports() {
    // All 124 complete sources ran pristine ASCII/UTF-8/HTML/tree/lint
    // before this fixture or assertion was written. roff_term_pre_ce()
    // groups by TEXT NODE_LINE and unconditionally flushes each group.
    // term.c::term_flushln centers/right-aligns only accepted graph rows.
    for case in cases().iter().filter(|case| case["geometry"] == true) {
        let query = roundtrip(case);
        for width in [40, 80] {
            let profile = format!("utf8-{width}");
            // Native Sh BODY's five-cell common origin is absent in the
            // plain CLI projection. Both then have the same available area.
            let expected: Vec<_> = native_rows(case, &profile)
                .iter()
                .map(|row| {
                    let leading = row.bytes().take_while(|byte| *byte == b' ').count();
                    format!(
                        "{}{}",
                        " ".repeat(leading.saturating_sub(5)),
                        &row[leading..]
                    )
                })
                .collect();
            let actual = body_rows(&mant_render::render_query_text_at_width(
                &query,
                width - 5,
                |_, text| text.to_owned(),
            ));
            assert_eq!(actual, expected, "{} width={width}", case["id"]);
        }
    }
}

fn reading_rows(rows: &[String]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.split([' ', '\u{a0}'])
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn aligned_head(node: &libmandoc_rs::Node, head: bool) -> bool {
    let head = head
        || node.kind == libmandoc_rs::NodeKind::Head && node.macro_token.as_deref() == Some("It");
    head && matches!(node.macro_token.as_deref(), Some("ce" | "rj"))
        || node.children.iter().any(|child| aligned_head(child, head))
}

#[test]
fn alignment_inside_actual_definition_heads_consumes_one_native_flush() {
    for case in cases().iter().filter(|case| case["head"] == true) {
        let source = case["source"].as_str().unwrap();
        let ast = libmandoc_rs::Parser::default()
            .parse_bytes("alignment-head.7", source.as_bytes())
            .unwrap();
        assert!(
            aligned_head(&ast.document.root, false),
            "{}: actual HEAD, not inferred from source spelling",
            case["id"]
        );
        let query = roundtrip(case);
        if case["partial"] == true {
            assert!(
                query
                    .document
                    .as_ref()
                    .unwrap()
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code.as_deref()
                        == Some("manual.partial-row-alignment")
                        && diagnostic.impact == mant_ir::DiagnosticImpact::None),
                "{}: partial-row layout reduction must be explicit",
                case["id"]
            );
        }
        // Native field padding remains responsive. Compare every hard row
        // and every word seam; generated inset NBSP-only rows remain empty
        // physical rows, not furniture to strip from the oracle.
        assert_eq!(
            reading_rows(&body_rows(&mant_render::render_query_text(&query))),
            reading_rows(&native_rows(case, "utf8-80")),
            "{}",
            case["id"]
        );
    }
}

#[test]
fn aligned_column_fields_close_only_their_own_actual_device_rows() {
    // term_flushln CENTER/RIGHT changes actual viscol. A later empty column
    // post can overrun its field and endline (term.c:150-160,235-253;
    // mdoc_term.c::termp_it_post()). Each source/profile ran pristine first.
    for case in cases().iter().filter(|case| case["column"] == true) {
        let query = roundtrip(case);
        let rows = body_rows(&mant_render::render_query_text(&query));
        assert_eq!(
            reading_rows(&rows),
            reading_rows(&native_rows(case, "utf8-80")),
            "{}: hard rows, not soft geometry",
            case["id"]
        );
        let section = query
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "PROBE_START")
            .unwrap();
        let mant_ir::Block::Table { rows, .. } = &section.blocks[0] else {
            panic!("actual column table")
        };
        assert!(
            rows[0].cells[0].break_after,
            "{}: accepted aligned field ends at its real post",
            case["id"]
        );
    }
}

#[test]
fn alignment_preserves_addressable_markdown_and_visible_search() {
    for case in cases() {
        let query = roundtrip(&case);
        let section = query
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "PROBE_START")
            .unwrap();
        let artifact = mant_codec::encode::render_addressable_markdown(&query);
        assert!(artifact.text().contains(case["needle"].as_str().unwrap()));
        let search = mant_query::search_query(
            &query,
            &mant_protocol::SearchQuery {
                pattern: case["needle"].as_str().unwrap().into(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap();
        assert!(
            search
                .matches
                .iter()
                .any(|hit| hit.outline.node.id() == section.id.as_str()
                    || hit
                        .outline
                        .ancestors
                        .iter()
                        .any(|ancestor| ancestor.id.as_str() == section.id.as_str())),
            "{}: content owner lost",
            case["id"]
        );
        let excerpt = mant_query::select_excerpt(
            &query,
            &[mant_protocol::ContentSelector::id(section.id.clone())],
        )
        .unwrap();
        let wire = serde_json::to_string(&excerpt).unwrap();
        assert_eq!(
            serde_json::from_str::<mant_protocol::QueryExcerpt>(&wire).unwrap(),
            excerpt
        );
        let reread =
            mant_loader::load_markdown_text(artifact.text(), Some("alignment.md".into())).unwrap();
        if case["geometry"] == true {
            // Ordinary Markdown omits alignment and uses paragraph spacing.
            // Check every nonblank execution row and word seam on its first
            // read, independently of repeated export. Exact native blank
            // rows are checked by the viewport matrix above. Addressable
            // HTML markers belong to the separate artifact mapping axis.
            let clean = mant_codec::encode::render_markdown(&query);
            let portable = mant_loader::load_markdown_text(&clean, None).unwrap();
            let nonblank = |rows: Vec<String>| {
                reading_rows(&rows)
                    .into_iter()
                    .filter(|row| !row.is_empty())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                nonblank(body_rows(&mant_render::render_query_text(&portable))),
                nonblank(native_rows(&case, "utf8-80")),
                "{}: first Markdown read changed execution rows",
                case["id"]
            );
        }
        let repeated = mant_codec::encode::render_markdown(&reread);
        let second =
            mant_loader::load_markdown_text(&repeated, Some("alignment-again.md".into())).unwrap();
        assert_eq!(
            body_rows(&mant_render::render_query_text(&reread)),
            body_rows(&mant_render::render_query_text(&second)),
            "{}: export/read hard rows must be stable",
            case["id"]
        );
    }
}
