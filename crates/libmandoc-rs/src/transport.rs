//! Shared native path/include preparation and Rust-managed byte decoding.
//! File dispatch and operation-specific error precedence remain at each caller.
use crate::{Compression, IncludePolicy, ParseError, ParseErrorKind};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::path::PathBuf;
use std::{ffi::CString, io, path::Path};
pub(crate) struct PreparedInput {
    pub(crate) path: CString,
    pub(crate) includes: IncludeSettings,
}
impl PreparedInput {
    pub(crate) fn new(path: &Path, policy: &IncludePolicy) -> Result<Self, ParseError> {
        let path_label = native_path(path)?;
        let includes = include_settings(policy, path)?;
        Ok(Self {
            path: path_label,
            includes,
        })
    }
}
pub(crate) fn native_path(path: &Path) -> Result<CString, ParseError> {
    path_label(path).map_err(|_| ParseError {
        path: path.to_path_buf(),
        kind: ParseErrorKind::InvalidPath,
        message: "manual source path contains a NUL byte".into(),
    })
}
pub(crate) fn prepare_bytes(
    source: &[u8],
    compression: Compression,
) -> io::Result<std::borrow::Cow<'_, [u8]>> {
    match compression {
        Compression::Auto if has_zstd_magic(source) => {
            crate::compression::decode_zstd(source).map(std::borrow::Cow::Owned)
        }
        Compression::Zstd => crate::compression::decode_zstd(source).map(std::borrow::Cow::Owned),
        Compression::Auto | Compression::Plain => Ok(std::borrow::Cow::Borrowed(source)),
    }
}
pub(crate) fn include_settings(
    policy: &IncludePolicy,
    source_path: &Path,
) -> Result<IncludeSettings, ParseError> {
    #[cfg(unix)]
    let _ = source_path;

    match policy {
        IncludePolicy::Deny => Ok(IncludeSettings {
            root: None,
            allow_includes: false,
        }),
        #[cfg(unix)]
        IncludePolicy::SourceTree => Ok(IncludeSettings {
            root: None,
            allow_includes: true,
        }),
        #[cfg(windows)]
        IncludePolicy::SourceTree => Err(unsupported_includes(source_path.to_path_buf())),
        IncludePolicy::Root(root) if root.as_os_str().is_empty() => Err(ParseError {
            path: root.clone(),
            kind: ParseErrorKind::InvalidPath,
            message: "manual include root is empty".into(),
        }),
        #[cfg(unix)]
        IncludePolicy::Root(root) => CString::new(root.as_os_str().as_bytes())
            .map(|root| IncludeSettings {
                root: Some(root),
                allow_includes: true,
            })
            .map_err(|_| ParseError {
                path: root.clone(),
                kind: ParseErrorKind::InvalidPath,
                message: "manual include root contains a NUL byte".into(),
            }),
        #[cfg(windows)]
        IncludePolicy::Root(root) => Ok(IncludeSettings {
            root: Some(root.clone()),
            allow_includes: true,
        }),
    }
}

pub(crate) struct IncludeSettings {
    #[cfg(unix)]
    pub(crate) root: Option<CString>,
    #[cfg(windows)]
    pub(crate) root: Option<PathBuf>,
    pub(crate) allow_includes: bool,
}

#[cfg(unix)]
pub(crate) fn path_label(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.as_os_str().as_bytes())
}

#[cfg(windows)]
pub(crate) fn path_label(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.to_string_lossy().as_bytes())
}

#[cfg(windows)]
fn unsupported_includes(path: PathBuf) -> ParseError {
    ParseError {
        path,
        kind: ParseErrorKind::Unsupported,
        message: "libmandoc-compatible source-tree inclusion is unavailable on Windows; use IncludePolicy::Root or SourceBundle"
            .into(),
    }
}

fn has_zstd_magic(source: &[u8]) -> bool {
    source.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_bytes_remain_borrowed_and_auto_decoding_is_magic_based() {
        let plain = b".TH TRANSPORT 1\n";
        for policy in [Compression::Plain, Compression::Auto] {
            assert!(matches!(
                prepare_bytes(plain, policy).unwrap(),
                std::borrow::Cow::Borrowed(_)
            ));
        }
        let encoded = zstd::stream::encode_all(&plain[..], 1).unwrap();
        assert_eq!(
            prepare_bytes(&encoded, Compression::Auto).unwrap().as_ref(),
            plain
        );
        assert_eq!(
            prepare_bytes(&encoded, Compression::Plain)
                .unwrap()
                .as_ref(),
            encoded
        );
        assert!(prepare_bytes(plain, Compression::Zstd).is_err());
    }

    #[test]
    fn source_path_validation_precedes_include_root_validation() {
        let invalid_root = IncludePolicy::Root(std::path::PathBuf::new());
        let error = PreparedInput::new(Path::new("bad\0path"), &invalid_root)
            .err()
            .unwrap();
        assert_eq!(error.message, "manual source path contains a NUL byte");
        let error = PreparedInput::new(Path::new("valid.1"), &invalid_root)
            .err()
            .unwrap();
        assert_eq!(error.message, "manual include root is empty");
    }
}
