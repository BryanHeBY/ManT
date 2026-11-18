//! Drawn device cells can admit a pass without supplying a reading glyph.

use mant_ir::{Block, Inline, ResolvedContent, inline_plain_text};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax, default_search_limit};

const HEADER: &str =
    ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- fixture\n.SH DESCRIPTION\n";
const END: &str = "\n.PP\nENDTEXT\n";
const OWNERS: [&str; 7] = ["paragraph", "nofill", "ip", "tp", "heading", "table", "ur"];

fn source(owner: &str, word: &str) -> String {
    source_with_header(HEADER, owner, word)
}

fn source_with_header(header: &str, owner: &str, word: &str) -> String {
    let body = match owner {
        "paragraph" => format!("{word}\nTAILWORD"),
        "nofill" => format!(".nf\n{word}\nTAILWORD\n.fi"),
        "ip" => format!(".IP \"{word}\" 18\nTAILWORD"),
        "tp" => format!(".TP 18\n.B \"{word}\"\nTAILWORD"),
        "heading" => format!(".SH \"{word}\"\nTAILWORD"),
        "table" => format!(".TS\nl.\n{word}\nTAILWORD\n.TE"),
        "ur" => format!(".UR https://example.test/r\n{word}\nTAILWORD\n.UE"),
        _ => unreachable!(),
    };
    format!("{header}{body}{END}")
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlotOracle {
    prefixes: Vec<String>,
    strokes: Vec<String>,
    motions: Vec<String>,
    paragraph: Vec<Vec<Option<String>>>,
    owners: Vec<String>,
    words: Vec<String>,
    by_owner: Vec<Vec<Option<String>>>,
}

const SLOT_HEADER: &str =
    ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- example\n.SH DESCRIPTION\n";

fn markers(text: &str) -> Vec<&'static str> {
    let mut words: Vec<_> = ["NEEDLE", "TAILWORD", "ENDTEXT"]
        .into_iter()
        .flat_map(|word| {
            text.match_indices(word)
                .map(move |(index, _)| (index, word))
        })
        .collect();
    words.sort_unstable_by_key(|(index, _)| *index);
    words.into_iter().map(|(_, word)| word).collect()
}

fn slot_oracle() -> SlotOracle {
    // The matrix preserves both terminal devices' visible marker order.
    // Every exact source ran fixed CVS lint, ASCII and UTF-8 first.
    // A null position records device disagreement.
    serde_json::from_str(include_str!("drawing_word_rows/slot_oracle.json")).unwrap()
}

#[test]
fn chained_slot_operations_retain_exact_marker_order_in_paragraphs() {
    let matrix = slot_oracle();
    let mut bitmap = matrix.paragraph.iter();
    let mut reviewed = 0;
    let mut passed = 0;
    for prefix in &matrix.prefixes {
        for stroke in &matrix.strokes {
            for (motion, expected) in matrix.motions.iter().zip(bitmap.next().unwrap()) {
                let Some(expected) = expected else {
                    reviewed += 1;
                    continue;
                };
                let word = format!(r"{prefix}{stroke}{motion}\p NEEDLE");
                let source = source_with_header(SLOT_HEADER, "paragraph", &word);
                let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                let actual = markers(&mant_render::render_query_text(&query));
                assert!(
                    actual.iter().copied().eq(expected.split_whitespace()),
                    "{source}\n{actual:?}"
                );
                passed += 1;
            }
        }
    }
    assert!(bitmap.next().is_none());
    assert_eq!((passed, reviewed), (740, 44));
}

#[test]
fn overwritten_markers_and_blank_overstrikes_follow_the_exact_source_owner() {
    let matrix = slot_oracle();
    let mut passed = 0;
    let mut reviewed = 0;
    for (owner, expected) in matrix.owners.iter().zip(matrix.by_owner) {
        for (word, outcome) in matrix.words.iter().zip(expected) {
            let Some(outcome) = outcome else {
                reviewed += 1;
                continue;
            };
            let word = format!(r"{word}\p NEEDLE");
            let source = source_with_header(SLOT_HEADER, owner, &word);
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let restored = crate::json_roundtrip::json_string_roundtrip(&query);
            let actual = markers(&mant_render::render_query_text(&restored));
            let expected: Vec<_> = outcome.split_whitespace().collect();
            assert_eq!(actual, expected, "{source}");
            let markdown = mant_codec::encode::render_markdown(&restored);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let mut exported = markers(&mant_render::render_query_text(&readback));
            exported.sort_unstable();
            exported.dedup();
            let mut emitted = expected.clone();
            emitted.sort_unstable();
            emitted.dedup();
            assert_eq!(exported, emitted, "{source}\n{markdown}");
            let count = expected.iter().filter(|&&word| word == "NEEDLE").count();
            if count == 1 {
                assert!(needle_in_original_owner(&restored, owner), "{source}");
            }
            assert_eq!(word_hits(&restored, "NEEDLE"), count, "{source}");
            passed += 1;
        }
    }
    assert_eq!((passed, reviewed), (58, 5));
}

fn word_hits(query: &ResolvedContent, word: &str) -> usize {
    mant_query::search_query(
        query,
        &SearchQuery {
            pattern: word.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: true,
            context_lines: 0,
            limit: default_search_limit(),
            offset: 0,
        },
    )
    .unwrap()
    .matches
    .len()
}

fn has_link_label(nodes: &[Inline], label: &str) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Link {
            children, target, ..
        } => {
            inline_plain_text(children).contains(label)
                && matches!(target, mant_ir::LinkTarget::External { uri } if uri == "https://example.test/r")
        }
        Inline::Strong { children } | Inline::Emphasis { children } => has_link_label(children, label),
        _ => false,
    })
}

