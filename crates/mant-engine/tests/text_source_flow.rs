//! Section facades and literal leaves preserve the same source row stream.
use mant_engine::{query_markdown_text, query_roff_bytes};
use mant_render::{render_excerpt_text, render_query_text, render_query_text_with};

#[test]
fn native_section_pd_and_explicit_requests_compose_once() {
    // mandoc CVS pre_SH consumes pardist; preceding .sp remains independent.
    for pd in [0, 1, 2] {
        for request in [0, 3] {
            for heading in ["SH", "SS"] {
                let source = format!(
                    ".TH PROBE 1\n.PD {pd}\n.SH FIRST\nALPHA\n.sp {request}\n.{heading} SECOND\nBETA\n"
                );
                let content = query_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&content);
                let heading_indent = if heading == "SS" { "  " } else { "" };
                assert!(
                    text.contains(&format!(
                        "ALPHA{}{heading_indent}SECOND\n",
                        "\n".repeat(pd + request + 1)
                    )),
                    "{source}\n{text:?}"
                );
                assert_eq!(
                    render_query_text_with(&content, |_, value| value.into()),
                    text
                );
            }
        }
    }
}

#[test]
fn section_tail_requests_survive_document_and_excerpt_facades() {
    let content = query_roff_bytes(b".TH PROBE 1\n.SH FIRST\nALPHA\n.sp 3\n").unwrap();
    assert!(render_query_text(&content).ends_with("ALPHA\n\n\n"));
    let excerpt =
        mant_engine::select_excerpt(&content, &[mant_protocol::ContentSelector::path("1")])
            .unwrap();
    assert!(render_excerpt_text(&excerpt).ends_with("ALPHA\n\n\n"));
}

#[test]
fn fenced_literal_edges_survive_document_and_excerpt_facades() {
    let content = query_markdown_text(
        "# PROBE\n\n## TEST\n\nBEFORE\n\n```text\n\nALPHA\n\n\n```\n\nAFTER\n",
        None,
    )
    .unwrap();
    let expected = "BEFORE\n\n\nALPHA\n\n\n\nAFTER";
    let text = render_query_text(&content);
    assert!(text.contains(expected), "{text:?}");
    let excerpt =
        mant_engine::select_excerpt(&content, &[mant_protocol::ContentSelector::path("1")])
            .unwrap();
    assert!(render_excerpt_text(&excerpt).contains(expected));
    assert_eq!(
        render_query_text_with(&content, |_, value| value.into()),
        text
    );
}

#[test]
fn definition_explain_preserves_nested_fence_blank_rows() {
    let content = query_markdown_text("# PROBE\n\n## TEST\n\n<!-- mant:entries role=option case=sensitive -->\n- `--example`: BEFORE\n\n  ```text\n\n  ALPHA\n\n\n  ```\n\n  AFTER\n", None).unwrap();
    let explanation = mant_engine::explain_query(
        &content,
        &mant_protocol::ExplanationQuery {
            entry: "--example".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    let text = mant_render::render_explanation_text(&explanation);
    assert!(
        text.contains("BEFORE\n\n\n  ALPHA\n\n\n\n  AFTER"),
        "{text:?}"
    );
    assert_eq!(
        mant_render::render_explanation_text_with(&explanation, |_, text| text.into()),
        text
    );
}

#[test]
fn empty_sections_do_not_insert_a_facade_default_gap() {
    let content =
        query_roff_bytes(b".TH PROBE 1\n.PD 2\n.SH FIRST\nALPHA\n.SH EMPTY\n.SH LAST\nBETA\n")
            .unwrap();
    let text = render_query_text(&content);
    // pre_SH deliberately suppresses paragraph distance after an empty SH.
    assert!(text.contains("ALPHA\n\n\nEMPTY\nLAST\nBETA"), "{text:?}");
}
