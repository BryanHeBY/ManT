//! Native target identity and order across structural owners.

use super::target_successors::anchor_line;
use super::*;
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{DocumentIndex, NodeId};

fn load(body: &str) -> ResolvedContent {
    let header = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
    mant_loader::load_roff_bytes(format!("{header}{body}").as_bytes()).unwrap()
}

fn assert_independent_destinations(
    body: &str,
    ids: &[&str],
    alias: &str,
    expected_sources: &[u32],
) {
    let query = json_roundtrip::json_string_roundtrip(&load(body));
    let document = query.document.as_ref().unwrap();
    let index = DocumentIndex::build(document);
    let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert_eq!(ids.len(), expected_sources.len());
    for (id, source) in ids.iter().zip(expected_sources) {
        assert_eq!(
            index.fragment_target(id).map(NodeId::as_str),
            Some(*id),
            "{body}"
        );
        assert_eq!(
            markdown
                .matches(&format!("<span id=\"{id}\"></span>"))
                .count(),
            1,
            "{body}\n{markdown}"
        );
        let mut spans = TargetSources {
            target: id,
            lines: Vec::new(),
        };
        spans.visit_document(document);
        assert_eq!(spans.lines, [*source], "{body}: {id}");
    }
    assert_eq!(index.fragment_target(alias), None, "{body}");
    assert!(index.is_fragment_ambiguous(alias), "{body}");
    assert!(
        !markdown.contains(&format!("<span id=\"{alias}\"></span>")),
        "{body}\n{markdown}"
    );
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let readback_document = readback.document.as_ref().unwrap();
    // A Markdown renderer keeps addressable export's raw HTML destinations.
    // ManT's semantic reimport handles the body, while JSON restores the IDs.
    let html = pulldown_cmark::Parser::new(&markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Html(value) | pulldown_cmark::Event::InlineHtml(value) => {
                Some(value)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for id in ids {
        let marker = format!("<span id=\"{id}\">");
        assert_eq!(
            html.iter()
                .map(|event| event.matches(&marker).count())
                .sum::<usize>(),
            1,
            "{body}\n{markdown}"
        );
    }
    for word in ["FIRST", "OUTER", "INNER", "MARK", "TARGET"] {
        if body.contains(word) {
            assert!(
                visible_document_text(readback_document).contains(word),
                "{body}"
            );
        }
    }
}

struct TargetSources<'a> {
    target: &'a str,
    lines: Vec<u32>,
}

impl<'a> Visit<'a> for TargetSources<'_> {
    fn visit_section(&mut self, section: &'a mant_ir::Section) {
        if section.id.as_str() == self.target {
            self.lines.push(section.source.unwrap().line);
        }
        visit::walk_section(self, section);
    }

    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Anchor {
            id, owner_source, ..
        } = inline
            && id.as_str() == self.target
        {
            self.lines.push(owner_source.unwrap().line);
        }
        visit::walk_inline(self, inline);
    }
}

