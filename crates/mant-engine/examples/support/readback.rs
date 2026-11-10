//! Transport and readback only; each observer owns its assertions and output.

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::ResolvedContent;
use mant_protocol::QueryBundle;

pub(super) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) struct WireObservation {
    pub(super) json: String,
    pub(super) equivalent: bool,
    pub(super) content: ResolvedContent,
}

pub(super) fn transport(content: &ResolvedContent) -> Result<WireObservation> {
    let json = serde_json::to_string(&QueryBundle::from(content))?;
    let decoded: QueryBundle = serde_json::from_str(&json)?;
    let equivalent = json == serde_json::to_string(&decoded)?;
    Ok(WireObservation {
        json,
        equivalent,
        content: decoded.into(),
    })
}

/// Start each export route from the original wire document. The second
/// round uses what the observer actually accepted from its first readback.
pub(super) fn visit_readbacks(
    original: &ResolvedContent,
    mut observe: impl FnMut(&str, &ResolvedContent, &str) -> Result<ResolvedContent>,
) -> Result<()> {
    for (options, routes) in [
        (MarkdownOptions::default(), ["default-1", "default-2"]),
        (
            MarkdownOptions::ADDRESSABLE,
            ["addressable-1", "addressable-2"],
        ),
    ] {
        let mut previous = None;
        for route in routes {
            let markdown =
                render_markdown_with_options(previous.as_ref().unwrap_or(original), options);
            let parsed = mant_loader::load_markdown_text(&markdown, None)?;
            previous = Some(observe(route, &parsed, &markdown)?);
        }
    }
    Ok(())
}
