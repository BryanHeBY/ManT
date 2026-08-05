//! Defines the temporary Rust TUI command line while it remains a separate binary.

use std::path::Path;

use clap::{CommandFactory, Parser, error::ErrorKind};
use mant_ast::QueryInput;
use mant_core::QueryPolicy;

/// Inputs accepted by the interactive document reader.
#[derive(Debug, Parser)]
#[command(
    name = "mantui-rs",
    about = "Explore local manuals and Markdown in a structured terminal UI",
    long_about = "Explore complete local Unix manuals and Markdown documents with a hierarchy-aware sidebar, full-page search, page-local references, and optional tldr quick references.",
    after_help = "Examples:\n  mantui-rs git\n  mantui-rs README.md\n  mantui-rs printf --section 3\n  mantui-rs --force-libmandoc tar"
)]
pub(crate) struct Arguments {
    /// Manual topic or local Markdown path.
    #[arg(required = true, num_args = 1.., value_name = "TOPIC|MARKDOWN")]
    input: Vec<String>,

    /// Select a manual section such as 1 or 3p.
    #[arg(short, long, value_name = "SECTION")]
    section: Option<String>,

    /// Require direct libmandoc output and print its diagnostics.
    #[arg(long, conflicts_with = "force_groff")]
    force_libmandoc: bool,

    /// Use man -Thtml and the groff HTML parser.
    #[arg(long)]
    force_groff: bool,
}

/// A validated query and its host-only parser policy.
#[derive(Debug)]
pub(crate) struct Invocation {
    pub(crate) input: QueryInput,
    pub(crate) policy: QueryPolicy,
}

impl Arguments {
    pub(crate) fn invocation(self) -> Result<Invocation, clap::Error> {
        let value = self.input.join(" ");
        let value = value.trim();
        if value.is_empty() {
            return Err(Self::command().error(
                ErrorKind::InvalidValue,
                "a manual topic or Markdown path is required",
            ));
        }

        let markdown = is_markdown_path(value);
        if markdown && self.section.is_some() {
            return Err(Self::command().error(
                ErrorKind::ArgumentConflict,
                "--section applies only to manual topics",
            ));
        }
        if markdown && (self.force_libmandoc || self.force_groff) {
            return Err(Self::command().error(
                ErrorKind::ArgumentConflict,
                "manual renderer policies do not apply to Markdown input",
            ));
        }

        let section = self.section.map(|section| section.trim().to_owned());
        if section.as_deref() == Some("") {
            return Err(
                Self::command().error(ErrorKind::InvalidValue, "manual section must not be empty")
            );
        }

        Ok(Invocation {
            input: if markdown {
                QueryInput::MarkdownFile {
                    path: value.to_owned(),
                }
            } else {
                QueryInput::Manual {
                    topic: value.to_owned(),
                    section,
                }
            },
            policy: QueryPolicy {
                force_libmandoc: self.force_libmandoc,
                force_groff: self.force_groff,
            },
        })
    }
}

fn is_markdown_path(input: &str) -> bool {
    let path = Path::new(input);
    path.is_file()
        || path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
        || input.starts_with('.')
        || input.contains('/')
        || input.contains('\\')
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use mant_ast::QueryInput;
    use mant_core::QueryPolicy;

    use super::Arguments;

    fn parse(values: &[&str]) -> super::Invocation {
        Arguments::try_parse_from(values)
            .expect("parse arguments")
            .invocation()
            .expect("validate arguments")
    }

    #[test]
    fn parses_manual_sections_and_parser_policies() {
        let invocation = parse(&["mantui-rs", "printf", "--section", "3"]);
        assert_eq!(
            invocation.input,
            QueryInput::Manual {
                topic: "printf".to_owned(),
                section: Some("3".to_owned()),
            }
        );
        assert_eq!(invocation.policy, QueryPolicy::default());

        let invocation = parse(&["mantui-rs", "--force-libmandoc", "tar"]);
        assert!(invocation.policy.force_libmandoc);
        assert!(!invocation.policy.force_groff);
    }

    #[test]
    fn identifies_markdown_paths_without_touching_the_file_system() {
        for path in ["README.md", "guide.markdown", "docs/guide", r"docs\guide"] {
            assert_eq!(
                parse(&["mantui-rs", path]).input,
                QueryInput::MarkdownFile {
                    path: path.to_owned(),
                }
            );
        }
    }

    #[test]
    fn accepts_a_dash_prefixed_topic_after_the_option_terminator() {
        assert_eq!(
            parse(&["mantui-rs", "--", "-topic"]).input,
            QueryInput::Manual {
                topic: "-topic".to_owned(),
                section: None,
            }
        );
    }

    #[test]
    fn rejects_conflicting_or_inapplicable_options() {
        assert!(
            Arguments::try_parse_from(["mantui-rs", "tar", "--force-libmandoc", "--force-groff"])
                .is_err()
        );

        let markdown = Arguments::try_parse_from(["mantui-rs", "README.md", "--section", "1"])
            .expect("syntactically valid")
            .invocation()
            .expect_err("section does not apply to Markdown");
        assert!(markdown.to_string().contains("only to manual topics"));
    }
}
