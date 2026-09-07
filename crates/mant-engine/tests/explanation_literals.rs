//! Literal evidence must not shorten executable names, with or without markup.
use mant_engine::{query_markdown_text, select_explanation};

#[test]
fn plain_code_and_transparent_wrappers_share_literal_boundaries() {
    for wrap in [("", ""), ("`", "`"), ("**", "**"), ("***", "***")] {
        for (long, prefix) in [
            ("-###", "-#"),
            ("--%", "--"),
            ("--all", "-a"),
            ("-ab", "-a"),
            ("-ca.cert", "-ca"),
        ] {
            let source = format!(
                "# Probe\n\nUse {}{long}{} to print commands.\n",
                wrap.0, wrap.1
            );
            let query = query_markdown_text(&source, None).unwrap();
            assert_eq!(
                select_explanation(&query, prefix).unwrap().total,
                0,
                "{source}"
            );
            assert_eq!(
                select_explanation(&query, long).unwrap().total,
                1,
                "{source}"
            );
            assert!(query.document.unwrap().diagnostics.is_empty());
        }
    }
    let query = query_markdown_text("Use (`--help=CLASS`), `-I`. 日本語。", None).unwrap();
    assert_eq!(select_explanation(&query, "--help").unwrap().total, 1);
    assert_eq!(select_explanation(&query, "-I").unwrap().total, 1);
    assert_eq!(select_explanation(&query, "-i").unwrap().total, 0);
}