fn needle_in_original_owner(content: &ResolvedContent, owner: &str) -> bool {
    let doc = content.document.as_ref().unwrap();
    let section = &doc.sections[if owner == "heading" { 2 } else { 1 }];
    let includes = |nodes: &[Inline]| inline_plain_text(nodes).contains("NEEDLE");
    if owner == "heading" {
        return includes(&section.heading.content);
    }
    match (&section.blocks[0], owner) {
        (Block::Paragraph { children, .. }, "paragraph")
        | (Block::Preformatted { children, .. }, "nofill") => includes(children),
        (Block::Paragraph { children, .. }, "ur") => has_link_label(children, "NEEDLE"),
        (Block::DefinitionList { items, .. }, "ip" | "tp") => includes(&items[0].terms[0]),
        (Block::Table { rows, .. }, "table") => {
            let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
                return false;
            };
            includes(children)
        }
        _ => false,
    }
}

#[test]
fn strokes_admit_the_following_author_words_in_each_native_owner() {
    // All 168 inputs ran pinned CVS lint, tree, HTML, ASCII, and UTF-8.
    // term.c:720-774 writes a stroke before term_fill's graph check
    // (294-306,362-366). Plain space uses the breakable branch. An invalid
    // operand writes nothing. Zero length uses the active field width.
    for owner in OWNERS {
        for prefix in ["", r"START\p "] {
            for (operation, accepts) in [
                (r"\l'2n.'", true),
                (r"\l'2n'", true),
                (r"\l'0n.'", true),
                (r"\l'2n '", false),
                (r"X\l'2n '", false),
                (r"\l'invalid.'", false),
                (r"\z\l'2n.'", true),
                (r"\zX\l'2n.'", true),
                (r"\h'2n'", true),
                (r"\o'ab'", true),
                (r"\&", true),
                ("Z", true),
            ] {
                let source = source(owner, &format!(r"{prefix}{operation}\p NEEDLE"));
                let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
                let text = mant_render::render_query_text(&content);
                let tail = accepts || !matches!(owner, "paragraph" | "ur");
                let expected = [("NEEDLE", accepts), ("TAILWORD", tail), ("ENDTEXT", true)];
                let mut previous = 0;
                let markdown = mant_codec::encode::render_markdown(&content);
                let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let readback = mant_render::render_query_text(&readback);
                for (word, present) in expected {
                    assert_eq!(
                        text.matches(word).count(),
                        usize::from(present),
                        "{source}\n{text}"
                    );
                    assert_eq!(readback.contains(word), present, "{source}\n{markdown}");
                    if let Some(position) = text.find(word) {
                        assert!(position >= previous, "{source}\n{text}");
                        previous = position;
                    }
                }
                if accepts {
                    assert!(
                        needle_in_original_owner(&content, owner),
                        "{source}\n{text}"
                    );
                }
                let found = mant_query::search_query(
                    &content,
                    &SearchQuery {
                        pattern: "NEEDLE".into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: default_search_limit(),
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(found.matches.len(), usize::from(accepts), "{source}");
            }
        }
    }
}

#[test]
fn space_strokes_obey_the_native_retreat_and_cursor_before_word_breaks() {
    // Fixed CVS encode1() preserves the preceding graph when a drawn SP
    // advances over it. term_word()'s following HORIZ can move back into
    // those generated slots before it writes the pending break marker.
    for (word, admits) in [
        (r"\l'2n '\h'-1n'", false),
        (r"\zX\l'2n '", false),
        (r"\zX\l'2n '\h'-1n'", false),
        (r"\zX\l'2n '\h'|1n'", true),
        (r"\zX\z\l'2n '", false),
        (r"\zX\z\l'2n '\h'-1n'", true),
        (r"\zX\z\l'2n '\h'|1n'", true),
    ] {
        let source = format!(
            ".TH PROBE 1 \"November 19, 2026\"\n.SH DESCRIPTION\n{word}\\p NEEDLE\nTAILWORD\n.PP\nENDTEXT\n"
        );
        let raw = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&raw);
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for parsed in [&content, &readback] {
            let rendered = mant_render::render_query_text(parsed);
            let words: Vec<_> = rendered
                .lines()
                .map(str::trim)
                .filter(|line| {
                    ["NEEDLE", "TAILWORD", "ENDTEXT"]
                        .iter()
                        .any(|word| line.contains(word))
                })
                .collect();
            let expected: &[&str] = if admits {
                &["NEEDLE TAILWORD", "ENDTEXT"]
            } else {
                &["ENDTEXT"]
            };
            assert_eq!(words, expected, "{source}\n{rendered}");
        }
    }
}

#[test]
fn an_absolute_move_beyond_drawn_spaces_supplies_a_fixed_blank_before_the_break() {
    // The exact sources ran all five fixed CVS profiles. At or inside the
    // drawn cursor term_word() writes no new graph. A positive delta writes
    // ASCII_NBRSP, which term_fill() accepts before the following marker.
    for owner in OWNERS {
        for column in [1, 2, 3, 5] {
            let source = source(owner, &format!(r"\l'2n '\h'|{column}n'\p NEEDLE"));
            let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let markdown = mant_codec::encode::render_markdown(&content);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            for parsed in [&content, &readback] {
                let text = mant_render::render_query_text(parsed);
                assert_eq!(text.contains("NEEDLE"), column > 2, "{source}\n{text}");
                assert!(text.contains("ENDTEXT"), "{source}\n{text}");
            }
            if column > 2 {
                assert!(needle_in_original_owner(&content, owner), "{source}");
            }
        }
    }
}
