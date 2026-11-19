use super::{OwnedLabels, Visit, word_hits};

#[derive(serde::Deserialize)]
struct Case {
    source: String,
    #[serde(flatten)]
    expected: Expected,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Expected {
    Words {
        rows: Vec<Vec<String>>,
        old_link: Option<Vec<String>>,
    },
    Combining {
        suffix: String,
    },
}

#[test]
fn native_passes_preserve_word_boundaries_and_surviving_glyph_owners() {
    // The two pinned terminal profiles agree on every word and line here.
    // term_field() skips NBRZW and breakpoint controls during output,
    // then prints retained device blanks only before subsequent glyphs.
    // The UTF-8 combining cases also all match the raw undrawn control.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("word_projection_oracle.json")).unwrap();
    for Case { source, expected } in cases {
        let decoded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&decoded);
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        if let Expected::Words {
            old_link: Some(want),
            ..
        } = &expected
        {
            let mut links = OwnedLabels::default();
            links.visit_section(&content.document.as_ref().unwrap().sections[1]);
            let actual: Vec<_> = links
                .0
                .into_iter()
                .filter_map(|(uri, label)| (uri == "https://old.test").then_some(label))
                .collect();
            assert_eq!(actual, *want, "{source}");
        }
        for document in [&content, &readback] {
            let text = mant_render::render_query_text(document);
            match &expected {
                Expected::Words { rows, .. } => {
                    assert_eq!(observed_rows(&text), *rows, "{source}\n{text}");
                    for word in ["LEFTYTAIL", "LEFTXYTAIL", "LEFT", "LEFTX", "YTAIL"] {
                        let want = rows.iter().flatten().filter(|found| *found == word).count();
                        assert_eq!(word_hits(document, word), want, "{word}: {source}\n{text}");
                    }
                }
                Expected::Combining { suffix } => {
                    let first = text
                        .split_once("LEFT")
                        .unwrap()
                        .1
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap_or_default();
                    assert_eq!(first, suffix, "{source}\n{text}");
                }
            }
        }
    }
}

fn observed_rows(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .map(|line| {
            line.split(|character: char| !character.is_ascii_uppercase())
                .filter(|word| word.starts_with("LEFT") || word.ends_with("YTAIL"))
                .collect::<Vec<_>>()
        })
        .filter(|row| !row.is_empty())
        .collect()
}
