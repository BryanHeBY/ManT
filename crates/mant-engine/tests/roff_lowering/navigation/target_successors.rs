//! Authored mdoc fragments follow the native tree and final target owners.

use super::*;
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{DocumentIndex, NodeId};
use mant_loader::load_roff_bytes;

const HEADER: &str = ".Dd November 18, 2026\n.Dt TARGET 7\n.Os\n.Sh NAME\n.Nm target\n.Nd navigation probe\n.Sh DESCRIPTION\n";
const TARGET: &str = "Mixed.Target";

fn load(body: &str) -> ResolvedContent {
    load_roff_bytes(format!("{HEADER}{body}").as_bytes()).unwrap()
}

fn anchor_line(document: &mant_ir::Document, fragment: &str, line: u32) {
    let index = DocumentIndex::build(document);
    let id = index
        .fragment_target(fragment)
        .unwrap_or_else(|| panic!("missing {fragment}: {index:?}"));
    let owners = anchor_owner_lines(document);
    assert_eq!(
        owners
            .iter()
            .filter(|(owner, _)| owner == id.as_str())
            .map(|(_, line)| *line)
            .collect::<Vec<_>>(),
        [line],
        "{fragment}: {owners:?}"
    );
}

fn check_destination(query: &ResolvedContent, line: u32, label: &str) {
    let query = json_roundtrip::json_string_roundtrip(query);
    let document = query.document.as_ref().unwrap();
    let index = DocumentIndex::build(document);
    assert_eq!(
        index.fragment_target(TARGET).map(NodeId::as_str),
        Some("mixed-target")
    );
    anchor_line(document, TARGET, line);

    let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    let marker = format!("<a id=\"{TARGET}\"></a>");
    assert_eq!(markdown.matches(&marker).count(), 1, "{markdown}");
    let (_, trailing) = markdown.split_once(&marker).unwrap();
    let (owner, _) = trailing.split_once("OWNERBODY.").unwrap();
    assert!(owner.contains(label), "{markdown}");
}

#[test]
fn argumentless_target_follows_native_macros_even_at_one_source_coordinate() {
    // Fixed CVS -Ttree and -Thtml place the ID on the following macro for all
    // 24 inputs. mdoc_validate.c::post_tg() uses native siblings, and every
    // node emitted by XX has the same source line as the call.
    for macro_name in [
        "Cm", "Dv", "Em", "Er", "Ev", "Fl", "Ic", "Li", "Ms", "No", "Sy", "Fn",
    ] {
        let request = format!(".Tg\n.{macro_name} {TARGET}\n");
        for (body, line) in [
            (format!("{request}OWNERBODY.\n"), 9),
            (format!(".de XX\n{request}..\n.XX\nOWNERBODY.\n"), 12),
        ] {
            check_destination(&load(&body), line, TARGET);
        }
    }
}

#[test]
fn derived_spelling_follows_tree_but_landing_uses_the_final_native_owner() {
    // CVS tag.c::tag_move_id() can move the following Fn's ID back to Pp.
    // The Eo case instead needs to ascend past BODY without selecting TAIL.
    for (body, line, label, later_line) in [
        (
            "INTRO.\n.Pp\n.de XX\n.Tg\n.Fn Mixed.Target\n..\n.XX\nOWNERBODY.\n.Pp\n.Fn Later.Automatic\nLATERBODY.\n",
            9,
            TARGET,
            Some(16),
        ),
        (
            ".de XX\n.Eo {LEFT\n.Tg\n.Ec RIGHT}\n.Fn Mixed.Target\n..\n.XX\nOWNERBODY.\n",
            14,
            TARGET,
            None,
        ),
        (
            ".de XX\n.Tg\n.Cm Mixed.Target\n..\n.XX\n.Fn Later.Automatic\nOWNERBODY.\n",
            12,
            TARGET,
            Some(13),
        ),
        (
            ".de XX\n.Tg Mixed.Target\n.Cm Supplemental\n..\n.XX\nOWNERBODY.\n",
            12,
            "Supplemental",
            None,
        ),
    ] {
        let query = load(body);
        check_destination(&query, line, label);
        if let Some(later_line) = later_line {
            let document = query.document.as_ref().unwrap();
            let index = DocumentIndex::build(document);
            assert!(index.fragment_target("Later.Automatic").is_none());
            anchor_line(document, "later-automatic", later_line);
        }
    }
}

#[test]
fn a_removed_or_invalid_argumentless_request_does_not_publish_a_fragment() {
    // CVS drops an operand-less Tg without a direct TEXT successor. A later
    // automatic Fn may still have an ID. It is not a surviving authored Tg.
    for (body, anchors, text) in [
        (
            ".Bl -column one two\n.Tg\n.It\n.El\n.Pp\n.Fn Later.Automatic\nLATERBODY.\n",
            &["later-automatic"][..],
            "LATERBODY.",
        ),
        (
            ".de XX\n.Tg\n.Fn \"Mixed.Target Tail\"\n..\n.XX\nOWNERBODY.\n",
            &["mixed-target"],
            "OWNERBODY.",
        ),
        ("BODYONLY.\n.Tg\n", &[], "BODYONLY."),
    ] {
        let query = load(body);
        let document = query.document.as_ref().unwrap();
        assert_eq!(anchor_ids(document), anchors, "{body}");
        assert!(visible_document_text(document).contains(text));
        let index = DocumentIndex::build(document);
        for fragment in [TARGET, "Later.Automatic"] {
            assert!(index.fragment_target(fragment).is_none(), "{body}");
        }
    }
}
