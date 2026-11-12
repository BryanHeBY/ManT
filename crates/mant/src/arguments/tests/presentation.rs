//! Presentation behavior contracts.

use super::*;
#[test]
fn defaults_direct_queries_to_automatic_text_reading() {
    assert_eq!(
        parse(&args(&["git"])).expect("query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot14,
                input: QueryInput::Document {
                    selector: "git".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Full {},
            }),
            presentation: OutputOptions::default(),
            pretty: true,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn removed_display_flags_offer_replacements_without_becoming_aliases() {
    for (old, new) in [("--ui", "tui"), ("--no-pager", "direct")] {
        let error = parse(&args(&["git", old])).expect_err("removed flag");
        assert!(
            error
                .to_string()
                .contains(&format!("{old} was removed; use --display {new}"))
        );
        assert!(
            parse(&args(&["--", old])).is_ok(),
            "a positional document name is not a flag"
        );
        assert!(parse(&args(&["git", &format!("--search={old}")])).is_ok());
    }
    for values in [
        vec!["git", "--display", "direct"],
        vec!["git", "--format", "json", "--display", "direct"],
        vec!["--schema", "all", "--display", "direct"],
    ] {
        assert!(parse(&args(&values)).is_ok());
    }
}

#[test]
fn noninteractive_input_modes_share_the_text_default() {
    for values in [
        vec!["--input", "-", "--input-format", "markdown"],
        vec!["--request-json"],
        vec!["--document", "git", "--explain=--help"],
        vec!["git", "--outline"],
        vec!["git", "--node", "1"],
        vec!["git", "--search", "help"],
    ] {
        assert!(
            matches!(
                parse(&args(&values)).expect("text query"),
                Command::Query {
                    presentation: OutputOptions {
                        format: None,
                        color: ColorMode::Auto,
                        display: DisplayMode::Auto,
                        markdown_math: None,
                    },
                    ..
                }
            ),
            "{values:?}"
        );
    }
}

#[test]
#[cfg(feature = "tui")]
fn parses_an_explicit_interactive_query_without_an_output_projection() {
    assert!(matches!(
        parse(&args(&["git", "--display", "tui"])).expect("interactive query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document { ref selector, .. },
                view: QueryView::Full {},
                ..
            }),
            presentation: OutputOptions { display: DisplayMode::Tui, .. },
            ..
        } if selector == "git"
    ));

    for conflicting in ["--outline", "--search=git", "--format=json"] {
        assert!(
            parse(&args(&["git", "--display", "tui", conflicting])).is_err(),
            "accepted {conflicting}"
        );
    }
}

#[test]
#[cfg(feature = "roff")]
fn parses_format_man_section_and_compact_json_options() {
    assert_eq!(
        parse(&args(&[
            "printf",
            "--man-section",
            "3",
            "--format",
            "json",
            "--compact",
        ]))
        .expect("query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot14,
                input: QueryInput::Document {
                    selector: "printf".to_owned(),
                    source: None,
                    manual_section: Some("3".to_owned()),
                },
                view: QueryView::Full {},
            }),
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: false,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );
    assert!(matches!(
        parse(&args(&["printf", "--source", "team"])).expect("source query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document {
                    ref source,
                    manual_section: None,
                    ..
                },
                ..
            }),
            ..
        } if source.as_deref() == Some("team")
    ));
}

#[test]
fn process_help_retains_styles_while_injected_help_stays_plain() {
    let arguments = args(&["--help", "--color", "always"]);
    let Command::Help(help) = parse(&arguments).expect("captured help") else {
        panic!("expected captured help")
    };
    assert!(!help.contains('\u{1b}'));

    let styled = parse_process(&arguments).expect_err("process help remains a clap display");
    assert_eq!(styled.kind(), clap::error::ErrorKind::DisplayHelp);
    assert!(styled.render().ansi().to_string().contains('\u{1b}'));
}

