//! Temporary P1 input adapter: one bounded standalone source to one Fixed IR.
//!
//! The normal roff loader remains unchanged until the annotated route replaces
//! it. This adapter borrows its source policy and hands the resulting document
//! to the same query and display consumers as ordinary CLI input.

use std::path::Path;

use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::ResolvedContent;

use crate::error::Failure;

pub(crate) fn load(path: &str) -> Result<ResolvedContent, Failure> {
    let source =
        mant_loader::read_standalone_manual_bytes(Path::new(path)).map_err(Failure::operational)?;
    let root = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Failure::usage("annotated preview requires a named roff file"))?;
    let mut bundle = SourceBundle::new();
    bundle.insert(root, source).map_err(Failure::operational)?;
    let document =
        mant_codec::annotated_fixed::project_annotated_manual(root, &bundle, InputFormat::Auto)
            .map_err(Failure::operational)?;
    let label = document
        .meta
        .names
        .first()
        .cloned()
        .or_else(|| document.meta.title.clone())
        .unwrap_or_else(|| root.to_owned());
    Ok(ResolvedContent {
        label,
        address: None,
        document: Some(document),
        tldr: None,
    })
}
