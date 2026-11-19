use mant_loader::load_markdown_text;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};
use mant_render::render_query_text;

const PREFIX: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\nSTARTPROBE\n.br\n";
const SUFFIX: &str = "\n.br\nSTOPPROBE\n.Sh END\nDone.\n";

#[test]
fn generated_os_keep_preserves_the_right_word_until_its_scope_ends() {
    // mdoc_term.c::termp_xx_pre/post installs a scoped PREKEEP. For these
    // exact fixed ASCII/UTF-8 cases, the ordinary word space is removed by
    // the repeated breakpoints, while the generated keep space survives.
    for (mac, name) in [
        ("Bsx", "BSD/OS"),
        ("Dx", "DragonFly"),
        ("Fx", "FreeBSD"),
        ("Nx", "NetBSD"),
        ("Ox", "OpenBSD"),
        ("Ux", "Unix"),
    ] {
        for (opening, closing) in [
            (".", ""),
            (".Bd -literal -compact\n.", "\n.Ed"),
            (".D1 ", ""),
            (".Dl ", ""),
        ] {
            for motion in [r"\h'|0n'", r"\h'-30n'"] {
                // Ux takes no argument. Its X is a sibling and sees ordinary
                // spacing because the implicit scope has already ended.
                for (words, survives) in [
                    (format!("{mac} X"), mac != "Ux"),
                    (format!("No {name} X"), false),
                ] {
                    let breaks = r"\p".repeat(name.len());
                    let source = format!(
                        "{PREFIX}{opening}{words} Ns No \"{motion}{breaks}\"{closing}{SUFFIX}"
                    );
                    let document = super::roundtrip(&source);
                    let markdown = mant_codec::encode::render_markdown(&document);
                    let readback = load_markdown_text(&markdown, None).unwrap();
                    for content in [&document, &readback] {
                        let text = render_query_text(content);
                        let body = text
                            .split_once("STARTPROBE")
                            .unwrap()
                            .1
                            .split_once("STOPPROBE")
                            .unwrap()
                            .0;
                        assert_eq!(body.trim(), if survives { "X" } else { "" }, "{source}");
                    }
                    let search = mant_query::search_query(
                        &document,
                        &SearchQuery {
                            pattern: "X".into(),
                            syntax: SearchSyntax::Literal,
                            case: SearchCase::Sensitive,
                            scope: SearchScope::Visible,
                            word: true,
                            context_lines: 0,
                            limit: 16,
                            offset: 0,
                        },
                    )
                    .unwrap();
                    assert_eq!(search.total, u32::from(survives), "{source}");
                }
            }
        }
    }
}
