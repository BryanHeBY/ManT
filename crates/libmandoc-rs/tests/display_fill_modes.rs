//! Implicit display closes restore the fill mode before later source nodes.
use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind, Parser};
use serde_json::{Value, json};

#[cfg(feature = "render")]
#[path = "support/native_render_region.rs"]
mod native_render_region;

const CASES: &str = include_str!("fixtures/display_modes.json");

#[test]
fn implicit_item_and_explicit_display_closes_preserve_native_fill_flags() {
    let fixture: Value = serde_json::from_str(CASES).unwrap();
    assert_eq!(fixture["header"]["count"], 18);
    assert_eq!(fixture["header"]["profilesPerSource"], 5);
    assert_eq!(fixture["header"]["expectationsFromProduct"], false);
    assert_eq!(
        fixture["header"]["oracleSha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    // Every complete source first ran pristine ASCII/UTF-8/HTML/tree/lint.
    // mdoc_macro.c::rew_pending restores ROFF_NOFILL from the original Bd
    // BLOCK NODE_NOFILL when It implicitly closes it. blk_exp_close retains
    // the existing Ed BODY restoration. A later fi/nf still takes precedence.
    for case in fixture["cases"].as_array().unwrap() {
        for _ in 0..2 {
            let report = Parser::default()
                .parse_bytes("display.1", case["source"].as_str().unwrap().as_bytes())
                .unwrap();
            let mut texts = Vec::new();
            positioned_text(&report.document.root, None, &mut texts);
            assert_eq!(
                json!(texts),
                case["expected"]["positionedText"],
                "{}",
                case["id"]
            );
            #[cfg(feature = "serde")]
            {
                let json = serde_json::to_string(&report).unwrap();
                let restored: libmandoc_rs::ParseReport = serde_json::from_str(&json).unwrap();
                assert_eq!(restored, report, "{}", case["id"]);
            }
        }
        #[cfg(feature = "render")]
        assert_native_render(case);
    }
}

fn positioned_text(node: &Node, owner: Option<&str>, output: &mut Vec<Value>) {
    let owner = match (node.macro_token.as_ref(), node.kind) {
        (Some(MacroToken::Mdoc(MdocMacro::It)), NodeKind::Head) => Some("It HEAD"),
        (Some(MacroToken::Mdoc(MdocMacro::It)), NodeKind::Body) => Some("It BODY"),
        _ => owner,
    };
    if node.kind == NodeKind::Text
        && node.text.as_deref().is_some_and(|text| {
            matches!(text, "BodyAlpha" | "BodyBeta" | "AfterAlpha" | "AfterBeta")
        })
    {
        output.push(json!({"label":node.text, "line":node.line,
            "column":node.column, "line_start":node.flags.line_start,
            "no_fill":node.flags.no_fill, "owner":owner}));
    }
    for child in &node.children {
        positioned_text(child, owner, output);
    }
}

#[cfg(feature = "render")]
fn assert_native_render(case: &Value) {
    use libmandoc_rs::{RenderFormat, Renderer};
    for (profile, format) in [("ascii", RenderFormat::Ascii), ("utf8", RenderFormat::Utf8)] {
        let report = Renderer::new(format)
            .with_width(78)
            .render_bytes("display.1", case["source"].as_str().unwrap().as_bytes())
            .unwrap();
        assert_eq!(
            native_render_region::description_body(&report.output),
            case["expected"]["nativeBodies"][profile].as_str().unwrap(),
            "{}: exact {profile} rows",
            case["id"]
        );
    }
    let report = Renderer::new(RenderFormat::Html)
        .render_bytes("display.1", case["source"].as_str().unwrap().as_bytes())
        .unwrap();
    let heading = report.output.find("id=\"DESCRIPTION\"").unwrap();
    let start = report.output[..heading]
        .rfind("<section class=\"Sh\">")
        .unwrap();
    let end = heading + report.output[heading..].find("</section>").unwrap() + "</section>".len();
    assert_eq!(
        &report.output[start..end],
        case["expected"]["nativeHtmlSection"].as_str().unwrap(),
        "{}: HTML recovers without assertion failure",
        case["id"]
    );
}
