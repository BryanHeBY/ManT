//! Tagged inline ink through the public JSON and Markdown CLI outputs.
#![cfg(feature = "roff")]

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde_json::Value;

const HEADER: &str = ".Dd November 26, 2026\n.Dt INSPECTION 1\n.Os\n.Sh NAME\n.Nm inspection\n.Nd annotations\n.Sh DESCRIPTION\n";
const TARGET: &str = "<span id=\"body-target\"></span><span id=\"Body.Target\"></span>";

fn run(body: &str, format: &str) -> String {
    run_with_options(body, format, true)
}

fn run_with_options(body: &str, format: &str, preserve_anchors: bool) -> String {
    let source = format!("{HEADER}{body}.Sh ENDTEST\nDone.\n");
    let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
    command.args([
        "--input",
        "-",
        "--input-format",
        "roff",
        "--display",
        "direct",
        "--format",
        format,
    ]);
    if format == "json" {
        command.arg("--compact");
    } else if preserve_anchors {
        command.arg("--preserve-anchors");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[derive(Default, Debug)]
struct Observation {
    ink: String,
    anchors: Vec<(usize, String, Vec<String>)>,
    links: Vec<(usize, char, String)>,
    manuals: Vec<(String, String)>,
}

fn observe(node: &Value, uri: Option<&str>, result: &mut Observation) {
    match node {
        Value::Array(nodes) => {
            for node in nodes {
                observe(node, uri, result);
            }
        }
        Value::Object(map) => match map.get("type").and_then(Value::as_str) {
            Some("text" | "code") => {
                for ch in map["value"]
                    .as_str()
                    .unwrap()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                {
                    if let Some(uri) = uri {
                        result
                            .links
                            .push((result.ink.chars().count(), ch, uri.into()));
                    }
                    result.ink.push(ch);
                }
            }
            Some("anchor") if map["id"] == "body-target" => {
                let aliases = map["fragmentAliases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|alias| alias.as_str().unwrap().to_owned())
                    .collect();
                result
                    .anchors
                    .push((result.ink.chars().count(), "body-target".into(), aliases));
            }
            _ => {
                let target = map.get("target").unwrap_or(&Value::Null);
                if target["kind"] == "manual" {
                    result.manuals.push((
                        target["name"].as_str().unwrap().into(),
                        target["manualSection"].as_str().unwrap().into(),
                    ));
                }
                let link = target["uri"].as_str().or(uri);
                for key in ["items", "terms", "description", "children", "content"] {
                    if let Some(child) = map.get(key) {
                        observe(child, link, result);
                    }
                }
            }
        },
        _ => {}
    }
}

fn json_observation(body: &str) -> Observation {
    let output = run(body, "json");
    let value: Value = serde_json::from_str(&output).unwrap();
    let section = value["document"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|section| section["id"] == "description")
        .unwrap();
    let mut result = Observation::default();
    observe(&section["blocks"], None, &mut result);
    result
}

#[test]
fn tagged_marker_owns_visible_glyphs_and_preserves_links() {
    // post_tg in mdoc_validate.c tags the macro; term.c::term_word writes
    // an invisible graph for \\% followed by the two ordinary glyphs.
    for token in ["Cm", "Dv", "Er"] {
        for word in [r"\%<>", "<>", r"\[u003C]\[u003E]", r"\&", r"\%", ""] {
            let body = format!(".No JA\n.Tg Body.Target\n.{token} \"{word}\" K\n");
            let result = json_observation(&body);
            let visible = !matches!(word, r"\&" | r"\%" | "");
            assert_eq!(
                result.ink,
                if visible { "JA<>K" } else { "JAK" },
                "{body}: {result:?}"
            );
            assert_eq!(
                result.anchors,
                [(2, "body-target".into(), vec!["Body.Target".into()])],
                "{body}: {result:?}"
            );
            assert!(
                result.links.is_empty() && result.manuals.is_empty(),
                "{body}: {result:?}"
            );
            let markdown = run(&body, "markdown");
            let section = markdown
                .split_once("## DESCRIPTION\n")
                .unwrap()
                .1
                .split_once("## ENDTEST")
                .unwrap()
                .0;
            let (before, after) = section
                .split_once(TARGET)
                .unwrap_or_else(|| panic!("{body}\n{section}"));
            assert!(before.contains("JA"), "{body}\n{section}");
            assert!(
                !before.contains("&lt;") && !before.contains('<'),
                "{body}\n{section}"
            );
            assert!(
                after.contains(if visible { r"\<\>" } else { "K" }),
                "{body}\n{section}"
            );
            assert_eq!(section.matches(TARGET).count(), 1);
        }
    }
    let body =
        ".No JA\n.Tg Body.Target\n.Cm \"\\X'tty: link https://a.test'\\%<>\\X'tty: link'\" K\n";
    let result = json_observation(body);
    assert_eq!(result.ink, "JA<>K");
    assert_eq!(
        result.anchors,
        [(2, "body-target".into(), vec!["Body.Target".into()])]
    );
    assert_eq!(
        result.links,
        [
            (2, '<', "https://a.test".into()),
            (3, '>', "https://a.test".into())
        ]
    );
    let markdown = run(body, "markdown");
    let target = markdown.find(TARGET).unwrap();
    let marker = markdown.find(r"\<\>").unwrap();
    let uri = markdown.find("https://a.test").unwrap();
    assert!(
        target < marker
            && marker < uri
            && markdown[target + TARGET.len()..marker].contains('[')
            && markdown[marker..uri].contains("]("),
        "{markdown}"
    );
}

#[test]
fn tagged_ink_does_not_capture_the_preceding_pending_glyph() {
    // term.c::encode1 resolves the preceding word's BACKBEFORE glyph
    // while the tagged operand executes; it retains its earlier owner.
    for (word, ink) in [(r"\%<>", "JA<>K"), (r"\&", "JAK")] {
        let body = format!(".No J\\zA\n.Tg Body.Target\n.Cm \"{word}\" K\n");
        let result = json_observation(&body);
        assert_eq!(result.ink, ink, "{body}: {result:?}");
        assert_eq!(
            result.anchors,
            [(2, "body-target".into(), vec!["Body.Target".into()])],
            "{body}: {result:?}"
        );
    }
}

#[test]
fn tagged_sphinx_reference_remains_a_manual_link() {
    let body = ".Tg Body.Target\n.No \"printf(3) \\%<>\" K\n";
    let result = json_observation(body);
    assert_eq!(result.ink, "printf(3)K");
    assert_eq!(
        result.anchors,
        [(0, "body-target".into(), vec!["Body.Target".into()])]
    );
    assert_eq!(result.manuals, [("printf".into(), "3".into())]);
    let markdown = run(body, "markdown");
    assert!(
        markdown.find(TARGET).unwrap() < markdown.find("printf(3)").unwrap(),
        "{markdown}"
    );
}

#[test]
fn linked_survivor_keeps_href_and_both_destinations_in_export() {
    // CVS mdoc_html.c emits a non-link target for Tg and a distinct href
    // owner for links. The pinned terminal witness reads JA ST K after
    // the sibling No overwrites the Ev's first operand.
    let body = ".No JA\n.Tg Body.Target\n.Ev \"\\%<>\" \"\\X'tty: link https://a.test'K\\X'tty: link'\" No \"\\h'|6n'ST\"\n";
    let observed = json_observation(body);
    assert_eq!(observed.ink, "JASTK");
    assert_eq!(observed.links, [(4, 'K', "https://a.test".into())]);
    assert_eq!(
        observed.anchors,
        [(4, "body-target".into(), vec!["Body.Target".into()])]
    );

    let clean = run_with_options(body, "markdown", false);
    assert!(clean.contains("[K](https://a.test)"), "{clean}");
    assert!(!clean.contains("Body.Target"), "{clean}");

    let auto = format!("{body}.Ss Sx DESCRIPTION\nMore.\n");
    for markdown in [
        run(body, "markdown"),
        run_with_options(&auto, "markdown", false),
    ] {
        let label = format!("[{TARGET}K](https://a.test)");
        assert!(markdown.contains(&label), "{markdown}");
        assert_eq!(markdown.matches(TARGET).count(), 1, "{markdown}");
    }
}
