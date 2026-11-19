use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, inline_plain_text};

use super::{MDOC, word_hits};

const END: &str = "\n.Sh AFTER\nEnd.\n";

#[test]
fn repeated_writes_retain_the_exact_earlier_glyph_and_link_range() {
    // CVS term.c::bufferc()/encode1() keep an unoverwritten suffix even
    // after a later word retreats into an earlier one. For each linked
    // case, the fixed ASCII and UTF-8 devices first ran with M/A tracers
    // to identify the surviving slots and then with two equal Q glyphs.
    let cases: Vec<(String, String, String)> =
        serde_json::from_str(include_str!("related_slot_owners.json")).unwrap();
    assert_eq!(cases.len(), 432);
    for (words, visible, links) in cases {
        let source = format!("{MDOC}{words}{END}");
        let raw = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&raw);
        let markdown = mant_codec::encode::render_markdown(&content);
        let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for projection in [&content, &restored] {
            let [Block::Paragraph { children, .. }] =
                projection.document.as_ref().unwrap().sections[1]
                    .blocks
                    .as_slice()
            else {
                panic!("{source}");
            };
            let mut actual = String::new();
            collect_link_slots(children, '-', &mut actual);
            assert_eq!(inline_plain_text(children), visible, "{source}");
            assert_eq!(actual, links, "{source}\n{markdown}");
        }
    }
}

fn collect_link_slots(nodes: &[Inline], owner: char, out: &mut String) {
    for node in nodes {
        match node {
            Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } => {
                let label = match uri.as_str() {
                    "https://one.test" => '1',
                    "https://two.test" => '2',
                    _ => panic!("unexpected link {uri}"),
                };
                collect_link_slots(children, label, out);
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect_link_slots(children, owner, out);
            }
            Inline::Anchor { .. } => {}
            Inline::Text { value } | Inline::Code { value } => {
                out.extend(std::iter::repeat_n(owner, value.chars().count()));
            }
            other => panic!("unexpected inline {other:?}"),
        }
    }
}

#[test]
fn repeated_writes_can_replace_the_middle_or_prefix_of_a_live_word() {
    for (source, expected) in [
        (r#".No PMA No "\h'-4n'T" Ns No Y"#, "TYA"),
        (r#".No PMA No "\h'-2n'T" No "\h'-3n'Y""#, "PYT"),
        (r#".No PMA No "\h'-4n'TZ" Ns No "\h'-2n'Y""#, "YZA"),
    ] {
        let content =
            mant_loader::load_roff_bytes(format!("{MDOC}{source}{END}").as_bytes()).unwrap();
        assert_eq!(body(&content), expected, "{source}");
        assert_eq!(word_hits(&content, expected), 1, "{source}");
    }
}

#[test]
fn a_reused_separator_only_prints_when_the_later_graph_admits_it() {
    // term_field() defers spaces until a graph prints them. An ordinary
    // incoming word can advance over an existing blank after C's backspace.
    for (strike, retained) in [("CD", "PC"), ("XY", "PX")] {
        for host in ["paragraph", "literal", "D1"] {
            for (tail, expected) in [
                ("", retained.to_owned()),
                (" No Y", "P Y".into()),
                (" Ns No Y", "PY".into()),
                (
                    "\n.No Y",
                    if host == "paragraph" {
                        "P Y".into()
                    } else {
                        format!("{retained}\nY")
                    },
                ),
            ] {
                let word = format!(r#".No "P\o'{strike}'" No "\h'-2n'\&"{tail}"#);
                let source = match host {
                    "literal" => format!(".Bd -literal -compact\n{word}\n.Ed"),
                    "D1" => format!(".D1 {}", word.trim_start_matches('.')),
                    _ => word,
                };
                let raw = mant_loader::load_roff_bytes(format!("{MDOC}{source}{END}").as_bytes())
                    .unwrap();
                let content = crate::json_roundtrip::json_string_roundtrip(&raw);
                let markdown = mant_codec::encode::render_markdown(&content);
                let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
                for projection in [&content, &restored] {
                    assert_eq!(body(projection), expected, "{source}\n{markdown}");
                    assert_eq!(
                        word_hits(projection, "P Y"),
                        usize::from(expected == "P Y"),
                        "{source}"
                    );
                }
            }
        }
    }
}

fn body(content: &ResolvedContent) -> String {
    let rendered = mant_render::render_query_text(content);
    rendered
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\nAFTER\n")
        .unwrap()
        .0
        .trim()
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}
