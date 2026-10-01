//! Portable spelling changes never reexecute or revoke native word receipts.

use mant_ir::{
    Block, Inline, ResolvedContent,
    visit::{self, Visit, VisitMut},
};

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

struct ReplaceDisplay<'a> {
    spelling: &'a str,
    count: usize,
    prose_count: usize,
    literal_depth: usize,
}

impl VisitMut for ReplaceDisplay<'_> {
    fn visit_block_mut(&mut self, block: &mut Block) {
        let literal = matches!(block, Block::Preformatted { .. });
        self.literal_depth += usize::from(literal);
        visit::walk_block_mut(self, block);
        self.literal_depth -= usize::from(literal);
    }

    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::PortableDisplay { display, children } = inline {
            self.spelling.clone_into(display);
            self.count += 1;
            // encode/blocks.rs exports fenced literal text from native
            // children. portable.rs replaces prose only when the accepted
            // interval contains a glyph; empty receipts cannot add prose.
            self.prose_count += usize::from(
                self.literal_depth == 0
                    && mant_ir::inline_plain_text(children)
                        .chars()
                        .any(|character| !character.is_whitespace()),
            );
        }
        visit::walk_inline_mut(self, inline);
    }
}

#[derive(Default)]
struct VisibleText(String);

impl<'ir> Visit<'ir> for VisibleText {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Text { value } | Inline::Code { value } = inline {
            self.0.push_str(value);
        }
        visit::walk_inline(self, inline);
    }
}

fn assert_portable_export(
    name: &str,
    query: &ResolvedContent,
    variant: &ResolvedContent,
    prose_count: usize,
) {
    let markdown = mant_codec::encode::render_markdown(variant);
    assert!(!markdown.contains('\0'), "{name}: private owner leaked");
    if prose_count == 0 {
        assert_eq!(
            markdown,
            mant_codec::encode::render_markdown(query),
            "{name}: display-only mutation changed native literal export"
        );
    }
    let reparsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let mut text = VisibleText::default();
    text.visit_document(reparsed.document.as_ref().unwrap());
    assert_eq!(
        text.0.matches("PORTABLE").count(),
        prose_count,
        "{name}: display transaction dropped or duplicated its portable spelling"
    );
}

#[test]
fn portable_transactions_keep_all_native_operand_and_owner_boundaries() {
    // Every exact source ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // before these assertions (108 clean lint sources). mdoc_validate.c
    // post_bx inserts Ns/BSD and the version-release join; termp_xx_pre/post
    // controls KEEP. Authored p/c/z/font words execute before generated BSD.
    // Native hard-row gold also runs each unchanged source at width 1000;
    // default width-78 rows remain recorded but include device soft wraps.
    // Native gold is pristine. The separate approved PortableDisplay contract
    // substitutes spelling, including shorter/longer/space-containing text,
    // while the original receipt children continue to own native execution.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/portable_word_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 9 * 4 * 3);
    let mut failures = Vec::new();
    let mut replaced = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let expected = case["wide_rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let actual = body_rows(&mant_render::render_query_man(&query));
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
        for spelling in [
            "PORTABLE",
            "A MUCH LONGER PORTABLE SPELLING",
            "PORTABLE WORDS",
        ] {
            let mut variant = query.clone();
            let mut replacement = ReplaceDisplay {
                spelling,
                count: 0,
                prose_count: 0,
                literal_depth: 0,
            };
            replacement.visit_document_mut(variant.document.as_mut().unwrap());
            replaced += replacement.count;
            if replacement.count == 0 {
                continue;
            }
            assert_eq!(
                body_rows(&mant_render::render_query_man(&variant)),
                actual,
                "{name}: display mutation changed native receipts"
            );
            assert_portable_export(name, &query, &variant, replacement.prose_count);
        }
    }
    assert!(replaced > 0, "portable mutation axis was not exercised");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
