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

#[test]
fn native_address_recovery_distinguishes_missing_glyphs_from_escape_errors() {
    // These suffixes came from the entity-decoded hrefs of the exact pinned
    // CVS inputs. print_encode() checks ESCAPE_ERROR before HTML_SKIPCHAR,
    // but checks unknown SPECIAL and out-of-range NUMBERED afterwards.
    for (operand, plain, skipped) in [
        (r"\[u0041]", "AY", "Y"),
        (r"\N'65'", "AY", "Y"),
        (r"\[u0041_0042]", "Y", "Y"),
        (r"\[u00041]", "Y", "Y"),
        (r"\[vc]", "Y", "Y"),
        (r"\[A ga]", "Y", "Y"),
        (r"\[DefinitelyUnknown]", "Y", "Y"),
        (r"\[B]", "Y", ""),
        (r"\[:]", "Y", ""),
        (r"\[uD800]", "Y", "Y"),
        (r"\C'B'", "Y", ""),
        (r"\!", "Y", "Y"),
        (r"\&", "Y", "Y"),
        (r"\fI", "Y", ""),
        (r"\f[unknown]", "Y", ""),
        (r"\N'999'", "Y", "Y"),
        (r"\N'-1'", "1'Y", "1'Y"),
    ] {
        for (skip, suffix) in [("", plain), (r"\z", skipped)] {
            for label in ["", " LABEL"] {
                let source = format!("{HEADER}.Lk \"https://e/{skip}{operand}Y\"{label}\n");
                assert_exported_address(&source, &format!("https://e/{suffix}"));
            }
        }
    }
}

#[test]
fn native_address_recovery_is_shared_by_web_mail_and_reference_operands() {
    let man_header = ".TH PROBE 1\n.SH DESCRIPTION\n";
    for (escape, suffix) in [
        (r"\[u0041_0042]", "Y"),
        (r"\z\[B]", ""),
        (r"\z\[vc]", "Y"),
        (r"\[u0041]", "AY"),
    ] {
        for source in [
            format!("{man_header}.UR \"https://e/{escape}Y\"\nLABEL\n.UE\n"),
            format!("{HEADER}.Rs\n.%T LABEL\n.%U \"https://e/{escape}Y\"\n.Re\n"),
        ] {
            assert_exported_address(&source, &format!("https://e/{suffix}"));
        }
        let source = format!("{HEADER}.Mt \"user{escape}Y@example.test\"\n");
        let address = format!("user{suffix}@example.test");
        assert_exported_address(&source, &address);
        let markdown = mant_codec::encode::render_markdown(&roundtrip(&source));
        assert!(
            markdown.contains(&format!("mailto:{address}")),
            "{markdown}"
        );
    }
}

#[test]
fn literal_angle_brackets_survive_address_decoding_without_enabling_activation() {
    const MDOC: &str = concat!(
        ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd exercise descriptors\n.Sh DESCRIPTION\n",
    );
    const MAN: &str = ".TH PROBE 1 \"November 18, 2026\"\n.SH NAME\nprobe \\- exercise descriptors\n.SH DESCRIPTION\n";
    // Fixed CVS html.c::print_encode consumes the percent escape, then
    // independently escapes the two authored angle brackets in the href.
    for (operand, decoded) in [
        ("<>", "<>"),
        (r"\%<>", "<>"),
        (r"\z\%<>", "<>"),
        (r"\%<", "<"),
        (r"\%>", ">"),
        (r"\%<>\%<>", "<><>"),
        ("%3C%3E", "%3C%3E"),
    ] {
        let uri = format!("https://e/{operand}Y");
        let address = format!("x{operand}Y@example.test");
        for (source, email) in [
            (format!("{MDOC}.Lk {uri} LABEL\n"), false),
            (format!("{MAN}.UR {uri}\nLABEL\n.UE\n"), false),
            (format!("{MDOC}.Rs\n.%U {uri}\n.Re\n"), false),
            (format!("{MDOC}.Mt {address}\n"), true),
            (format!("{MAN}.MT {address}\nLABEL\n.ME\n"), true),
        ] {
            let (target, expected, code) = if email {
                let expected = format!("x{decoded}Y@example.test");
                let target = LinkTarget::Email {
                    address: expected.clone(),
                };
                (target, expected, "ir.invalid-email-address")
            } else {
                let expected = format!("https://e/{decoded}Y");
                let target = LinkTarget::External {
                    uri: expected.clone(),
                };
                (target, expected, "ir.invalid-external-uri")
            };
            let query = roundtrip(&source);
            assert_eq!(
                link_targets(&query),
                std::slice::from_ref(&expected),
                "{source}"
            );
            let valid = decoded == "%3C%3E";
            assert_eq!(target.to_uri().is_some(), valid, "{source}");
            let document = query.document.as_ref().unwrap();
            assert_eq!(
                document
                    .diagnostics
                    .iter()
                    .filter(|d| d.code.as_deref() == Some(code))
                    .count(),
                usize::from(!valid),
                "{source}",
            );
            let markdown = mant_codec::encode::render_markdown(&query);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let exported = link_targets(&readback);
            if valid {
                assert!(exported.contains(&expected), "{source}: {markdown}");
            } else {
                assert!(exported.is_empty(), "{source}: {markdown}");
            }
            if source.contains("LABEL") {
                assert_eq!(search(&query, "LABEL").matches.len(), 1, "{source}");
            }
        }
    }
}

fn assert_exported_address(source: &str, expected: &str) {
    let query = roundtrip(source);
    assert_eq!(link_targets(&query), [expected], "{source}");
    let markdown = mant_codec::encode::render_markdown(&query);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    assert_eq!(link_targets(&readback), [expected], "{source}: {markdown}");
    if source.contains("LABEL") {
        assert_eq!(search(&query, "LABEL").matches.len(), 1, "{source}");
    }
}
