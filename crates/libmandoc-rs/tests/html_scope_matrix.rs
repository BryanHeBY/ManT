#![cfg(feature = "render")]

use std::path::Path;

use libmandoc_rs::{RenderFormat, Renderer, SourceBundle};

const HEADER: &str = ".TH P 1 \"2026-11-07\"\n.SH NAME\np \\- test\n.SH DESCRIPTION\n";
const NEXT: &str = ".SH NEXT\n.UR https://later.example\nNEXTMARK\n.UE\n";
const SYNOPSIS: &str = ".SY CMDMARK\nARGMARK\n.YS\n";
const MINIMAL: &str = include_str!("fixtures/html-scopes/synopsis-in-link.1");

#[derive(Debug)]
struct Marker<'a> {
    word: &'a str,
    section: usize,
    ancestors: Vec<&'a str>,
}

impl Marker<'_> {
    fn inside(&self, tag: &str) -> bool {
        self.ancestors
            .iter()
            .any(|element| tag_name(element) == tag)
    }

    fn linked_to(&self, href: &str) -> bool {
        let attribute = format!("href=\"{href}\"");
        self.ancestors
            .iter()
            .any(|element| tag_name(element) == "a" && element.contains(&attribute))
    }
}

fn tag_name(element: &str) -> &str {
    element.split([' ', '/', '\n']).next().unwrap()
}

// Read the generated markup without browser error recovery. Attributes and
// comments do not count as text, even when a permalink repeats a sentinel.
fn markers<'a>(mut html: &'a str, expected: &[&str]) -> Vec<Marker<'a>> {
    let mut stack: Vec<&str> = Vec::new();
    let mut section = 0;
    let mut found = Vec::new();
    loop {
        let (data, tag) = html.split_once('<').unwrap_or((html, ""));
        if stack
            .last()
            .is_some_and(|element| tag_name(element) == "tr")
        {
            assert!(data.trim().is_empty(), "direct text in table row: {data:?}");
        }
        for word in data.split(|ch: char| !ch.is_ascii_uppercase()) {
            if !stack.contains(&"style") && expected.contains(&word) {
                found.push(Marker {
                    word,
                    section,
                    ancestors: stack.clone(),
                });
            }
        }

        if tag.is_empty() {
            break;
        }
        if let Some(comment) = tag.strip_prefix("!--") {
            html = comment.split_once("-->").unwrap().1;
            continue;
        }
        let (element, rest) = tag.split_once('>').unwrap();
        html = rest;
        if let Some(close) = element.strip_prefix('/') {
            assert_eq!(stack.pop().map(tag_name), Some(close), "{element}");
        } else if !element.starts_with('!') {
            let name = tag_name(element);
            if stack
                .last()
                .is_some_and(|element| tag_name(element) == "tr")
            {
                assert!(
                    matches!(name, "td" | "th"),
                    "direct <{element}> in table row"
                );
            }
            if name == "section" {
                section += 1;
            }
            if !element.ends_with('/') {
                stack.push(element);
            }
        }
    }
    assert!(stack.is_empty(), "unclosed tags: {stack:?}");
    assert_eq!(
        found.iter().map(|marker| marker.word).collect::<Vec<_>>(),
        expected,
        "{found:?}"
    );
    found
}

#[test]
fn synopsis_inside_link_releases_its_row_before_following_text() {
    // Pristine CVS HTML faults in full mode and prematurely closes MAIN in
    // fragment mode. The tree puts AFTER beside SY, whose two TDs are emitted
    // by man_SY_pre(). The detached anchor cannot stand in for that boundary.
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/html-scopes/synopsis-in-link.1");
    let mut bundle = SourceBundle::new();
    bundle
        .insert("scope.1", MINIMAL.as_bytes().to_vec())
        .unwrap();
    std::thread::scope(|scope| {
        for fragment in [true, false] {
            let path = &path;
            let bundle = &bundle;
            scope.spawn(move || {
                let renderer = Renderer::new(RenderFormat::Html).with_html_fragment(fragment);
                for output in [
                    renderer
                        .render_bytes("scope.1", MINIMAL.as_bytes())
                        .unwrap()
                        .output,
                    renderer.render_file(path).unwrap().output,
                    renderer.render_bundle("scope.1", bundle).unwrap().output,
                    renderer
                        .render_bytes("scope.1", MINIMAL.as_bytes())
                        .unwrap()
                        .output,
                ] {
                    let hits = markers(&output, &["A", "ARG", "AFTER"]);
                    assert!(hits[0].linked_to("https://example.org"));
                    assert!(hits[1].inside("td") && hits[1].inside("tr"));
                    assert!(hits[2].inside("main") && hits[2].inside("section"));
                    assert!(!hits[2].inside("table") && !hits[2].inside("a"));
                }
            });
        }
    });
}

