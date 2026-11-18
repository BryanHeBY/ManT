//! Exact href codepoints from the pinned CVS HTML renderer.

use super::{link_targets, roundtrip, search};
use mant_ir::LinkTarget;

const HEADER: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n",
    ".Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n",
);

#[test]
fn native_address_controls_follow_html_decoding_independently_of_the_label() {
    // The exact 36 inputs ran through fixed CVS. Values below come from
    // entity-decoded HTML hrefs, not from the codec observation helper.
    // html.c::print_encode() preserves tab and converts C0/C1 codepoints
    // outside ASCII printables to U+FFFD after both UNICODE and NUMBERED.
    for (point, expected) in [
        (1, '�'),
        (27, '�'),
        (127, '�'),
        (136, '�'),
        (65, 'A'),
        (9, '\t'),
        (0, '�'),
        (31, '�'),
        (159, '�'),
    ] {
        for operand in [format!(r"\[u{point:04X}]"), format!(r"\N'{point}'")] {
            for label in ["", " LABEL"] {
                let address = format!("https://a/{operand}Y");
                let expected = format!("https://a/{expected}Y");
                let source = format!("{HEADER}.Lk {address}{label}\n");
                let query = roundtrip(&source);
                assert_eq!(
                    link_targets(&query),
                    std::slice::from_ref(&expected),
                    "{source}"
                );
                assert_eq!(
                    mant_codec::observe_roff_link_operand(&address).as_deref(),
                    Some(expected.as_str()),
                    "{source}"
                );
                let valid = point == 65;
                let target = LinkTarget::External {
                    uri: expected.clone(),
                };
                assert_eq!(
                    target.to_uri().as_deref(),
                    valid.then_some(expected.as_str()),
                    "{source}"
                );
                let document = query.document.as_ref().unwrap();
                assert_eq!(
                    document
                        .diagnostics
                        .iter()
                        .filter(|diagnostic| {
                            diagnostic.code.as_deref() == Some("ir.invalid-external-uri")
                        })
                        .count(),
                    usize::from(!valid),
                    "{source}"
                );
                let terminal = mant_render::render_query_man(&query);
                let markdown = mant_codec::encode::render_markdown(&query);
                for output in [&terminal, &markdown] {
                    assert!(
                        output
                            .chars()
                            .all(|ch| !ch.is_control() || matches!(ch, '\n' | '\t')),
                        "{source}: {output:?}"
                    );
                }
                let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                if valid {
                    assert_eq!(link_targets(&readback), [expected], "{source}: {markdown}");
                } else {
                    assert!(link_targets(&readback).is_empty(), "{source}: {markdown}");
                }
                if !label.is_empty() {
                    for content in [&query, &readback] {
                        assert_eq!(search(content, "LABEL").matches.len(), 1, "{source}");
                    }
                }
            }
        }
    }
}
