#![cfg(feature = "render")]

use libmandoc_rs::{RenderFormat, Renderer};

fn render(source: &[u8], fragment: bool) -> String {
    Renderer::new(RenderFormat::Html)
        .with_html_fragment(fragment)
        .render_bytes("scope.1", source)
        .unwrap()
        .output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn spacing_inside_a_link_does_not_unwind_the_section() {
    // Exact pristine CVS reproducer. HTML faults while terminal and tree
    // finish the two lines. roff_html_pre_sp closes the anchor before B.
    let source =
        b".TH PROBE 1 \"2026-11-07\"\n.SH DESCRIPTION\n.UR https://example.org\nA\n.sp 0\nB\n.UE\n";
    let html = render(source, true);
    assert!(
        html.contains(concat!(
            "<p class=\"Pp\"><a class=\"Lk\" href=\"https://example.org\">A</a></p> ",
            "<p class=\"Pp\">B</p> </section> </main>"
        )),
        "{html}"
    );
}

#[test]
fn a_closed_link_scope_keeps_its_surrounding_sections() {
    // Pristine CVS HTML crashes for the spacing/paragraph cases after
    // html_close_paragraph() closes the held anchor and its paragraph.
    // Its tree and terminal output establish the text and section order.
    // The successful EX/EE controls establish the HTML link cutoff.
    for (open, close, class, destination, href) in [
        (
            "UR",
            "UE",
            "Lk",
            "https://example.org",
            "https://example.org",
        ),
        ("MT", "ME", "Mt", "me@example.org", "mailto:me@example.org"),
    ] {
        for boundary in [".sp 0", ".PP", ".EX\nINSIDE\n.EE"] {
            let source = format!(
                ".TH SCOPE 1 \"2026-11-07\"\n.SH DESCRIPTION\n.{open} {destination}\nLINKLABEL\n{boundary}\nAFTERBREAK\n.{close}\nAFTERLINK\n.SH NEXT\nNEXTBODY\n"
            );
            for fragment in [true, false] {
                let html = render(source.as_bytes(), fragment);
                let link = format!(
                    "<p class=\"Pp\"><a class=\"{class}\" href=\"{href}\">LINKLABEL</a></p>"
                );
                let sections = html
                    .split("<section class=\"Sh\">")
                    .skip(1)
                    .collect::<Vec<_>>();
                assert_eq!(sections.len(), 2, "{open} {boundary}: {html}");
                let (description, after_description) =
                    sections[0].split_once("</section>").unwrap();
                let (next, after_next) = sections[1].split_once("</section>").unwrap();
                assert!(after_description.trim().is_empty(), "{html}");
                assert!(after_next.trim_start().starts_with("</main>"), "{html}");
                assert_eq!(
                    html.matches(&format!("href=\"{href}\"")).count(),
                    1,
                    "{html}"
                );
                assert!(description.contains(&link), "{html}");
                assert!(
                    description.contains("<p class=\"Pp\">AFTERBREAK AFTERLINK</p>"),
                    "{html}"
                );
                assert!(next.contains("<p class=\"Pp\">NEXTBODY</p>"), "{html}");
                if boundary.starts_with(".EX") {
                    assert!(description.contains("<pre>INSIDE </pre>"), "{html}");
                }
            }
        }
    }
}

#[test]
fn a_filled_list_head_preserves_the_list_opened_after_a_literal() {
    // The exact four inputs fault in pristine HTML at the list close.
    // The CVS tree gives the heading, literal BODY, and following text
    // distinct owners. html_fillmode() must retire the closed PRE below DL
    // without removing the new DL on the way to that old fill boundary.
    for (open, close, class, target, href) in [
        ("UR", "UE", "Lk", "https://one.test", "https://one.test"),
        ("MT", "ME", "Mt", "x@example.test", "mailto:x@example.test"),
    ] {
        for item in [".TP\nTAGMARK", ".IP TAGMARK 4"] {
            let source = format!(
                ".TH REVIEW 1 \"November 7, 2026\"\n.SH NAME\nreview \\- example\n.SH DESCRIPTION\n.nf\n.{open} {target}\nFIRSTMARK\n{item}\nLASTMARK\n.{close}\n.fi\nTAILMARK\n"
            );
            for fragment in [true, false] {
                let html = render(source.as_bytes(), fragment);
                let description = html.split_once("id=\"DESCRIPTION\"").unwrap().1;
                let (section, after) = description.split_once("</section>").unwrap();
                let (before_list, list) = section.split_once("<dl class=\"Bl-tag\">").unwrap();
                assert!(
                    before_list.contains(&format!(
                        "<pre><a class=\"{class}\" href=\"{href}\"> FIRSTMARK</a></pre>"
                    )),
                    "{html}"
                );
                let (contents, tail) = list.split_once("</dl>").unwrap();
                assert!(
                    contents.contains("TAGMARK</a></dt> <dd> <pre>LASTMARK</pre> </dd>"),
                    "{html}"
                );
                assert!(tail.contains("<p class=\"Pp\">TAILMARK</p>"), "{html}");
                assert!(after.trim_start().starts_with("</main>"), "{html}");
            }
        }
    }
}