#[test]
fn manual_guidance_uses_shared_header_and_command_styles_after_usage() {
    let header = super::CLI_STYLES.get_header();
    let literal = super::CLI_STYLES.get_literal();
    for arguments in [args(&["--help"]), args(&["-h"]), vec![]] {
        let error = parse_process(&arguments).expect_err("help or missing action");
        let styled = error.render().ansi().to_string();
        assert!(styled.contains(&format!("{header}ManT manual:{header:#}")));
        assert!(styled.contains(&format!("{literal}mant mant{literal:#}")));
        assert!(styled.contains(&format!("{literal}mant mant --outline{literal:#}")));
        let plain = error.to_string();
        assert!(!plain.contains('\x1b'));
        assert!(plain.find("Usage:").unwrap() < plain.find("ManT manual:").unwrap());
    }
}

#[test]
fn color_policy_is_global_without_changing_deterministic_presentations() {
    assert!(matches!(
        parse(&args(&["git", "--format", "json", "--color", "always"]))
            .expect("JSON query with terminal color policy"),
        Command::Query {
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Always,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            ..
        }
    ));
    assert!(matches!(
        parse(&args(&["git", "--tldr", "--color", "never"])).expect("plain tldr query"),
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Never,
                ..
            },
            ..
        }
    ));

    assert_eq!(
        requested_color(&args(&["git", "--color=always"])),
        ColorMode::Always
    );
    assert_eq!(
        requested_color(&args(&["git", "--color", "never"])),
        ColorMode::Never
    );
    assert_eq!(
        requested_color(&args(&["--", "--color=always"])),
        ColorMode::Auto
    );
}

#[test]
fn tldr_footer_preserves_semantic_parts_and_is_shared_by_help_and_usage() {
    let header = super::CLI_STYLES.get_header();
    let literal = super::CLI_STYLES.get_literal();
    for arguments in [args(&["--help"]), args(&["-h"]), vec![]] {
        let error = parse_process(&arguments).expect_err("help or usage output");
        let styled = error.render().ansi().to_string();
        assert!(styled.contains(&format!("{header}TLDR:{header:#}")));
        let plain = error.to_string();
        assert!(!plain.contains('\x1b'));
        assert!(!plain.contains("Examples:"));
        assert!(!plain.contains("{{"));
        assert!(plain.trim_end().ends_with("Explore its outline"));
        let footer = plain.split_once("TLDR:\n").unwrap().1;
        let tldr = footer.split_once("\n\nManT manual:").unwrap().0;
        assert!(!tldr.contains("\n\n"), "TLDR examples stay compact");
        assert_eq!(tldr.lines().count(), super::help::examples().count() * 2);
        let mut remaining = footer;
        for (description, _, parts) in super::help::examples() {
            remaining = remaining.split_once(description).unwrap().1;
            let command: String = parts.iter().map(|(value, _)| *value).collect();
            remaining = remaining.split_once(&command).unwrap().1;
            for (value, placeholder) in *parts {
                if *placeholder {
                    assert!(!styled.contains(&format!("{literal}{value}{literal:#}")));
                } else {
                    assert!(styled.contains(&format!("{literal}{value}{literal:#}")));
                }
            }
        }
        assert!(remaining.contains("ManT manual:"));
    }
}

#[test]
fn help_is_side_effect_free_and_the_option_terminator_preserves_a_name() {
    for flag in ["--help", "-h"] {
        let help = parse(&args(&[flag])).expect("help");
        assert!(matches!(help, Command::Help(text) if text.contains("Usage: mant")));
    }
    assert_eq!(
        parse(&args(&["--", "--help"])).expect("query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot14,
                input: QueryInput::Document {
                    selector: "--help".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Full {},
            }),
            presentation: OutputOptions::default(),
            pretty: true,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn version_is_side_effect_free() {
    let version = parse(&args(&["--version"])).expect("version");
    assert!(
        matches!(version, Command::Help(text) if text == concat!("mant ", env!("CARGO_PKG_VERSION"), "\n"))
    );
}