#[test]
fn structural_and_descendant_targets_have_independent_native_owners() {
    // All 90 sources ran pristine CVS in five profiles with lint 0.
    // post_tg() places tags at the selected It/Bd/Bl/Sh or descendant
    // wrapper, and the HTML backend emits a separate destination for each.
    for (prefix, suffix, parent_line, parent_id) in [
        (
            ".Tg Mixed.Target\n.Bl -column Ds\n.It FIRST\n",
            ".El\n",
            9,
            "mixed-target",
        ),
        (
            ".Tg Mixed.Target\n.Bl -tag -width Ds\n.It TOP\nFIRST\n",
            ".El\n",
            9,
            "mixed-target",
        ),
        (
            ".Tg Mixed.Target\n.Bl -bullet\n.It\nFIRST\n",
            ".El\n",
            9,
            "mixed-target",
        ),
        (
            ".Bl -column Ds\n.Tg Mixed.Target\n.It FIRST\n",
            ".El\n",
            10,
            "mixed-target",
        ),
        (
            ".Bl -tag -width Ds\n.Tg Mixed.Target\n.It TOP\nFIRST\n",
            ".El\n",
            10,
            "mixed-target",
        ),
        (
            ".Bl -bullet\n.Tg Mixed.Target\n.It\nFIRST\n",
            ".El\n",
            10,
            "mixed-target",
        ),
        (
            ".Tg Mixed.Target\n.Bd -literal\nFIRST\n",
            ".Ed\n",
            9,
            "mixed-target",
        ),
        (
            ".Tg Mixed.Target\n.Bd -filled\nFIRST\n",
            ".Ed\n",
            9,
            "mixed-target",
        ),
        (".Tg Mixed.Target\n.Sh SUB\nFIRST\n", "", 9, "sub"),
    ] {
        for child in [
            ".Pp\nMARK\n",
            ".D1 TARGET\n",
            ".Dl TARGET\n",
            ".Bd -literal\nTARGET\n.Ed\n",
            ".Bl -bullet\n.It\nTARGET\n.El\n",
        ] {
            let child_line = u32::try_from(prefix.lines().count()).unwrap() + 9;
            for alias in ["Mixed.Target", "Child.Target"] {
                let body = format!("{prefix}.Tg {alias}\n{child}{suffix}");
                if alias == "Mixed.Target" {
                    let child_id = if parent_id == "sub" {
                        "mixed-target"
                    } else {
                        "mixed-target-2"
                    };
                    assert_independent_destinations(
                        &body,
                        &[parent_id, child_id],
                        alias,
                        &[parent_line, child_line],
                    );
                } else {
                    let query = json_roundtrip::json_string_roundtrip(&load(&body));
                    let document = query.document.as_ref().unwrap();
                    let index = DocumentIndex::build(document);
                    assert_eq!(
                        index.fragment_target("Mixed.Target").map(NodeId::as_str),
                        Some(parent_id),
                        "{body}"
                    );
                    assert_eq!(
                        index.fragment_target(alias).map(NodeId::as_str),
                        Some("child-target"),
                        "{body}"
                    );
                    anchor_line(document, alias, child_line);
                }
            }
        }
    }
}

#[test]
fn target_identity_does_not_depend_on_location_or_ancestry() {
    // In the macro case CVS reports identical line and column for the two
    // native BODY IDs. The nested case adds a third independent owner.
    for (body, ids, lines) in [
        (
            ".Tg Mixed.Target\n.Bd -filled\nOUTER\n.Tg Mixed.Target\n.Bl -bullet\n.It\nINNER\n.Tg Mixed.Target\n.D1 TARGET\n.El\n.Ed\n",
            &["mixed-target", "mixed-target-2", "mixed-target-3"][..],
            &[9, 12, 16][..],
        ),
        (
            ".de XX\n.Tg Mixed.Target\n.Bl -column Ds\n.It FIRST\n.Tg Mixed.Target\n.D1 TARGET\n.El\n..\n.XX\n",
            &["mixed-target", "mixed-target-2"][..],
            &[16, 16][..],
        ),
        (
            ".Bl -column Ds\n.It FIRST\n.Tg Outer.Target\n.Ox 7.6\n.Tg Outer.Target\n.D1 TARGET\n.El\n",
            &["outer-target", "outer-target-2"][..],
            &[9, 13][..],
        ),
        (
            ".Bl -column Ds\n.It FIRST\n.Tg Mixed.Target\n.D1 FIRSTCHILD\n.Tg Mixed.Target\n.Dl SECONDCHILD\n.El\n",
            &["mixed-target", "mixed-target-2"][..],
            &[11, 13][..],
        ),
        (
            ".Bl -bullet\n.Tg Mixed.Target\n.Tg Mixed.Target\n.It\nFIRST\n.El\n",
            &["mixed-target", "mixed-target-2"][..],
            &[9, 11][..],
        ),
    ] {
        let alias = if body.contains("Outer.Target") {
            "Outer.Target"
        } else {
            "Mixed.Target"
        };
        assert_independent_destinations(body, ids, alias, lines);
    }
    // A distinct automatic HEAD target remains independently reachable.
    // When both spellings are the same, CVS manual priority removes
    // the automatic target; only the actual surviving native owner counts.
    for (alias, auto_id) in [("Outer.Target", Some("auto")), ("auto", Some("auto"))] {
        let body = format!(".Tg {alias}\n.Bl -tag -width Ds\n.It Cm auto\nBODY\n.El\n");
        let query = load(&body);
        let document = query.document.as_ref().unwrap();
        let index = DocumentIndex::build(document);
        assert!(index.fragment_target(alias).is_some(), "{body}");
        assert_eq!(
            index.fragment_target("auto").map(NodeId::as_str),
            auto_id,
            "{body}"
        );
        assert!(!index.is_fragment_ambiguous("auto"), "{body}");
    }
}

