//! Native buffer state is consumed at each actual formatter control boundary.

use mant_ir::ResolvedContent;

fn body_rows(rendered: &str, eof: bool) -> Vec<String> {
    let mut projected = String::with_capacity(rendered.len());
    for character in rendered.chars() {
        match character {
            '\u{8}' => {
                projected.pop();
            }
            '\u{a0}' => projected.push(' '),
            _ => projected.push(character),
        }
    }
    let rows = projected.lines().map(str::trim).collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = if eof {
        rows.len()
    } else {
        rows.iter().position(|row| *row == "NEXT").unwrap()
    };
    rows[start..end]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

#[test]
fn every_control_consumes_the_current_buffer_once_through_json() {
    // All exact sources ran the registered pristine reference before these
    // snapshots were recorded. term.c::term_word()/term_fill() distinguish
    // empty operands, NBRZW, BACKBEFORE and ESCAPE_BREAK. roff_term_pre_mc
    // flushes with NOBREAK; br/sp/nf/ti retire different native boundaries.
    // man_term.c::print_man_node() also applies its generic font lifecycle.
    // The 8 buffer states cross all 12 controls in both dialects (192),
    // plus 7 repeated/end boundaries crossed with 3 states (42). The target
    // control uses real Tg in mdoc and a zero-output ft request in man.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/native_control_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 234);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let json = mant_render::render_query_json(&query, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private owner leaked"
        );
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = decoded.into();
        let actual = body_rows(
            &mant_render::render_query_man(&query),
            case["eof"].as_bool().unwrap(),
        );
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
