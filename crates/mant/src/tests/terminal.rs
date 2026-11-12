//! Terminal behavior contracts.

use super::*;
#[test]
fn terminal_capabilities_resolve_interactivity_and_text_colour() {
    let terminal = TerminalCapabilities {
        input: true,
        output: true,
        color: true,
        kind: TerminalKind::Capable,
    };
    let full_display = if cfg!(feature = "tui") {
        DisplayMode::Tui
    } else if cfg!(feature = "pager") {
        DisplayMode::Pager
    } else {
        DisplayMode::Direct
    };
    let partial_display = if cfg!(feature = "pager") {
        DisplayMode::Pager
    } else {
        DisplayMode::Direct
    };
    let mut terminal_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
    resolve_process_presentation(&mut terminal_query, terminal).expect("terminal query");
    assert!(matches!(
        terminal_query,
        Command::Query {
            presentation: OutputOptions {
                display,
                ..
            },
            ..
        } if display == full_display
    ));

    let mut redirected_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
    resolve_process_presentation(
        &mut redirected_query,
        TerminalCapabilities {
            output: false,
            ..terminal
        },
    )
    .expect("redirected query");
    assert!(matches!(
        redirected_query,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Never,
                display: DisplayMode::Direct,
                markdown_math: None,
            },
            ..
        }
    ));

    let mut outline =
        arguments::parse(&["git".to_owned(), "--outline".to_owned()]).expect("outline query");
    resolve_process_presentation(&mut outline, terminal).expect("outline remains non-interactive");
    assert!(matches!(
        outline,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Always,
                display,
                markdown_math: None,
            },
            ..
        } if display == partial_display
    ));

    let mut tldr = arguments::parse(&["git".to_owned(), "--tldr".to_owned()]).expect("tldr query");
    resolve_process_presentation(&mut tldr, terminal).expect("tldr remains non-interactive");
    assert!(matches!(
        tldr,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Always,
                ..
            },
            ..
        }
    ));
}

#[test]
#[cfg(feature = "tui")]
fn explicit_interactive_queries_require_both_terminal_streams() {
    for terminal in [
        TerminalCapabilities {
            input: false,
            output: true,
            color: true,
            kind: TerminalKind::Capable,
        },
        TerminalCapabilities {
            input: true,
            output: false,
            color: true,
            kind: TerminalKind::Capable,
        },
        TerminalCapabilities {
            input: true,
            output: true,
            color: false,
            kind: TerminalKind::Dumb,
        },
    ] {
        let mut command =
            arguments::parse(&["git".to_owned(), "--display".to_owned(), "tui".to_owned()])
                .expect("UI query");
        let error = resolve_process_presentation(&mut command, terminal)
            .expect_err("incomplete terminal must fail");
        assert!(error.message().contains("interactive display requires"));
    }
}

#[test]
fn dumb_term_uses_copyable_output_for_automatic_queries() {
    let mut command = arguments::parse(&["git".to_owned()]).expect("automatic query");
    resolve_process_presentation(
        &mut command,
        TerminalCapabilities {
            input: true,
            output: true,
            color: false,
            kind: TerminalKind::Dumb,
        },
    )
    .expect("dumb terminal falls back to output");

    assert!(matches!(
        command,
        Command::Query {
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Never,
                display: DisplayMode::Direct,
                markdown_math: None,
            },
            ..
        }
    ));
}

#[test]
fn automatic_full_text_retains_explicit_and_detected_colour() {
    for (flags, input, output, capable_color, expected) in [
        (vec![], false, true, true, ColorMode::Always),
        (vec![], true, false, true, ColorMode::Never),
        (vec![], false, true, false, ColorMode::Never),
        (
            vec!["--color", "always"],
            true,
            false,
            false,
            ColorMode::Always,
        ),
        (
            vec!["--color", "never"],
            false,
            true,
            true,
            ColorMode::Never,
        ),
    ] {
        let mut args = vec!["demo".to_owned()];
        args.extend(flags.into_iter().map(str::to_owned));
        let mut command = arguments::parse(&args).expect("full query");
        resolve_process_presentation(
            &mut command,
            TerminalCapabilities {
                input,
                output,
                color: capable_color,
                kind: TerminalKind::Capable,
            },
        )
        .expect("resolve text colour");
        assert!(matches!(command, Command::Query {
            presentation: OutputOptions {
                format: None, color, ..
            }, ..
        } if color == expected));
    }
}

#[test]
fn catalog_paging_requires_text_and_a_complete_non_dumb_terminal() {
    let should_page_catalog = |command: &Command, terminal| {
        resolve_process_presentation(&mut command.clone(), terminal).expect("resolve catalog")
            == DisplayMode::Pager
    };
    let terminal = TerminalCapabilities {
        input: true,
        output: true,
        color: false,
        kind: TerminalKind::Capable,
    };
    let list = arguments::parse(&["--list".to_owned()]).expect("catalog list");
    assert_eq!(
        should_page_catalog(&list, terminal),
        cfg!(feature = "pager")
    );

    let direct = arguments::parse(&[
        "--list".to_owned(),
        "--display".to_owned(),
        "direct".to_owned(),
    ])
    .expect("direct catalog list");
    assert!(!should_page_catalog(&direct, terminal));

    let json = arguments::parse(&[
        "--find".to_owned(),
        "git".to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ])
    .expect("catalog JSON");
    assert!(!should_page_catalog(&json, terminal));
    assert!(!should_page_catalog(
        &list,
        TerminalCapabilities {
            output: false,
            ..terminal
        }
    ));
    assert!(!should_page_catalog(
        &list,
        TerminalCapabilities {
            kind: TerminalKind::Dumb,
            ..terminal
        }
    ));
}

#[test]
fn catalog_colour_preserves_records_and_newlines() {
    let host = FakeHost::new();
    let catalog = host
        .discover(&CatalogQuery::default())
        .expect("fake catalog");
    for grouped in [false, true] {
        let plain = super::presentation::render_catalog_output(&catalog, grouped, false);
        let colored = super::presentation::render_catalog_output(&catalog, grouped, true);
        assert!(colored.contains('\x1b'));
        // Catalog uses only SGR styling; stripping it must retain tabs and final newlines.
        let mut visible = String::new();
        let mut chars = colored.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for ch in chars.by_ref() {
                    if ch == 'm' {
                        break;
                    }
                }
            } else {
                visible.push(ch);
            }
        }
        assert_eq!(visible, plain);
    }
}
