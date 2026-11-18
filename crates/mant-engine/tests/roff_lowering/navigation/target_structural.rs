//! Native display and list targets within a definition term.

use super::target_successors::{anchor_line, load};
use super::*;
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{DocumentIndex, NodeId};

#[test]
fn structural_target_survives_body_and_definition_head_lowering() {
    // Fixed CVS post_tg() assigns all five tags to the structure's direct
    // BODY. mdoc_html.c emits that ID on the display or list wrapper.
    for structure in [
        ".D1 MARK",
        ".Dl MARK",
        ".Bd -literal\nMARK\n.Ed",
        ".Bl -bullet\n.It\nMARK\n.El",
        ".Bl -column one two\n.It MARK Ta LAST\n.El",
    ] {
        let request = format!(".Tg Mixed.Target\n{structure}");
        for (body, line) in [
            (format!("{request}\n.Pp\nBODY.\n"), 9),
            (
                format!(".Bl -tag -width Ds\n.It Xo\n{request}\n.Xc\nBODY.\n.El\n"),
                11,
            ),
        ] {
            let query = json_roundtrip::json_string_roundtrip(&load(&body));
            let document = query.document.as_ref().unwrap();
            let index = DocumentIndex::build(document);
            assert_eq!(
                index.fragment_target("Mixed.Target").map(NodeId::as_str),
                Some("mixed-target"),
                "{body}"
            );
            anchor_line(document, "Mixed.Target", line);
            if line == 11 {
                let [Block::DefinitionList { items, .. }] = &document.sections[1].blocks[..] else {
                    panic!("expected definition owner: {body}");
                };
                let [item] = &items[..] else {
                    panic!("expected one definition item")
                };
                let entry = item.entry.as_ref().unwrap();
                assert_ne!(entry.id.as_str(), "mixed-target");
                assert!(
                    item.terms
                        .iter()
                        .any(|term| mant_ir::inline_plain_text(term).contains("MARK"))
                );
            }
            let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
            let marker = "<a id=\"Mixed.Target\"></a>";
            assert_eq!(markdown.matches(marker).count(), 1, "{markdown}");
            assert!(marker_precedes_content(&markdown, marker, "MARK", "BODY."));
        }
    }
}

fn marker_precedes_content(markdown: &str, marker: &str, target: &str, next: &str) -> bool {
    markdown.find(marker).unwrap() < markdown.find(target).unwrap()
        && markdown.find(target).unwrap() < markdown.rfind(next).unwrap()
}

#[test]
fn adjacent_head_structures_keep_their_individual_native_targets() {
    // Fixed CVS -Ttree/-Thtml place these targets on distinct D1/Dl BODY,
    // nested It BODY/BLOCK, or the Pp and Fo HEAD controls. The neighboring
    // untagged content contributes no authored alias.
    for (body, first, second) in [
        (
            ".Bl -tag -width Ds\n.It Xo\n.Tg First.Target\n.D1 FIRST\n.Tg Second.Target\n.Dl SECOND\n.Dl UNTAGGED\n.Xc\nBODY.\n.El\n",
            11,
            13,
        ),
        (
            ".Bl -tag -width Ds\n.It Xo\n.Bl -bullet\n.Tg First.Target\n.It\nFIRST\n.Tg Second.Target\n.It\nSECOND\n.It\nUNTAGGED\n.El\n.Xc\nBODY.\n.El\n",
            12,
            15,
        ),
        (
            ".Bl -tag -width Ds\n.It Xo\n.Bl -column one two\n.Tg First.Target\n.It FIRST Ta FIRSTCOL\n.Tg Second.Target\n.It SECOND Ta SECONDCOL\n.It UNTAGGED Ta LAST\n.El\n.Xc\nBODY.\n.El\n",
            12,
            14,
        ),
        (
            ".Bl -tag -width Ds\n.It Xo\nINTRO.\n.Tg First.Target\n.Pp\n.No FIRST\n.Tg Second.Target\n.Fo SECOND\n.Fa arg\n.Fc\n.Cm UNTAGGED\n.Xc\nBODY.\n.El\n",
            12,
            15,
        ),
    ] {
        let query = json_roundtrip::json_string_roundtrip(&load(body));
        let document = query.document.as_ref().unwrap();
        anchor_line(document, "First.Target", first);
        anchor_line(document, "Second.Target", second);
        assert!(
            DocumentIndex::build(document)
                .fragment_target("UNTAGGED")
                .is_none()
        );
        let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
        let first_marker = "<a id=\"First.Target\"></a>";
        let second_marker = "<a id=\"Second.Target\"></a>";
        for marker in [first_marker, second_marker] {
            assert_eq!(markdown.matches(marker).count(), 1, "{markdown}");
        }
        assert!(
            marker_precedes_content(&markdown, first_marker, "FIRST", second_marker),
            "{markdown}"
        );
        assert!(
            marker_precedes_content(&markdown, second_marker, "SECOND", "BODY."),
            "{markdown}"
        );
    }
}
