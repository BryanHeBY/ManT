//! Display ownership after the native list parser closes an item.
use mant_ir::ResolvedContent;
use mant_loader::load_roff_bytes;
use mant_protocol::{ContentSelector, EvidenceClass, ExplanationOptions, ExplanationQuery};
use mant_query::{explain_query, select_excerpt};
use mant_render::{render_excerpt_text, render_query_text};

use super::json_roundtrip;

#[path = "display_ownership/real_cases.rs"]
mod real_cases;

const HEADER: &str =
    ".Dd November 19, 2026\n.Dt PROBE 4\n.Os\n.Sh NAME\n.Nm probe\n.Nd example\n.Sh DESCRIPTION\n";
const ITEMS: &str =
    ".Bl -tag -width Ds\n.It Dv FIRST\nFirst description.\n.It Dv SECOND\nSecond description.\n";
const DISPLAY: &str = "SENTRYDISPLAY";

fn check_display_owner(content: &ResolvedContent, name: &str, display: &str, owned: bool) {
    let explanation = explain_query(
        content,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let item = explanation
        .evidence
        .iter()
        .find(|entry| entry.class == EvidenceClass::DirectEntry)
        .unwrap_or_else(|| panic!("{name}: {explanation:?}"));
    let body = serde_json::to_string(item.content.as_ref().unwrap()).unwrap();
    let excerpt = select_excerpt(content, &[ContentSelector::path(item.outline.path())]).unwrap();
    assert_eq!(body.contains(display), owned, "{name}: {display}");
    assert_eq!(
        render_excerpt_text(&excerpt).contains(display),
        owned,
        "{name}: {display}"
    );
    assert_eq!(render_query_text(content).matches(display).count(), 1);
}

#[test]
fn an_offset_display_stays_on_the_authored_side_of_el() {
    // Fixed CVS mdoc_macro.c::blk_exp_close supplies the owner. The exact
    // ASCII/UTF-8 inputs retain the display and mdoc_html.c places it in the
    // item's dd only when it precedes El, independently of its text offset.
    let nested = format!(".Bl -tag -width Ds\n.It Dv OUTER\nOuter description.\n{ITEMS}");
    for (items, end) in [
        (
            ".Bl -tag -width Ds\n.It Dv SECOND\nSecond description.\n",
            ".El\n",
        ),
        (ITEMS, ".El\n"),
        (&nested, ".El\n.El\n"),
    ] {
        for inside in [true, false] {
            for kind in ["literal", "unfilled", "list"] {
                for offset in ["0n", "2n", "indent"] {
                    for gap in ["", ".Pp\n"] {
                        let display = if kind == "list" {
                            format!(
                                "{gap}.Bl -tag -width Ds -offset {offset}\n.It Dv CHILD\n{DISPLAY}\n.El\n"
                            )
                        } else {
                            format!("{gap}.Bd -{kind} -offset {offset}\n{DISPLAY}\n.Ed\n")
                        };
                        let body = if inside {
                            format!("{display}{end}")
                        } else {
                            format!("{end}{display}")
                        };
                        let source = format!("{HEADER}{items}{body}.Pp\nNext paragraph.\n");
                        let content = json_roundtrip::json_string_roundtrip(
                            &load_roff_bytes(source.as_bytes()).unwrap(),
                        );
                        check_display_owner(&content, "SECOND", DISPLAY, inside);
                    }
                }
            }
        }
    }
}
