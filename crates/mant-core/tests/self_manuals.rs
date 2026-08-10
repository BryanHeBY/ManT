//! Keeps the shipped Markdown manuals inside the supported document subset.

use mant_ast::{ExcerptSelection, OutlineDetail, OutlineNode, TldrCommandPart, TldrOrigin};
use mant_core::{
    build_outline_with_detail, query_markdown_text, render_markdown, render_query_text,
    select_excerpt,
};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

const MANT_MANUAL: &str = include_str!("../../../docs/manuals/mant.md");
const PROTOCOL_REFERENCE: &str = include_str!("../../../docs/protocol.md");

#[test]
fn shipped_manual_parses_without_lossy_fallbacks() {
    let name = "mant.md";
    let query = query_markdown_text(MANT_MANUAL, Some(format!("docs/manuals/{name}")))
        .expect("self manual query");
    let document = query.document.as_ref().expect("manual body");
    let tldr = query.tldr.as_ref().expect("embedded tldr");

    assert_eq!(document.meta.title.as_deref(), Some("mant"));
    assert!(
        !document.sections.is_empty(),
        "{name} has a navigable outline"
    );
    assert_eq!(
        document.sections[0].title, "Name",
        "{name} begins its manual body with a conventional Name section"
    );
    assert!(
        document.diagnostics.is_empty(),
        "{name} must not rely on unsupported Markdown: {:?}",
        document.diagnostics
    );
    assert_eq!(tldr.origin, TldrOrigin::Embedded);
    assert!(
        tldr.examples.len() >= 4
            && tldr
                .examples
                .iter()
                .all(|example| !example.description.is_empty() && !example.command.is_empty()),
        "{name} quick reference follows the tldr description/command layout"
    );
    assert!(
        tldr.examples.iter().any(|example| {
            example
                .command_parts
                .iter()
                .any(|part| matches!(part, TldrCommandPart::Placeholder { .. }))
        }),
        "{name} uses the standard tldr placeholder syntax that drives command highlighting"
    );
    assert!(
        tldr.examples.iter().any(|example| {
            example.description == "Inspect a manual outline"
                && example.command == "mant {{name}} --outline {{sections}}"
        }),
        "{name} quick reference exposes the section-only outline"
    );

    let outline =
        build_outline_with_detail(&query, OutlineDetail::Sections).expect("self manual outline");
    assert_eq!(outline.nodes[0].path(), "0");
    assert!(matches!(
        &outline.nodes[1],
        OutlineNode::DocumentSection { path, title, .. }
            if path == "1" && title == "Name"
    ));
    let excerpt = select_excerpt(&query, &["tldr".to_owned()]).expect("TLDR alias");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::Tldr { path, document, .. }]
            if path == "0" && document.origin == TldrOrigin::Embedded
    ));
    let markdown = render_markdown(&query);
    assert!(!markdown.contains("<a "));
    assert!(
        !markdown.contains("tldr-pages · CC BY 4.0"),
        "{name} must not claim the community cache licence for owned content"
    );
    assert!(!render_query_text(&query).is_empty());
}

#[test]
fn shipped_manual_options_are_addressable_for_agents_and_the_tui() {
    let query = query_markdown_text(MANT_MANUAL, Some("docs/manuals/mant.md".to_owned()))
        .expect("self manual query");
    let outline =
        build_outline_with_detail(&query, OutlineDetail::Options).expect("manual outline");

    for expected in ["--manual", "--search", "--ui", "--help"] {
        assert!(
            contains_entry(&outline.nodes, expected),
            "mant.md should expose {expected} as a semantic entry"
        );
    }
}

#[test]
fn shipped_manuals_explain_project_local_roff_lookup() {
    for required in [
        "### Local Roff Trees",
        "MANT_MANPATH",
        "MANPATH",
        "project-man/man1/widget.1",
        "without invoking `man`",
        "Do not pass `./widget.1`",
    ] {
        assert!(
            MANT_MANUAL.contains(required),
            "mant.md should document local roff lookup with {required:?}"
        );
    }
    assert!(
        PROTOCOL_REFERENCE.contains("the native index reads `MANT_MANPATH`"),
        "the request reference should define the manual lookup environment"
    );
}

#[test]
fn shipped_manual_explains_recursive_registered_documents() {
    for required in [
        "$HOME/.local/share/mant/documents",
        "team/handbook.md",
        "Directories organize documents but do not form",
        "symbolic-link cycles",
        "registration directory is a trust boundary",
        "read-only MCP server",
    ] {
        assert!(
            MANT_MANUAL.contains(required),
            "mant.md should document registered Markdown with {required:?}"
        );
    }
}

#[test]
fn protocol_reference_is_structured_and_its_json_examples_are_valid() {
    let query = query_markdown_text(PROTOCOL_REFERENCE, Some("docs/protocol.md".to_owned()))
        .expect("protocol reference query");
    let document = query.document.as_ref().expect("protocol document");

    assert_eq!(
        document.meta.title.as_deref(),
        Some("ManT JSON Protocol and Schema Reference")
    );
    assert!(
        document.diagnostics.is_empty(),
        "the protocol reference must remain inside ManT's supported Markdown subset: {:?}",
        document.diagnostics
    );
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.title == "Document AST")
    );

    let examples = json_fenced_examples(PROTOCOL_REFERENCE);
    for json in &examples {
        serde_json::from_str::<serde_json::Value>(json).expect("valid JSON example");
    }
    assert!(
        examples.len() >= 10,
        "the protocol reference should retain comprehensive JSON examples"
    );
}

#[test]
fn protocol_json_examples_are_independent_from_checkout_line_endings() {
    for newline in ["\n", "\r\n"] {
        let markdown = format!("```json{newline}{{\"portable\":true}}{newline}```{newline}");
        assert_eq!(
            json_fenced_examples(&markdown),
            ["{\"portable\":true}\n"],
            "JSON fences should parse with {newline:?} line endings"
        );
    }
}

/// Read JSON examples through `CommonMark` events instead of checkout-specific
/// newline bytes. Git may materialize the same tracked Markdown as LF or CRLF.
fn json_fenced_examples(markdown: &str) -> Vec<String> {
    let mut examples = Vec::new();
    let mut current = None;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.split_whitespace().next() == Some("json") =>
            {
                current = Some(String::new());
            }
            Event::Text(text) if current.is_some() => {
                current.as_mut().expect("JSON block state").push_str(&text);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(json) = current.take() {
                    examples.push(json);
                }
            }
            _ => {}
        }
    }

    examples
}

fn contains_entry(nodes: &[OutlineNode], name: &str) -> bool {
    nodes.iter().any(|node| {
        matches!(
            node,
            OutlineNode::DocumentEntry { names, .. }
                if names.iter().any(|candidate| candidate == name)
        ) || contains_entry(node.children(), name)
    })
}
