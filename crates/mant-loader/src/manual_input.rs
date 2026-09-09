//! Product input policy: prepare bounded bytes, then invoke the pure roff codec.

mod error;
mod source;

use crate::ManualPage;
use libmandoc_rs::ParseReport;
use mant_codec::{parse_roff_bytes, parse_roff_bytes_with_report, redirect_target};
use mant_ir::Document;
use source::{load_manual_source, resolve_manual_redirects};
use std::path::Path;

pub use error::{ManualError, ManualErrorKind};
pub use source::MAX_MANUAL_BYTES;

/// Parse and normalize one standalone man or mdoc source file.
///
/// This safe convenience entry point does not expand `.so` redirects because
/// no caller-approved manual hierarchy accompanies a bare path. `ManT`'s indexed
/// query path uses [`parse_manual_page`] instead.
///
/// # Errors
///
/// Returns [`ManualError`] when the source cannot be opened, decoded, or parsed.
pub fn parse_manual_source(path: &Path) -> Result<Document, ManualError> {
    parse_manual_source_with_report(path).map(|(document, _)| document)
}

/// Parse through the production file pipeline, retaining its native witness.
///
/// Both results describe the same bounded, decompressed and control-masked
/// input, with the same deny-include policy as [`parse_manual_source`]. This
/// avoids a second parser invocation when consumers audit native-to-IR facts.
/// The native report is fully owned and no source file is read during lowering.
///
/// # Errors
///
/// Returns [`ManualError`] on source, decompression, redirect or parser failure.
pub fn parse_manual_source_with_report(
    path: &Path,
) -> Result<(Document, ParseReport), ManualError> {
    let loaded = load_manual_source(path)?;
    reject_standalone_redirect(path, &loaded.source)?;
    parse_roff_bytes_with_report(path, &loaded.source).map_err(ManualError::from)
}

/// Parse one already bounded, uncompressed standalone roff input.
///
/// This is the standard-input counterpart of [`parse_manual_source`]. It does
/// not expand `.so` redirects and never reads another file.
///
/// # Errors
///
/// Returns [`ManualError`] when libmandoc rejects the input.
pub fn parse_manual_bytes(path: &Path, source: &[u8]) -> Result<Document, ManualError> {
    reject_standalone_redirect(path, source)?;
    parse_roff_bytes(path, source).map_err(ManualError::from)
}

fn reject_standalone_redirect(path: &Path, source: &[u8]) -> Result<(), ManualError> {
    if redirect_target(source)
        .map_err(|error| ManualError::redirect(path, error.to_string()))?
        .is_some()
    {
        return Err(ManualError::redirect(
            path,
            "standalone .so redirects require MANPATH discovery and cannot be followed by --input",
        ));
    }
    Ok(())
}

/// Parse an indexed manual, resolving `.so` redirects against its discovered
/// manual hierarchy without falling back to the process working directory.
///
/// # Errors
///
/// Returns [`ManualError`] when the source cannot be opened, decoded, or parsed.
pub fn parse_manual_page(page: &ManualPage) -> Result<Document, ManualError> {
    let resolved = resolve_manual_redirects(page)?;
    let mut document = parse_roff_bytes(&page.path, &resolved.source)?;
    if let Some(alias_target) = resolved.alias_target {
        document.meta.alias_target = Some(alias_target);
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_inputs_reject_redirect_only_so_pages() {
        let path = Path::new("stdin");
        let error = parse_manual_bytes(path, b".so man1/target.1\n")
            .expect_err("standalone input must not follow another file");
        assert_eq!(error.kind(), ManualErrorKind::Redirect);
        assert_eq!(error.path(), path);
        assert!(error.to_string().contains("require MANPATH discovery"));
    }

    #[test]
    fn standalone_malformed_redirect_keeps_original_path_and_failure_kind() {
        for source in [
            b".so\n".as_slice(),
            b".so first second\n",
            b".so bad\0name\n",
        ] {
            let path = Path::new("original display name.1");
            let error = parse_manual_bytes(path, source).expect_err("reject malformed alias");
            assert_eq!(error.kind(), ManualErrorKind::Redirect);
            assert_eq!(error.path(), path);
            assert_eq!(
                error.message(),
                "manual .so redirect must contain exactly one target path"
            );
        }
    }
}
