//! Help-only presentation of the self-manual's generated quick reference.
//!
//! The examples are packaged constants: help must not resolve documents, read
//! installed sources, or parse the manual during ordinary argument handling.

use std::fmt::Write as _;

use clap::builder::StyledStr;

use super::CLI_STYLES;

/// Description, required build features, and ordered command fragments.
type HelpExample = (
    &'static str,
    &'static [&'static str],
    &'static [(&'static str, bool)],
);

include!("help_tldr_generated.rs");

pub(super) fn examples() -> impl Iterator<Item = &'static HelpExample> {
    EXAMPLES.iter().filter(|(_, required, _)| {
        required
            .iter()
            .all(|feature| super::capabilities::enabled(feature))
    })
}

/// Keep quick-start tasks before the optional deeper-reading destination.
pub(super) fn footer() -> StyledStr {
    let header = CLI_STYLES.get_header();
    let literal = CLI_STYLES.get_literal();
    let mut text = format!("{header}TLDR:{header:#}\n");
    for (description, _, parts) in examples() {
        let _ = write!(text, "  {description}\n    ");
        for (value, placeholder) in *parts {
            if *placeholder {
                text.push_str(value);
            } else {
                let _ = write!(text, "{literal}{value}{literal:#}");
            }
        }
        text.push('\n');
    }
    let _ = write!(
        text,
        "\n{header}ManT manual:{header:#}\n\
         \x20 When the self manual is installed:\n\
         \x20   {literal}mant mant{literal:#}            Read the full manual\n\
         \x20   {literal}mant mant --outline{literal:#}  Explore its outline\n",
    );
    text.into()
}
