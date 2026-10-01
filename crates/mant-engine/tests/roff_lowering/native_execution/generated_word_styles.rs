//! Pending glyphs keep their earlier font and semantic owner at generated words.

use mant_ir::{
    Inline, ResolvedContent,
    visit::{self, Visit},
};

#[derive(Default)]
struct AcceptedGlyphs {
    font: u8,
    link_depth: usize,
    styles: Vec<u8>,
}

impl<'ir> Visit<'ir> for AcceptedGlyphs {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        let previous_font = self.font;
        match inline {
            Inline::Strong { .. } => self.font |= 1,
            Inline::Emphasis { .. } => self.font |= 2,
            Inline::Link { .. } => self.link_depth += 1,
            Inline::Text { value } | Inline::Code { value } => {
                for _ in value.chars().filter(|character| *character == 'A') {
                    assert_eq!(
                        self.link_depth, 0,
                        "earlier pending A moved into the next link"
                    );
                    self.styles.push(self.font);
                }
            }
            _ => {}
        }
        visit::walk_inline(self, inline);
        if matches!(inline, Inline::Link { .. }) {
            self.link_depth -= 1;
        }
        self.font = previous_font;
    }
}

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

#[test]
fn mail_and_link_words_keep_the_accepted_pending_glyphs_font_and_owner() {
    // All 48 exact sources passed pristine lint and ran ASCII/UTF-8/HTML/
    // tree first. term.c::encode1 buffers the font-marked A before BACKBEFORE;
    // generated Mt/Lk writes share held-cell/backtracking order. CVS device
    // overstrikes record A's real bold/underline bits independently of the
    // later link's font. Accepted prior glyphs never enter that Link owner.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/generated_word_styles/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 2 * 4 * 2 * 3);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let expected_rows = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let expected_styles = case["accepted_a_styles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bits| u8::try_from(bits.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let document = query.document.as_ref().unwrap();
        let section = document
            .sections
            .iter()
            .find(|section| section.id.as_str() == "description")
            .unwrap();
        let mut glyphs = AcceptedGlyphs::default();
        glyphs.visit_section(section);
        let actual_rows = body_rows(&mant_render::render_query_man(&query));
        if actual_rows != expected_rows || glyphs.styles != expected_styles {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual rows: {actual_rows:?}\nreference rows: {expected_rows:?}\nactual A fonts: {:?}\nreference A fonts: {expected_styles:?}", glyphs.styles
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
