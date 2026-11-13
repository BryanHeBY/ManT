//! Markdown behavior contracts.

use super::*;
#[test]
fn preserves_markdown_anchors_only_when_requested() {
    assert!(matches!(
        parse(&args(&["git", "--preserve-anchors"])).expect("addressable Markdown"),
        Command::Query {
            presentation: OutputOptions {
                format: Some(QueryFormat::Markdown),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            preserve_anchors: true,
            ..
        }
    ));
}

#[test]
fn mathematical_markdown_uses_the_same_policy_for_documents_and_reports() {
    for (mode, expected) in [
        ("readable", super::MarkdownMathMode::Readable),
        ("latex", super::MarkdownMathMode::Latex),
        ("mathml", super::MarkdownMathMode::Mathml),
    ] {
        assert!(matches!(
            parse(&args(&["git", "--markdown-math", mode])).unwrap(),
            Command::Query {
                presentation: OutputOptions {
                    format: Some(QueryFormat::Markdown),
                    markdown_math: Some(selected),
                    ..
                },
                ..
            } if selected == expected
        ));
        for arguments in [
            vec!["git", "--format", "json"],
            vec!["git", "--format", "text"],
            vec!["--list"],
            vec!["--find", "git"],
            vec!["--doctor"],
            vec!["--schema", "all"],
            vec!["--protocol-version"],
            #[cfg(feature = "update")]
            vec!["--update-docs"],
            #[cfg(feature = "update")]
            vec!["--update-tldr"],
            #[cfg(feature = "update")]
            vec!["--prune-docs"],
            #[cfg(feature = "mcp")]
            vec!["--mcp"],
        ] {
            let mut values = arguments;
            values.extend(["--markdown-math", mode]);
            assert!(parse(&args(&values)).is_err(), "{values:?}");
        }
        for mut values in [
            vec!["git", "--outline"],
            vec!["git", "--node", "1"],
            vec!["git", "--search", "foo"],
            vec!["git", "--explain", "foo"],
            vec!["--document", "git", "--search", "foo"],
        ] {
            values.extend(["--markdown-math", mode]);
            assert!(parse(&args(&values)).is_ok(), "{values:?}");
        }
    }
    assert!(parse(&args(&["--request-json", "--markdown-math", "latex"])).is_ok());
    assert!(parse(&args(&["git", "--markdown-math", "tex"])).is_err());
}
