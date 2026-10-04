//! Frozen CVS column execution and separately declared portable reading geometry.
use std::ops::ControlFlow;

use libmandoc_rs::{Node, NodeKind, RenderFormat, Renderer};
use mant_ir::{Block, ContentBlockStep, ResolvedContent, Section, TableRow};
use serde_json::{Value, json};

fn cases() -> Vec<Value> {
    let matrix: Value =
        serde_json::from_str(include_str!("column_preferences/cases.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 23);
    assert_eq!(matrix["header"]["profiles_per_source"], 5);
    assert_eq!(matrix["header"]["expectations_from_product"], false);
    assert_eq!(
        matrix["header"]["oracle_sha256"],
        "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
    );
    matrix["cases"].as_array().unwrap().clone()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}

fn boundary_cases() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("column_preferences/cell_boundaries.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 17);
    assert_eq!(fixture["header"]["expectations_from_product"], false);
    fixture["cases"].as_array().unwrap().clone()
}
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| text(value).to_owned())
        .collect()
}

fn terminal_projection(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character == '\u{8}' {
            result.pop();
        } else {
            result.push(character);
        }
    }
    result
}

fn native_body<'a>(value: &'a str, case: &Value) -> &'a str {
    let stop = format!("{}\n", text(&case["stop_heading"]));
    let mut start = None;
    let mut offset = 0;
    for line in value.split_inclusive('\n') {
        let visible = terminal_projection(line);
        if visible == "DESCRIPTION\n" {
            start = Some(offset + line.len());
        } else if visible == stop
            && let Some(start) = start
        {
            // Sh's single separating newline is outside the body. Identify
            // headings after overstrike projection, but return untouched raw
            // bytes. Header/footer OS and platform names never enter the slice.
            assert_eq!(value.as_bytes()[offset - 1], b'\n');
            return &value[start..offset - 1];
        }
        offset += line.len();
    }
    panic!("missing native section boundaries")
}

#[test]
fn raw_body_boundary_projection_keeps_styling_and_excludes_host_furniture() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "linked-gap2")
        .unwrap();
    for name in ["ascii", "utf8"] {
        let raw = text(&case["profiles"][name]["stdout"]);
        assert!(!raw.contains("DESCRIPTION\n"));
        let changed = raw
            .replace(
                "Linux 6.18.40.1-microsoft-standard-WSL2",
                "Darwin Portable Host",
            )
            .replace("TEST(1)", "OTHER(9)");
        assert_eq!(native_body(&changed, &case), native_body(raw, &case));
        assert_eq!(
            terminal_projection(native_body(raw, &case))
                .split_terminator('\n')
                .collect::<Vec<_>>(),
            strings(&case["native_rows"])
        );
        assert!(native_body(raw, &case).contains('\u{8}'));
    }
}

fn find_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It") {
        Some(node)
    } else {
        node.children.iter().find_map(find_item)
    }
}

fn ast_texts(node: &Node, result: &mut Vec<Value>) {
    if node.kind == NodeKind::Text {
        result.push(json!({ "value": node.text.as_deref().unwrap_or_default(),
            "line": node.line, "column": node.column,
            "line_start": node.flags.line_start, "no_fill": node.flags.no_fill }));
    }
    for child in &node.children {
        ast_texts(child, result);
    }
}