#[test]
fn man_boundary_matrix_preserves_ancestry_in_both_html_modes() {
    // The pristine tree/UTF-8 preserve these markers. The successful unwrapped
    // HTML controls show RS containing its BODY and SY containing only its two
    // cells. TP/IP own subsequent text up to the next section.
    for (open, close, href) in [
        ("", "", None),
        (
            ".UR https://example.org\n",
            ".UE\n",
            Some("https://example.org"),
        ),
        (".MT x@example.org\n", ".ME\n", Some("mailto:x@example.org")),
    ] {
        for (fill, restore) in [("", ""), (".nf\n", ".fi\n")] {
            for prefix in ["FIRSTMARK\n", "\n", "\\&\n"] {
                for (boundary, inside) in [
                    (".PP\n", &[][..]),
                    (".sp 0\n", &[][..]),
                    (".RS 2\nINSIDEMARK\n.RE\n", &["INSIDEMARK"][..]),
                    (".TP\nTAGMARK\nINSIDEMARK\n", &["TAGMARK", "INSIDEMARK"][..]),
                    (
                        ".IP TAGMARK 4\nINSIDEMARK\n",
                        &["TAGMARK", "INSIDEMARK"][..],
                    ),
                    (SYNOPSIS, &["CMDMARK", "ARGMARK"][..]),
                ] {
                    let source = format!(
                        "{HEADER}{fill}{open}{prefix}{boundary}AFTERMARK\n{close}{restore}{NEXT}"
                    );
                    let visible_prefix = prefix.starts_with("FIRSTMARK");
                    let mut expected = Vec::new();
                    if visible_prefix {
                        expected.push("FIRSTMARK");
                    }
                    expected.extend_from_slice(inside);
                    expected.extend(["AFTERMARK", "NEXTMARK"]);
                    for fragment in [true, false] {
                        let output = Renderer::new(RenderFormat::Html)
                            .with_html_fragment(fragment)
                            .render_bytes("scope.1", source.as_bytes())
                            .unwrap()
                            .output;
                        let hits = markers(&output, &expected);
                        let after = &hits[hits.len() - 2];
                        let next = &hits[hits.len() - 1];
                        assert_eq!(after.section, 2, "{source}\n{output}");
                        assert!(
                            after.inside("main")
                                && after.inside("section")
                                && !after.inside("table"),
                            "{source}\n{output}"
                        );
                        assert_eq!(next.section, 3, "{source}\n{output}");
                        assert!(
                            next.linked_to("https://later.example")
                                && !next.inside("table")
                                && !next.inside("dd")
                        );
                        if let Some(href) = href {
                            if visible_prefix {
                                assert!(hits[0].linked_to(href), "{source}\n{output}");
                            }
                            // roff_html_pre_sp() only ends a line inside PRE.
                            let keep_link = !fill.is_empty() && boundary == ".sp 0\n";
                            assert_eq!(after.linked_to(href), keep_link, "{source}\n{output}");
                        }
                        let content = &hits[usize::from(visible_prefix)..hits.len() - 2];
                        match content {
                            [argument] => {
                                assert!(
                                    argument.inside("div") && !after.inside("div"),
                                    "{source}\n{output}"
                                );
                            }
                            [term, body] if boundary == SYNOPSIS => {
                                assert!(
                                    term.inside("code") && term.inside("td") && body.inside("td"),
                                    "{source}\n{output}"
                                );
                                assert_eq!(
                                    body.inside("pre"),
                                    !fill.is_empty(),
                                    "{source}\n{output}"
                                );
                            }
                            [term, body] => {
                                assert!(
                                    term.inside("dt") && body.inside("dd"),
                                    "{source}\n{output}"
                                );
                            }
                            [] => {}
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn nested_man_and_mdoc_scopes_keep_surviving_ancestors() {
    let man = format!(
        "{HEADER}.RS 2\n.UR https://example.org\nFIRSTMARK\n{SYNOPSIS}AFTERMARK\n.UE\n.RE\nOUTSIDEMARK\n{NEXT}"
    );
    let mdoc = ".Dd November 9, 2026\n.Dt P 1\n.Os ManT\n.Sh NAME\n.Nm p\n.Nd test\n\
        .Sh SYNOPSIS\n.Nm CMDMARK\n.Ar ARGMARK\n.Sh DESCRIPTION\n\
        .Bl -tag -width TAGMARK\n.It Sy TAGMARK\n.Bd -literal\nINSIDEMARK\n.Ed\n.El\n.Pp\n.Em AFTERMARK\n\
        .Sh SEE ALSO\n.Xr NEXTMARK 1\n";
    for fragment in [true, false] {
        let renderer = Renderer::new(RenderFormat::Html).with_html_fragment(fragment);
        let output = renderer
            .render_bytes("scope.1", man.as_bytes())
            .unwrap()
            .output;
        let hits = markers(
            &output,
            &[
                "FIRSTMARK",
                "CMDMARK",
                "ARGMARK",
                "AFTERMARK",
                "OUTSIDEMARK",
                "NEXTMARK",
            ],
        );
        assert!(hits[3].inside("div") && !hits[3].inside("table"));
        assert!(!hits[4].inside("div") && hits[4].inside("section"));

        // Successful pristine HTML shows the mdoc Nm table, list and literal
        // display as separate owners. Reuse the renderer across dialects.
        let output = renderer
            .render_bytes("scope.1", mdoc.as_bytes())
            .unwrap()
            .output;
        let hits = markers(
            &output,
            &[
                "CMDMARK",
                "ARGMARK",
                "TAGMARK",
                "INSIDEMARK",
                "AFTERMARK",
                "NEXTMARK",
            ],
        );
        assert!(hits[0].inside("td") && hits[1].inside("td"));
        assert!(hits[2].inside("dt") && hits[3].inside("dd") && hits[3].inside("pre"));
        assert!(hits[4].inside("section") && hits[4].inside("i") && !hits[4].inside("dl"));
        assert_eq!(hits[5].section, 4);
    }
}
