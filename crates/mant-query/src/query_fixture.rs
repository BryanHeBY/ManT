//! Test-only IR fixtures, independent of loader input policy and query execution.
use mant_ir::ResolvedContent;

pub(crate) fn markdown(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, mant_codec::MarkdownParseError> {
    let parsed = mant_codec::parse_markdown(source, source_path)?;
    Ok(ResolvedContent {
        label: "query fixture".to_owned(),
        address: None,
        document: Some(parsed.document),
        tldr: parsed.tldr,
    })
}