fn assert_ast(case: &Value) {
    let parsed = libmandoc_rs::Parser::default()
        .parse_bytes("column-preferences.1", text(&case["source"]).as_bytes())
        .unwrap();
    let item = find_item(&parsed.document.root).unwrap();
    assert_eq!((item.line, item.column), (9, 2));
    let cells = item
        .children
        .iter()
        .filter(|node| {
            node.kind == NodeKind::Body
                && node.macro_token.as_deref() == Some("It")
                && node.scope_end.is_none()
        })
        .map(|node| {
            let mut texts = Vec::new();
            ast_texts(node, &mut texts);
            json!({ "line": node.line, "column": node.column, "texts": texts })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        json!(cells),
        case["ast_cells"],
        "{}: original BODY cells",
        case["id"]
    );
}

#[test]
fn pristine_column_profiles_and_ast_cells_remain_frozen() {
    // Every exact source ran the registered pristine binary before assertions.
    // mdoc_term.c::termp_it_pre sums declared widths plus 4/3/1 gaps, gives
    // excess intermediate fields capacity ten without a gap, and stretches
    // the last field. term_flushln uses configured end, not actual origin;
    // term_ascii.c::ascii_advance/locale_advance cap each advance at 256.
    for case in cases() {
        assert_eq!(text(&case["source_sha256"]).len(), 64);
        assert_ast(&case);
        for (name, format) in [("ascii", RenderFormat::Ascii), ("utf8", RenderFormat::Utf8)] {
            let profile = &case["profiles"][name];
            assert_eq!(profile["status"], 0);
            let actual = Renderer::new(format)
                .with_width(78)
                .render_bytes("column-preferences.1", text(&case["source"]).as_bytes())
                .unwrap();
            assert_eq!(
                native_body(&actual.output, &case),
                native_body(text(&profile["stdout"]), &case),
                "{}: raw {name}",
                case["id"]
            );
            let projected = terminal_projection(&actual.output);
            let rows = native_body(&projected, &case)
                .split_terminator('\n')
                .collect::<Vec<_>>();
            assert_eq!(
                rows,
                strings(&case["native_rows"]),
                "{}: hard and empty rows",
                case["id"]
            );
            assert_native_positions(&case, &rows);
        }
        assert_eq!(case["profiles"]["html"]["status"], 0);
        assert_eq!(case["profiles"]["tree"]["status"], 0);
        assert!(case["profiles"]["lint"]["status"].as_u64().unwrap() <= 2);
        assert_eq!(case["profiles"].as_object().unwrap().len(), 5);
    }
}

fn assert_native_positions(case: &Value, rows: &[&str]) {
    for position in case["native_positions"].as_array().unwrap() {
        let row: usize = position["row"].as_u64().unwrap().try_into().unwrap();
        let start = rows[row].find(text(&position["value"])).unwrap();
        assert_eq!(
            mant_ir::geometry::text_width(&rows[row][..start]),
            usize::try_from(position["column"].as_u64().unwrap()).unwrap(),
            "{}: native column",
            case["id"]
        );
    }
}

fn roundtrip(case: &Value) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(text(&case["source"]).as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    assert!(!wire.contains("\\u0000mant:"));
    assert!(!wire.contains("columnWidths"));
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(decoded, original, "{}: actual JSON readback", case["id"]);
    decoded
}

fn description(content: &ResolvedContent) -> &Section {
    content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
}

fn table(content: &ResolvedContent) -> (&mant_ir::ColumnPreferences, &[TableRow]) {
    let [
        Block::Table {
            column_preferences,
            rows,
            source,
            ..
        },
    ] = description(content).blocks.as_slice()
    else {
        panic!("one original table owner")
    };
    assert_eq!(source.as_ref().unwrap().line, 8);
    (column_preferences, rows)
}

fn payload(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                Some(mant_ir::inline_plain_text(children))
            }
            Block::VerticalSpace { .. } => None,
            other => panic!("unexpected column content: {other:#?}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cli_rows(content: &ResolvedContent, case: &Value) -> Vec<String> {
    let output = mant_render::render_query_text(content);
    let body = output
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once(&format!("{}\n", text(&case["stop_heading"])))
        .unwrap()
        .0;
    let mut rows = body
        .split_terminator('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    // This single row belongs to the following section's fixed spacing.
    // Interior completed blanks and open literal tails remain observable.
    assert_eq!(rows.pop().as_deref(), Some(""));
    rows
}

fn assert_reading(content: &ResolvedContent, case: &Value) {
    let (preferences, rows) = table(content);
    assert_eq!(json!(preferences.widths), case["declarations"]);
    assert_eq!(json!(preferences.gap_columns), case["gap"]);
    assert_eq!(preferences.advance_limit_columns, Some(256));
    assert_eq!(preferences.extra_width_columns, Some(10));
    let [row] = rows else {
        panic!("one original row")
    };
    let payloads = row
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| cell_row_payload(cell, case, index))
        .collect::<Vec<_>>();
    assert_eq!(
        payloads,
        strings(&case["payloads"]),
        "{}: no duplicate or trimmed source cells",
        case["id"]
    );
    assert_eq!(
        cli_rows(content, case),
        strings(&case["cli_rows"]),
        "{}: declared reading policy {}",
        case["id"],
        case["reading_policy"]
    );
}

fn cell_row_payload(cell: &mant_ir::TableCell, case: &Value, index: usize) -> String {
    let mut body = payload(&cell.blocks);
    if let Some(inline_payloads) = case.get("inline_payloads") {
        // These four exact Bd sources formerly projected the executed
        // column close as a generated LF. The current source-neutral cell
        // boundary carries that one event. Assert accepted owner scalars
        // separately before composing the legacy hard-row snapshot view.
        // mdoc_term.c::print_mdoc_node empty TEXT -> term_vspace (358-376),
        // Bd BODY post -> term_newln (1478-1500); term.c:475-497.
        assert_eq!(
            body,
            text(&inline_payloads[index]),
            "{}: accepted owner body",
            case["id"]
        );
        if case["payload_close_endings"][index] == true {
            assert!(
                cell.break_after,
                "{}: executed current tail close",
                case["id"]
            );
            if body.ends_with('\n') {
                let Some(Block::Preformatted { children, .. }) = cell.blocks.last() else {
                    panic!("completed literal tail keeps its original owner")
                };
                assert!(
                    matches!(children.last(), Some(mant_ir::Inline::Text { value }) if value.is_empty())
                );
            }
            body.push('\n');
        }
    }
    body
}

fn assert_link_owners(content: &ResolvedContent, case: &Value) {
    let document = content.document.as_ref().unwrap();
    let mut links = Vec::new();
    let report = mant_ir::scan_reference_scope(
        document,
        mant_ir::ReferenceScope::Block {
            sections: &[1],
            blocks: &[ContentBlockStep::Block { index: 0 }],
        },
        mant_ir::ReferenceScanLimits::default(),
        |occurrence, _| {
            assert_eq!(
                occurrence.location.resolve_link(document),
                Some(occurrence.link)
            );
            links.push((
                mant_ir::inline_plain_text(occurrence.label),
                occurrence.target.clone(),
            ));
            assert!(matches!(
                occurrence.label,
                [mant_ir::Inline::Emphasis { .. }]
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    let expected = if case["linked"] == true {
        strings(&case["payloads"])
    } else {
        Vec::new()
    };
    assert_eq!(links.len(), expected.len());
    for ((label, target), expected) in links.iter().zip(expected) {
        assert_eq!(label, &expected);
        assert_eq!(
            *target,
            mant_ir::LinkTarget::Section {
                id: expected.to_ascii_lowercase().into()
            }
        );
    }
}

fn assert_query(content: &ResolvedContent, case: &Value) {
    let section = description(content);
    let excerpt = mant_query::select_excerpt(
        content,
        &[mant_protocol::ContentSelector::id(section.id.clone())],
    )
    .unwrap();
    let wire = serde_json::to_string(&excerpt).unwrap();
    let restored: mant_protocol::QueryExcerpt = serde_json::from_str(&wire).unwrap();
    assert_eq!(restored, excerpt);
    let [
        mant_protocol::ExcerptSelection::DocumentSection {
            section: selected, ..
        },
    ] = restored.selections.as_slice()
    else {
        panic!("complete original section")
    };
    assert_eq!(selected, section);
    let artifact = mant_codec::encode::render_addressable_markdown(content);
    for word in strings(&case["payloads"])
        .iter()
        .flat_map(|value| value.split_whitespace())
    {
        for scope in [
            mant_protocol::SearchScope::Visible,
            mant_protocol::SearchScope::Markdown,
        ] {
            let query = mant_protocol::SearchQuery {
                pattern: word.into(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope,
                word: true,
                context_lines: 0,
                limit: 100,
                offset: 0,
            };
            let search = mant_query::search_query(content, &query).unwrap();
            let wire = serde_json::to_string(&search).unwrap();
            let decoded: mant_protocol::QuerySearch = serde_json::from_str(&wire).unwrap();
            assert_eq!(decoded, search);
            let hits = decoded
                .matches
                .iter()
                .filter(|hit| hit.outline.node.id() == section.id.as_str());
            let occurrences = hits.flat_map(|hit| &hit.occurrences).collect::<Vec<_>>();
            assert_eq!(
                occurrences.len(),
                1,
                "{}: original table word {word}",
                case["id"]
            );
            let range = occurrences[0].markdown;
            let start: usize = range.start_byte.try_into().unwrap();
            let end: usize = range.end_byte.try_into().unwrap();
            assert_eq!(&artifact.text()[start..end], word);
            assert_eq!(occurrences[0].matched_text, word);
        }
    }
}

#[test]
fn source_columns_survive_actual_json_query_links_and_declared_reading_policy() {
    // Native gold above remains untouched. Reading omits the terminal's
    // right-margin soft wrapping, stacks an effective negative origin, and
    // composes the real parent with negative literal children. .ti's numeric
    // geometry is retired (only its preceding break executes in the codec).
    for case in cases() {
        let content = roundtrip(&case);
        assert_reading(&content, &case);
        assert_link_owners(&content, &case);
        assert_query(&content, &case);
    }
}

#[test]
fn cell_closed_graph_receipts_survive_source_wire_and_reading_consumers() {
    // All seventeen exact inputs ran five pristine profiles first. term_vspace
    // closes NOBREAK graph once (term.c:489-497), while pre_br/sp0 leave its
    // row open (roff_term.c:69-78). A later accepted word reopens the tail;
    // transparent ft/Tg do not. Empty source TEXT uses term_vspace and the
    // authored/rejected pass loop owns its endline (mdoc_term.c:358-376;
    // term.c:143-146,177-217). ASCII_NBRZW accepts graph without printing;
    // its executed endline still owns an empty data row (340-349,397).
    // Literal Bd uses the same live column device. Bl pre/post only call
    // term_newln (mdoc_term.c:1128-1154): an empty wrapper cannot reopen
    // a closed row, while a later actual accepted word can.
    for case in boundary_cases() {
        assert_ast(&case);
        for (name, format) in [("ascii", RenderFormat::Ascii), ("utf8", RenderFormat::Utf8)] {
            let output = Renderer::new(format)
                .with_width(78)
                .render_bytes("column-boundaries.1", text(&case["source"]).as_bytes())
                .unwrap();
            assert_eq!(
                native_body(&output.output, &case),
                native_body(text(&case["profiles"][name]["stdout"]), &case)
            );
        }
        let content = roundtrip(&case);
        let (_, rows) = table(&content);
        let [row] = rows else {
            panic!("one source It owner")
        };
        assert_eq!(row.cells.len(), 2, "{}: actual BODY owners", case["id"]);
        assert_eq!(
            row.cells[0].break_after,
            case["break_after"].as_bool().unwrap(),
            "{}: latest tail",
            case["id"]
        );
        assert_eq!(
            cli_rows(&content, &case),
            strings(&case["cli_rows"]),
            "{}: exact hard and empty rows",
            case["id"]
        );
        assert_query(&content, &case);
    }
}

#[test]
fn source_cell_metadata_keeps_closed_gap_and_signed_descendant_owners() {
    // Both exact sources reran pristine ASCII/UTF-8/HTML/tree/lint before
    // these owner assertions. Bd BODY's signed source offset survives to
    // accepted reading geometry (mdoc_term.c:1449-1455). Bl/It's zero-word
    // posts cannot reopen the separately completed term_vspace row
    // (1128-1154,939-953; term.c:475-497).
    let outdent = cases()
        .into_iter()
        .find(|case| case["id"] == "outdent-blank")
        .unwrap();
    let content = roundtrip(&outdent);
    let (_, rows) = table(&content);
    let cell = &rows[0].cells[0];
    let [
        Block::Preformatted {
            children,
            layout,
            inline_layout,
            ..
        },
    ] = cell.blocks.as_slice()
    else {
        panic!("one accepted literal owner: {cell:?}")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "A");
    assert_eq!(layout.indent_columns, -3);
    assert_eq!(inline_layout.row_hints, Vec::new());
    assert!(cell.break_after);
    assert_reading(&content, &outdent);
    assert_query(&content, &outdent);

    let wrapper = boundary_cases()
        .into_iter()
        .find(|case| case["id"] == "empty-anchored-list")
        .unwrap();
    let content = roundtrip(&wrapper);
    let (_, rows) = table(&content);
    let cell = &rows[0].cells[0];
    assert!(cell.break_after);
    assert!(matches!(
        cell.blocks.last(),
        Some(Block::VerticalSpace { lines: 1, .. })
    ));
    assert_eq!(cli_rows(&content, &wrapper), strings(&wrapper["cli_rows"]));
    assert_query(&content, &wrapper);
}

fn install_pure_ir_hints(content: &mut ResolvedContent) {
    let section = content
        .document
        .as_mut()
        .unwrap()
        .sections
        .iter_mut()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let Block::Table { rows, .. } = &mut section.blocks[0] else {
        panic!("table")
    };
    let Block::Paragraph { inline_layout, .. } = &mut rows[0].cells[0].blocks[0] else {
        panic!("paragraph")
    };
    inline_layout.row_hints = [(0, -2), (1, 3), (2, -1)]
        .into_iter()
        .map(|(row, indent_columns)| mant_ir::RowLayoutHint {
            row,
            indent_columns,
        })
        .collect();
}

#[test]
fn explicit_json_owner_hints_compose_with_parent_without_becoming_native_ti_geometry() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "hinted-hard-rows")
        .unwrap();
    let mut content = roundtrip(&case);
    install_pure_ir_hints(&mut content);
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(decoded, content);
    assert_eq!(
        cli_rows(&decoded, &case),
        ["   A", "        C", "    D", "     B"]
    );
    // Flattened tables use the frozen fenced-literal policy: a positive
    // correction becomes literal ASCII padding in the derived readback.
    // The original owner keeps its author scalars and signed geometry.
    // Ordinary Markdown isolates the fenced payload contract. Raw HTML
    // navigation markers have a separate import spelling; assert_query below
    // independently checks the actual addressable artifact and source ranges.
    let markdown = mant_codec::encode::render_markdown(&decoded);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let [Block::Preformatted { children, .. }] = description(&readback).blocks.as_slice() else {
        panic!(
            "actual fenced table readback: {:?}\n{markdown}",
            description(&readback).blocks
        )
    };
    assert_eq!(mant_ir::inline_plain_text(children), "A\n   C\nD | B");
    assert_eq!(cli_rows(&readback, &case), ["A", "   C", "D | B"]);
    assert_eq!(payload(&table(&decoded).1[0].cells[0].blocks), "A\nC\nD");
    assert_query(&decoded, &case);
}
