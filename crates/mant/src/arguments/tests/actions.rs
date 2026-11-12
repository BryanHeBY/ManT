//! Actions behavior contracts.

use super::*;
#[test]
fn parses_grouped_lists_and_grep_like_catalog_searches() {
    assert_eq!(
        parse(&args(&["--list", "--source", "pwsh7"])).expect("catalog list"),
        Command::Catalog {
            query: CatalogQuery {
                pattern: None,
                source: Some("pwsh7".to_owned()),
                limit: 10_000,
                ..CatalogQuery::default()
            },
            grouped: true,
            presentation: OutputOptions {
                format: Some(QueryFormat::Text),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: true,
        }
    );
    #[cfg(feature = "roff")]
    assert_eq!(
        parse(&args(&[
            "--find",
            "^PRINT",
            "--regex",
            "--case",
            "sensitive",
            "--kind",
            "manual",
            "--man-section",
            "3",
            "--limit",
            "20",
            "--format",
            "json",
            "--compact",
        ]))
        .expect("catalog search"),
        Command::Catalog {
            query: CatalogQuery {
                pattern: Some("^PRINT".to_owned()),
                syntax: SearchSyntax::Regex,
                case: SearchCase::Sensitive,
                kind: Some(CatalogDocumentKind::Manual),
                source: None,
                manual_section: Some("3".to_owned()),
                limit: 20,
                offset: 0,
            },
            grouped: false,
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: false,
        }
    );

    assert!(matches!(
        parse(&args(&["--list", "--display", "direct"])).expect("direct catalog list"),
        Command::Catalog {
            presentation: OutputOptions {
                display: DisplayMode::Direct,
                ..
            },
            ..
        }
    ));

    for invalid in [
        vec!["--list", "--regex"],
        vec!["--list", "--format", "markdown"],
        vec!["git", "--limit", "2"],
        vec!["git", "--kind", "manual"],
        vec!["git", "--outline", "--format", "man"],
        vec!["git", "--node", "1", "--format", "man"],
        vec!["git", "--explain", "branch", "--format", "man"],
        vec!["git", "--search", "branch", "--format", "man"],
    ] {
        assert!(parse(&args(&invalid)).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn parses_long_option_actions_without_ad_hoc_subcommands() {
    assert_eq!(
        parse(&args(&["--doctor"])).expect("doctor"),
        Command::Doctor {
            presentation: OutputOptions {
                format: Some(QueryFormat::Text),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: true,
        }
    );
    assert_eq!(
        parse(&args(&[
            "--doctor",
            "--format",
            "json",
            "--compact",
            "--color",
            "always",
        ]))
        .expect("compact doctor JSON"),
        Command::Doctor {
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Always,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: false,
        }
    );
    #[cfg(feature = "update")]
    assert_eq!(
        parse(&args(&["--update-docs", "--compact"])).expect("document update"),
        Command::UpdateDocs { pretty: false }
    );
    #[cfg(feature = "update")]
    assert_eq!(
        parse(&args(&["--prune-docs", "--dry-run", "--compact"])).expect("document source prune"),
        Command::PruneDocs {
            pretty: false,
            dry_run: true,
        }
    );
    #[cfg(feature = "update")]
    assert_eq!(
        parse(&args(&["--update-tldr"])).expect("update"),
        Command::UpdateTldr { pretty: true }
    );
    assert_eq!(
        parse(&args(&["--protocol-version", "--compact"])).expect("version"),
        Command::ProtocolVersion { pretty: false }
    );
    assert_eq!(
        parse(&args(&["--schema", "request", "--compact"])).expect("schema"),
        Command::Schema {
            contract: SchemaContract::Request,
            pretty: false,
        }
    );
    assert_eq!(
        parse(&args(&["--schema", "doctor"])).expect("doctor schema"),
        Command::Schema {
            contract: SchemaContract::Doctor,
            pretty: true,
        }
    );
    assert_eq!(
        parse(&args(&["--schema", "tldr-update"])).expect("tldr update schema"),
        Command::Schema {
            contract: SchemaContract::TldrUpdate,
            pretty: true,
        }
    );
    #[cfg(feature = "mcp")]
    assert_eq!(parse(&args(&["--mcp"])).expect("MCP"), Command::Mcp);
}

#[test]
fn rejects_ambiguous_or_incompatible_inputs() {
    let cases = [
        vec!["git", "--format", "json", "--format", "text"],
        vec!["git", "--compact"],
        vec!["git", "--preserve-anchors", "--format", "json"],
        vec!["git", "--outline", "--preserve-anchors"],
        vec!["git", "--search", "branch", "--preserve-anchors"],
        vec!["--request-json", "git", "--format", "json"],
        vec!["--request-json", "--man-section", "1", "--format", "json"],
        vec!["--request-json", "--outline", "--format", "json"],
        vec!["git", "--outline", "--node", "1"],
        vec!["git", "--outline", "--search", "branch"],
        vec!["git", "--node", "1", "--search", "branch"],
        vec!["git", "--explain=--help", "--node", "help"],
        vec!["git", "--explain=--help", "--outline"],
        vec!["git", "--explain=--help", "--search", "help"],
        vec!["git", "--regex"],
        vec!["git", "--search", "branch", "--limit", "many"],
        vec!["git", "--node"],
        vec!["--man-section", "1"],
        vec!["--update-tldr", "--format", "json"],
        vec!["--update-docs", "--format", "json"],
        vec!["--prune-docs", "--format", "json"],
        vec!["--doctor", "--format", "markdown"],
        vec!["--doctor", "--compact"],
        vec!["--doctor", "--source", "team"],
        vec!["--dry-run"],
        vec!["git", "--source", "team", "--man-section", "1"],
        vec!["git", "--source", "team", "--manual"],
        vec!["git", "--manual", "--tldr"],
        vec!["git", "--tldr", "--node", "0"],
        vec!["git", "--tldr", "--display", "tui"],
        vec!["--input", "README.md", "--source", "team"],
        vec!["--schema", "request", "--format", "json"],
        vec!["--mcp", "git"],
        vec!["--mcp", "--format", "json"],
        vec!["--mcp", "--manual"],
        vec!["--mcp", "--tldr"],
        vec!["--mcp", "--update-tldr"],
        vec!["--update-tldr", "--preserve-anchors"],
        vec!["--input", "README.md", "--manual"],
        vec!["--input", "-", "--input-format", "markdown", "--manual"],
        vec!["--request-json", "--manual", "--format", "json"],
        vec!["--schema", "unknown"],
        vec!["update", "tldr"],
        vec!["git", "--json"],
        vec!["git", "--md"],
        vec!["git", "--markdown"],
        vec!["git", "--text"],
        vec!["git", "-s", "1"],
        vec!["git", "-n", "1"],
        vec!["--unknown", "git"],
    ];
    for values in cases {
        assert!(parse(&args(&values)).is_err(), "accepted {values:?}");
    }
}
