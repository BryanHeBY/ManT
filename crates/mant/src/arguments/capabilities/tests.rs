use clap::{CommandFactory, ValueEnum};

use super::*;
use crate::arguments::{CatalogKindMode, Cli, DisplayMode, InputFormatMode, parse};

fn parses(args: &[&str]) -> bool {
    parse(&args.iter().map(ToString::to_string).collect::<Vec<_>>()).is_ok()
}

#[test]
fn clap_capability_surface_has_no_dangling_relationships() {
    Cli::command().debug_assert();
    let command = Cli::command();
    let flags = command
        .get_arguments()
        .filter_map(clap::Arg::get_long)
        .collect::<Vec<_>>();
    for (feature, flag, args) in [
        ("roff", "manual", vec!["demo", "--manual"]),
        ("roff", "man-section", vec!["demo", "--man-section", "1"]),
        ("update", "update-docs", vec!["--update-docs"]),
        ("update", "update-tldr", vec!["--update-tldr"]),
        ("update", "prune-docs", vec!["--prune-docs"]),
        ("update", "dry-run", vec!["--prune-docs", "--dry-run"]),
        ("mcp", "mcp", vec!["--mcp"]),
    ] {
        assert_eq!(flags.contains(&flag), enabled(feature), "{flag}");
        assert_eq!(parses(&args), enabled(feature), "{args:?}");
    }
    // Formerly conflicting identifiers must not break basic modes when skipped.
    for args in [
        vec!["demo", "--source", "team"],
        vec!["demo", "--tldr"],
        vec!["--doctor"],
        vec!["--request-json"],
        vec!["demo", "--preserve-anchors"],
        vec!["--schema", "all"],
        vec!["--schema", "tldr-update"],
    ] {
        assert!(parses(&args), "{args:?}");
    }
}

#[test]
fn value_enumeration_and_parser_share_the_same_capabilities() {
    for (feature, args, value) in [
        ("tui", vec!["demo", "--display", "tui"], DisplayMode::Tui),
        (
            "pager",
            vec!["demo", "--display", "pager"],
            DisplayMode::Pager,
        ),
    ] {
        assert_eq!(value.to_possible_value().is_some(), enabled(feature));
        assert_eq!(parses(&args), enabled(feature));
    }
    assert_eq!(
        InputFormatMode::value_variants()
            .iter()
            .filter_map(ValueEnum::to_possible_value)
            .any(|value| value.get_name() == "roff"),
        enabled("roff")
    );
    assert!(CatalogKindMode::Manual.to_possible_value().is_some());
    assert_eq!(
        parses(&["--input", "probe.1", "--input-format", "roff"]),
        enabled("roff")
    );
    assert!(parses(&["--list", "--kind", "manual"]));
    assert!(parses(&[
        "--input",
        "probe.md",
        "--input-format",
        "markdown"
    ]));
    assert!(parses(&["demo", "--format", "man"]));
}

#[test]
fn full_build_usage_is_byte_identical_and_partial_usage_omits_unavailable_actions() {
    if cfg!(all(feature = "roff", feature = "update", feature = "mcp")) {
        assert_eq!(usage(), crate::arguments::CLI_USAGE);
    }
    assert_eq!(usage().contains("mant --mcp"), enabled("mcp"));
    assert_eq!(usage().contains("mant --update-docs"), enabled("update"));
    assert_eq!(usage().contains("<MAN_SECTION>"), enabled("roff"));
}

#[test]
fn help_examples_use_explicit_capability_metadata() {
    let footer = crate::arguments::help::footer().to_string();
    for (description, requirements, _) in crate::arguments::help::EXAMPLES {
        assert_eq!(
            footer.contains(description),
            requirements.iter().all(|feature| enabled(feature)),
            "{description}"
        );
    }
    assert!(footer.contains("Validate a local Markdown manual"));
    assert!(footer.contains("mant mant --outline"));
    assert!(!footer.contains("requires="));
}
