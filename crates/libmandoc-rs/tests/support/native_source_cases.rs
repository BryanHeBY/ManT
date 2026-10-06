//! Independently recorded source arguments and metadata at the owned boundary.

use libmandoc_rs::{MacroToken, Node, NodeKind, ParseReport, Parser};
use serde_json::{Value, json};

pub fn cases(source: &str, count: usize) -> Vec<Value> {
    let fixture: Value = serde_json::from_str(source).unwrap();
    assert_eq!(fixture["header"]["count"], count);
    assert_eq!(fixture["header"]["profilesPerSource"], 5);
    assert_eq!(fixture["header"]["expectationsFromProduct"], false);
    assert_eq!(
        fixture["header"]["oracleSha256"],
        "6297105d1370a44fd306851ae5beab3f23492d756a682f8a0693896922249ea0"
    );
    fixture["cases"].as_array().unwrap().clone()
}

pub fn assert_case(case: &Value) {
    // Each exact source ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // first. mdoc_validate.c::rewrite_macro2len receives iswidth only for
    // Bl -width; column arguments and offsets retain their separate rules.
    // msec.c::mandoc_a2msec tries the whole section before its first byte.
    let source = case["source"].as_str().unwrap();
    for _ in 0..2 {
        let report = Parser::default()
            .parse_bytes("source.roff", source.as_bytes())
            .unwrap();
        assert_owned(&report, case);
        #[cfg(feature = "serde")]
        {
            let json = serde_json::to_string(&report).unwrap();
            let restored: ParseReport = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, report, "{}", case["id"]);
        }
    }
    #[cfg(feature = "render")]
    assert_native_render(case);
}

fn assert_owned(report: &ParseReport, case: &Value) {
    let meta = &report.document.metadata;
    assert_eq!(
        json!({"title": meta.title, "name": meta.name, "section": meta.section,
            "volume": meta.volume, "arch": meta.arch, "os": meta.os,
            "date": meta.date, "hasBody": meta.has_body}),
        case["expected"]["metadata"],
        "{}: bibliographic values, independent of headings",
        case["id"]
    );
    let mut texts = Vec::new();
    let mut arguments = Vec::new();
    let mut positioned = Vec::new();
    collect_nodes(
        &report.document.root,
        false,
        false,
        &mut texts,
        &mut arguments,
        &mut positioned,
    );
    assert_eq!(json!(texts), case["expected"]["texts"], "{}", case["id"]);
    assert_eq!(
        json!(arguments),
        case["expected"]["normalizedArguments"],
        "{}: native normalized arguments",
        case["id"]
    );
    assert_eq!(
        json!(positioned),
        case["expected"]["positionedText"],
        "{}: actual HEAD/BODY source ownership",
        case["id"]
    );
    let diagnostics = report
        .diagnostics
        .iter()
        // The unrelated Os policy style finding depends on the build's
        // target OS. Raw reference evidence retains it; source errors and
        // all width/section diagnostics remain exact here.
        .filter(|finding| {
            !finding
                .message
                .starts_with("operating system explicitly specified: Os ")
        })
        .map(|finding| {
            assert!(finding.code.is_none());
            let location = finding.location.unwrap();
            json!({"level": format!("{:?}", finding.level),
                "message": finding.message, "line": location.line,
                "column": location.column})
        })
        .collect::<Vec<_>>();
    assert_eq!(
        json!(diagnostics),
        case["expected"]["diagnostics"],
        "{}: diagnosed inputs do not become clean-input claims",
        case["id"]
    );
}

fn collect_nodes(
    node: &Node,
    in_head: bool,
    in_body: bool,
    texts: &mut Vec<String>,
    arguments: &mut Vec<Value>,
    positioned: &mut Vec<Value>,
) {
    let name = node.macro_token.as_ref().map(MacroToken::as_str);
    if node.kind == NodeKind::Block && matches!(name, Some("Bl" | "Bd")) {
        arguments.push(json!({"macro": name, "line": node.line,
            "column": node.column, "width": node.width, "offset": node.offset,
            "columns": node.columns, "kind": node.list_kind.map(|kind| format!("{kind:?}")),
            "style": node.definition_list_style.map(|style| format!("{style:?}")),
            "display": node.display_kind.map(|display| format!("{display:?}")),
            "insideItemHead": in_head}));
    }
    if node.kind == NodeKind::Text {
        let value = node.text.as_ref().unwrap();
        texts.push(value.clone());
        if matches!(
            value.as_str(),
            "alpha" | "--alpha" | "HeadWord" | "BodyWord" | "NestedWord" | "AfterWord"
        ) {
            positioned.push(json!({"value": value, "line": node.line,
                "column": node.column, "lineStart": node.flags.line_start,
                "noFill": node.flags.no_fill, "insideItemHead": in_head,
                "insideItemBody": in_body}));
        }
    }
    let item = matches!(name, Some("It" | "TP"));
    for child in &node.children {
        collect_nodes(
            child,
            in_head || (item && node.kind == NodeKind::Head),
            in_body || (item && node.kind == NodeKind::Body),
            texts,
            arguments,
            positioned,
        );
    }
}

#[cfg(feature = "render")]
fn assert_native_render(case: &Value) {
    use libmandoc_rs::{RenderFormat, Renderer};
    for (profile, format) in [("ascii", RenderFormat::Ascii), ("utf8", RenderFormat::Utf8)] {
        let output = Renderer::new(format)
            .with_width(78)
            .render_bytes("source.roff", case["source"].as_str().unwrap().as_bytes())
            .unwrap()
            .output;
        assert_eq!(
            native_body(&output),
            case["expected"]["nativeBodies"][profile].as_str().unwrap(),
            "{}: {profile} raw scoped rows",
            case["id"]
        );
    }
    let output = Renderer::new(RenderFormat::Html)
        .render_bytes("source.roff", case["source"].as_str().unwrap().as_bytes())
        .unwrap()
        .output;
    let heading = output.find("id=\"OPTIONS\"").unwrap();
    let start = output[..heading].rfind("<section class=\"Sh\">").unwrap();
    let end = heading + output[heading..].find("</section>").unwrap() + "</section>".len();
    assert_eq!(
        &output[start..end],
        case["expected"]["nativeHtmlSection"].as_str().unwrap(),
        "{}: native structured section",
        case["id"]
    );
    let volume = output.split_once("class=\"head-vol\">").unwrap().1;
    let volume = volume.split_once("</span>").unwrap().0;
    // mdoc_html.c::mdoc_root_pre appends " (arch)" to the displayed
    // volume, while meta.vol remains the bare bibliographic value.
    // Only HTML header formatting whitespace is normalized here; the
    // section assertion above keeps body bytes and hard rows exact.
    assert_eq!(
        volume.split_whitespace().collect::<Vec<_>>().join(" "),
        case["expected"]["nativeHtmlVolume"].as_str().unwrap(),
        "{}: native volume header",
        case["id"]
    );
}

#[cfg(feature = "render")]
fn native_body(raw: &str) -> &str {
    let mut start = None;
    let mut offset = 0;
    for row in raw.split_inclusive('\n') {
        let mut projected = String::new();
        for scalar in row.chars() {
            if scalar == '\u{8}' {
                projected.pop();
            } else {
                projected.push(scalar);
            }
        }
        if projected == "OPTIONS\n" {
            start = Some(offset + row.len());
        } else if projected == "NEXT\n" {
            return &raw[start.unwrap()..offset];
        }
        offset += row.len();
    }
    panic!("missing native OPTIONS/NEXT scope")
}
