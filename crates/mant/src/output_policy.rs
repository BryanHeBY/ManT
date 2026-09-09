//! Pure output policy: content format, terminal colour, and display are independent.

use crate::arguments::{ColorMode, Command, DisplayMode, OutputOptions, QueryFormat, QuerySource};
use crate::error::Failure;
use mant_protocol::{QueryRequest, QueryView};

/// Capabilities are sampled once by the process; injected streams supply no terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalCapabilities {
    pub(crate) input: bool,
    pub(crate) output: bool,
    pub(crate) color: bool,
    pub(crate) kind: TerminalKind,
}

impl TerminalCapabilities {
    pub(crate) const fn detached() -> Self {
        Self {
            input: false,
            output: false,
            color: false,
            kind: TerminalKind::Dumb,
        }
    }

    fn interactive(self) -> bool {
        self.input && self.output && self.kind == TerminalKind::Capable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalKind {
    Capable,
    Dumb,
}

/// Display eligibility belongs to the operation, not the renderer or pager.
#[derive(Debug, Clone, Copy)]
struct DisplayCapabilities {
    reader: bool,
    pager: bool,
    auto_page: bool,
}

impl DisplayCapabilities {
    fn for_command(command: &Command) -> Self {
        match command {
            Command::Query { source, policy, .. } => {
                let stdin = matches!(
                    source,
                    QuerySource::StdinJson | QuerySource::InputStdin { .. }
                );
                let full = matches!(
                    source,
                    QuerySource::Arguments(QueryRequest {
                        view: QueryView::Full {},
                        ..
                    }) | QuerySource::ScopeArguments { view: None, .. }
                );
                Self {
                    reader: cfg!(feature = "tui")
                        && full
                        && *policy != mant_engine::LoadPolicy::TldrOnly,
                    pager: cfg!(feature = "pager") && !stdin,
                    auto_page: cfg!(feature = "pager") && !stdin,
                }
            }
            Command::Catalog { .. } => Self {
                reader: false,
                pager: cfg!(feature = "pager"),
                auto_page: cfg!(feature = "pager"),
            },
            Command::Doctor { .. } => Self {
                reader: false,
                pager: cfg!(feature = "pager"),
                auto_page: false,
            },
            _ => Self {
                reader: false,
                pager: false,
                auto_page: false,
            },
        }
    }
}

fn options(command: &Command) -> Option<OutputOptions> {
    match command {
        Command::Query { presentation, .. }
        | Command::Catalog { presentation, .. }
        | Command::Doctor { presentation, .. } => Some(*presentation),
        _ => None,
    }
}

/// Validate explicit requests independently of host capabilities or document loading.
pub(crate) fn validate(command: &Command) -> Result<(), Failure> {
    let Some(output) = options(command) else {
        return Ok(());
    };
    let capabilities = DisplayCapabilities::for_command(command);
    match output.display {
        DisplayMode::Tui if !cfg!(feature = "tui") => Err(Failure::usage(
            "this build does not support --display tui (requires the tui feature)",
        )),
        DisplayMode::Pager if !cfg!(feature = "pager") => Err(Failure::usage(
            "this build does not support --display pager (requires the pager feature)",
        )),
        DisplayMode::Tui if !capabilities.reader || output.format.is_some() => Err(Failure::usage(
            "--display tui requires full document reading without --format or stdin input",
        )),
        DisplayMode::Pager if !capabilities.pager || output.format() == QueryFormat::Json => {
            Err(Failure::usage(
                "--display pager requires textual output without stdin document or request input",
            ))
        }
        _ => Ok(()),
    }
}

/// Resolve once; execution receives only direct, pager, or TUI display and fixed colour.
pub(crate) fn resolve_process_presentation(
    command: &mut Command,
    terminal: TerminalCapabilities,
) -> Result<DisplayMode, Failure> {
    validate(command)?;
    let Some(mut output) = options(command) else {
        return Ok(DisplayMode::Direct);
    };
    let capabilities = DisplayCapabilities::for_command(command);
    output.display = match output.display {
        DisplayMode::Auto
            if terminal.interactive() && capabilities.reader && output.format.is_none() =>
        {
            DisplayMode::Tui
        }
        DisplayMode::Auto
            if terminal.interactive()
                && capabilities.auto_page
                && output.format() == QueryFormat::Text =>
        {
            DisplayMode::Pager
        }
        DisplayMode::Auto => DisplayMode::Direct,
        DisplayMode::Pager | DisplayMode::Tui if !terminal.interactive() => {
            return Err(Failure::usage(
                "interactive display requires a capable input and output terminal; use --display direct or auto",
            ));
        }
        explicit => explicit,
    };
    output.color = match output.color {
        _ if output.format() != QueryFormat::Text => ColorMode::Never,
        ColorMode::Auto if terminal.output && terminal.color => ColorMode::Always,
        ColorMode::Auto => ColorMode::Never,
        explicit => explicit,
    };
    match command {
        Command::Query { presentation, .. }
        | Command::Catalog { presentation, .. }
        | Command::Doctor { presentation, .. } => *presentation = output,
        _ => unreachable!("only commands with output options reach resolution"),
    }
    Ok(output.display)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arguments;

    fn command(args: &[&str]) -> Command {
        arguments::parse(&args.iter().map(ToString::to_string).collect::<Vec<_>>())
            .expect("valid output options")
    }

    fn terminal() -> TerminalCapabilities {
        TerminalCapabilities {
            input: true,
            output: true,
            color: true,
            kind: TerminalKind::Capable,
        }
    }

    #[test]
    #[cfg(all(feature = "tui", feature = "pager", feature = "update"))]
    fn display_policy_covers_every_operation_without_changing_formats() {
        for (args, expected) in [
            (vec!["git"], DisplayMode::Tui),
            (vec!["git", "--display", "direct"], DisplayMode::Direct),
            (vec!["git", "--format", "text"], DisplayMode::Pager),
            (vec!["git", "--outline"], DisplayMode::Pager),
            (vec!["git", "--node", "1"], DisplayMode::Pager),
            (vec!["git", "--explain=--help"], DisplayMode::Pager),
            (vec!["git", "--search", "help"], DisplayMode::Pager),
            (vec!["git", "--tldr"], DisplayMode::Pager),
            (vec!["--list"], DisplayMode::Pager),
            (vec!["--find", "git"], DisplayMode::Pager),
            (vec!["--doctor"], DisplayMode::Direct),
            (vec!["--doctor", "--display", "pager"], DisplayMode::Pager),
            (vec!["--request-json"], DisplayMode::Direct),
            (
                vec!["--input", "-", "--input-format", "markdown"],
                DisplayMode::Direct,
            ),
            (vec!["git", "--format", "json"], DisplayMode::Direct),
            (vec!["git", "--format", "markdown"], DisplayMode::Direct),
            (vec!["git", "--format", "man"], DisplayMode::Direct),
            (vec!["git", "--preserve-anchors"], DisplayMode::Direct),
            (
                vec!["git", "--format", "markdown", "--display", "pager"],
                DisplayMode::Pager,
            ),
            (
                vec!["git", "--format", "man", "--display", "pager"],
                DisplayMode::Pager,
            ),
            (vec!["--schema", "all"], DisplayMode::Direct),
            (vec!["--update-docs"], DisplayMode::Direct),
        ] {
            let mut command = command(&args);
            let format = options(&command).map(OutputOptions::format);
            assert_eq!(
                resolve_process_presentation(&mut command, terminal()).unwrap(),
                expected,
                "{args:?}"
            );
            assert_eq!(options(&command).map(OutputOptions::format), format);
        }
    }

    #[test]
    fn automatic_output_never_enters_an_incomplete_terminal() {
        for terminal in [
            TerminalCapabilities::detached(),
            TerminalCapabilities {
                input: false,
                ..terminal()
            },
            TerminalCapabilities {
                output: false,
                ..terminal()
            },
            TerminalCapabilities {
                kind: TerminalKind::Dumb,
                ..terminal()
            },
        ] {
            for args in [vec!["git"], vec!["git", "--outline"], vec!["--list"]] {
                assert_eq!(
                    resolve_process_presentation(&mut command(&args), terminal).unwrap(),
                    DisplayMode::Direct
                );
            }
            for display in ["pager", "tui"] {
                if (display == "pager" && !cfg!(feature = "pager"))
                    || (display == "tui" && !cfg!(feature = "tui"))
                {
                    continue;
                }
                assert!(
                    resolve_process_presentation(
                        &mut command(&["git", "--display", display]),
                        terminal
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn automatic_presentation_uses_only_compiled_capabilities() {
        let full = if cfg!(feature = "tui") {
            DisplayMode::Tui
        } else if cfg!(feature = "pager") {
            DisplayMode::Pager
        } else {
            DisplayMode::Direct
        };
        let text = if cfg!(feature = "pager") {
            DisplayMode::Pager
        } else {
            DisplayMode::Direct
        };
        for (args, expected) in [
            (vec!["demo"], full),
            (vec!["demo", "--outline"], text),
            (vec!["demo", "--format", "text"], text),
            (vec!["--list"], text),
            (vec!["--doctor"], DisplayMode::Direct),
            (
                vec!["--input", "-", "--input-format", "markdown"],
                DisplayMode::Direct,
            ),
            (vec!["demo", "--format", "json"], DisplayMode::Direct),
        ] {
            assert_eq!(
                resolve_process_presentation(&mut command(&args), terminal()).unwrap(),
                expected,
                "{args:?}"
            );
        }
        for (available, display) in [
            (cfg!(feature = "tui"), DisplayMode::Tui),
            (cfg!(feature = "pager"), DisplayMode::Pager),
        ] {
            let mut request = command(&["demo"]);
            let Command::Query { presentation, .. } = &mut request else {
                unreachable!()
            };
            presentation.display = display;
            assert_eq!(
                validate(&request).is_ok(),
                available,
                "typed request {display:?}"
            );
        }
    }

    #[test]
    fn incompatible_display_requests_fail_during_argument_validation() {
        for args in [
            vec!["git", "--format", "json", "--display", "pager"],
            vec!["--list", "--format", "json", "--display", "pager"],
            vec!["--doctor", "--format", "json", "--display", "pager"],
            vec!["--list", "--display", "tui"],
            vec!["--doctor", "--display", "tui"],
            vec!["git", "--outline", "--display", "tui"],
            vec!["--request-json", "--display", "pager"],
            vec!["--request-json", "--display", "tui"],
            vec![
                "--input",
                "-",
                "--input-format",
                "roff",
                "--display",
                "pager",
            ],
            vec!["--update-docs", "--display", "pager"],
            vec!["--protocol-version", "--display", "tui"],
            vec!["--mcp", "--display", "direct"],
            vec!["git", "--display", "direct", "--display", "tui"],
        ] {
            assert!(
                arguments::parse(&args.iter().map(ToString::to_string).collect::<Vec<_>>())
                    .is_err(),
                "{args:?}"
            );
        }
    }
}