#[test]
fn display_body_anchor_follows_opening_sibling_at_every_output_destination() {
    // mdoc_html.c::mdoc_d1_pre opens the tagged div at BODY, after the
    // opening delimiter allocated as a direct BLOCK child. Tree and both
    // terminal profiles cover the four initial-delimiter cases where the
    // pristine HTML renderer asserts outside a preceding paragraph.
    for (left, right) in [
        ("", ""),
        (".Bl -column a b\n.It FIRST Ta \\&\n", ".El\n"),
        (".Bl -bullet\n.It\n", ".El\n"),
        (".Bl -tag -width Ds\n.It TOP\n", ".El\n"),
        (".Bl -tag -width Ds\n.It Xo\n", ".Xc\nAFTER\n.El\n"),
        (".Bd -literal\n", ".Ed\n"),
    ] {
        for display in ["D1", "Dl"] {
            for (arg, expected) in [("MARK", "<a>MARK"), ("( MARK )", "(<a>MARK)")] {
                for before in ["", ".No BEFORE\n"] {
                    for following in ["", ".Tg Next.Target\n.D1 NEXT\n"] {
                        let body = format!(
                            "{left}{before}.Tg Mixed.Target\n.{display} {arg}\n{following}{right}"
                        );
                        let query = json_roundtrip::json_string_roundtrip(&load(&body));
                        let document = query.document.as_ref().unwrap();
                        anchor_line(
                            document,
                            "Mixed.Target",
                            9 + u32::try_from(left.lines().count() + before.lines().count())
                                .unwrap(),
                        );
                        let mut tape = DisplayTape(String::new());
                        tape.visit_document(document);
                        assert!(tape.0.contains(expected), "{body}\n{}", tape.0);
                        if !before.is_empty() {
                            assert!(
                                tape.0.find("BEFORE").unwrap() < tape.0.find(expected).unwrap(),
                                "{body}"
                            );
                        }
                        if !following.is_empty() {
                            assert!(
                                tape.0.find(expected).unwrap() < tape.0.find("<next>NEXT").unwrap(),
                                "{body}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn block_display_target_uses_the_actual_fill_owner() {
    // mdoc_bd_pre opens the ID-bearing div before a descendant's NODE_NOFILL
    // selects the HTML pre. The pinned HTML puts hello inside that same div
    // for all display forms, including after an empty row or a fill request.
    for kind in ["ragged", "filled", "centered", "unfilled", "literal"] {
        for prefix in ["", "\n", ".No \\&\n", ".fi\n", ".nf\n", ".Pp\n"] {
            let body = format!(".Tg display-target\n.Bd -{kind}\n{prefix}hello\n.Ed\n");
            let query = json_roundtrip::json_string_roundtrip(&load(&body));
            let document = query.document.as_ref().unwrap();
            let literal =
                prefix != ".fi\n" && (matches!(kind, "unfilled" | "literal") || prefix == ".nf\n");
            let section = &document.sections.last().unwrap().blocks;
            let owner = section.iter().find(|block| match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    mant_ir::inline_plain_text(children).contains("hello")
                }
                _ => false,
            });
            match owner {
                Some(Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) => {
                    assert_eq!(
                        matches!(owner, Some(Block::Preformatted { .. })),
                        literal,
                        "{body}"
                    );
                    assert!(
                        children.iter().any(|child| matches!(child, Inline::Anchor { id, .. } if id == "display-target")),
                        "{body}\n{section:?}"
                    );
                }
                _ => panic!("{body}\n{section:?}"),
            }
            anchor_line(document, "display-target", 9);
        }
    }
}

struct DisplayTape(String);

impl<'a> Visit<'a> for DisplayTape {
    fn visit_inline(&mut self, inline: &'a Inline) {
        match inline {
            Inline::Text { value } | Inline::Code { value } => {
                self.0.extend(value.chars().filter(|c| !c.is_whitespace()));
            }
            Inline::Anchor { id, .. } if id == "mixed-target" => self.0.push_str("<a>"),
            Inline::Anchor { id, .. } if id == "next-target" => self.0.push_str("<next>"),
            _ => visit::walk_inline(self, inline),
        }
    }
}
