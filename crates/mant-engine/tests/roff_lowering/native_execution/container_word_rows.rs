//! Native word acceptance remains ordered across nested output containers.

use mant_ir::ResolvedContent;

fn body_rows(rendered: &str) -> Vec<String> {
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
    let end = rows.iter().position(|row| *row == "NEXT").unwrap();
    rows[start..end]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

fn assert_display_equation(
    query: &ResolvedContent,
    actual: &[String],
    expected: &[String],
    name: &str,
) {
    struct Equations(usize);
    impl<'ir> mant_ir::visit::Visit<'ir> for Equations {
        fn visit_block(&mut self, block: &'ir mant_ir::Block) {
            if let mant_ir::Block::Equation { value, display, .. } = block {
                assert!(*display);
                assert_eq!(value, "x");
                self.0 += 1;
            }
            mant_ir::visit::walk_block(self, block);
        }
    }
    // EQ/EN is a structured display in the frozen IR contract. term_eqn
    // instead writes words into its surrounding device buffer. Keep the
    // accepted prefix and owned equation independently of that layout.
    let display = actual.iter().position(|row| row == "x").unwrap();
    let preceding = actual[..display].join("\n");
    let reference = expected.join("\n");
    let prefix = reference
        .split_once(" x ")
        .map_or(reference.as_str(), |(prefix, _)| prefix);
    let glyphs = |value: &str| {
        value
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    };
    assert_eq!(glyphs(&preceding), glyphs(prefix), "{name}");
    let mut equations = Equations(0);
    mant_ir::visit::Visit::visit_document(&mut equations, query.document.as_ref().unwrap());
    assert_eq!(equations.0, 1, "{name}");
    assert_eq!(actual.iter().filter(|row| *row == "Following").count(), 1);
}

#[test]
fn nested_containers_and_alternate_operands_preserve_native_word_consumption() {
    // These 21 exact files ran pristine ASCII/UTF-8/HTML/tree/lint before
    // recording their expectations. mdoc_term.c::print_mdoc_node routes
    // TBL/EQN through the active termp and runs a crossed BODY post exactly
    // once at its actual source position. term.c::term_fill consumes the
    // remaining buffer, including an accepted prefix and a rejected suffix.
    // man_term.c::pre_alternate calls term_word for each BI/BR operand and
    // sets NOSPACE between them; these operands are not ordinary B children.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/container_word_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 4 * 3 + 3 * 3);
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
        let actual = body_rows(&mant_render::render_query_man(&query));
        if case["scope"] == "display-equation" {
            assert_display_equation(&query, &actual, &expected, name);
            continue;
        }
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
