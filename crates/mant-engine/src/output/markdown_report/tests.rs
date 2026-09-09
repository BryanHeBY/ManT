//! Render an existing protocol snapshot without performing queries.

use mant_protocol::QueryBundle;

use mant_codec::encode::{MarkdownOptions, render_markdown, render_markdown_with_options};

#[test]
fn renders_the_shared_query_contract_without_leaking_json() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/contracts/minimal-query-v0.11.json");
    if !fixture.exists() {
        // The tagged repository owns shared process-contract fixtures; they
        // intentionally remain outside the published engine package.
        return;
    }
    let query = serde_json::from_str::<QueryBundle>(
        &std::fs::read_to_string(fixture).expect("shared query fixture"),
    )
    .expect("query contract")
    .into();

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# ls\n"));
    assert!(markdown.contains("## TLDR"));
    assert!(markdown.contains("## NAME"));
    assert!(markdown.contains("**ls**"));
    assert!(
        markdown.contains("[the project site](https://example.test/ls \"Project documentation\")")
    );
    assert!(markdown.contains("[the documentation team](mailto:docs@example.test)"));
    assert!(markdown.contains(", or read OPTIONS"));
    assert!(!markdown.contains("[OPTIONS](#options-1)"));
    assert!(!markdown.contains("<a "));
    assert!(!markdown.contains("mant.query/v0.11"));

    let addressable = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(addressable.contains("[OPTIONS](#options-1)"));
    assert!(addressable.contains("<a id=\"options-1\"></a>\n\n## OPTIONS"));
    assert!(addressable.contains("<a id=\"all-option\"></a>"));
}
