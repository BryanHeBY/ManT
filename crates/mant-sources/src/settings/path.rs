//! Lexical directory policy, independent of the host running a Windows probe.

use std::{ffi::OsStr, path::Path};

use super::SourceConfigError;

pub(super) const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy)]
enum WindowsForm {
    Relative,
    Absolute { root_end: usize, verbatim: bool },
    Invalid,
}

/// An opaque, host-local comparison key, never a file-operation path or proof
/// of installation ownership. Native encoded bytes are retained without loss.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowsPathKey(Vec<u8>);

/// Whether a bounded configuration path is fully qualified for the selected host.
/// Windows accepts disk and complete UNC paths, including their backslash-only
/// extended forms, but not drive/root-relative paths or device namespaces.
#[must_use]
pub fn is_absolute_configuration_path(path: &Path, windows: bool) -> bool {
    valid(path.as_os_str())
        && if windows {
            matches!(windows_form(path.as_os_str()), WindowsForm::Absolute { .. })
        } else {
            path.as_os_str().as_encoded_bytes().starts_with(b"/")
        }
}

/// Whether a bounded path is an ordinary relative path rather than a Windows
/// drive/root-relative or namespace path. It must be anchored before use.
#[must_use]
pub fn is_relative_configuration_path(path: &Path, windows: bool) -> bool {
    valid(path.as_os_str())
        && if windows {
            matches!(windows_form(path.as_os_str()), WindowsForm::Relative)
        } else {
            !path.as_os_str().as_encoded_bytes().starts_with(b"/")
        }
}

pub(super) fn resolve(
    path: &Path,
    base: Option<&Path>,
    windows: bool,
) -> Result<std::path::PathBuf, SourceConfigError> {
    if !valid(path.as_os_str()) {
        return Err(problem(
            "must be nonempty, contain no controls and fit in 4096 encoded bytes",
        ));
    }
    if is_absolute_configuration_path(path, windows) {
        return Ok(path.to_owned());
    }
    if windows && !is_relative_configuration_path(path, true) {
        return Err(problem(
            "must be a fully qualified Windows path or an ordinary relative path",
        ));
    }
    let base =
        base.ok_or_else(|| problem("requires a configuration directory for relative paths"))?;
    let resolved = base.join(path);
    if !is_absolute_configuration_path(&resolved, windows) {
        return Err(problem(
            "is invalid or exceeds 4096 encoded bytes after resolving its base",
        ));
    }
    Ok(resolved)
}

/// Compare ordinary Windows paths without accessing the filesystem. Separators,
/// ASCII case and dot components follow ordinary Windows lexical resolution;
/// roots are retained. Extended paths keep their literal components and namespace.
/// Invalid paths have no key. This is not a filesystem-identity or ownership check.
#[must_use]
pub fn windows_path_key(path: &Path) -> Option<WindowsPathKey> {
    if !valid(path.as_os_str()) {
        return None;
    }
    let WindowsForm::Absolute { root_end, verbatim } = windows_form(path.as_os_str()) else {
        return None;
    };
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut key = bytes[..root_end].to_vec();
    if !verbatim {
        for byte in &mut key {
            if separator(*byte) {
                *byte = b'\\';
            }
        }
    }
    if !key.ends_with(b"\\") {
        key.push(b'\\');
    }
    if verbatim {
        let suffix = &bytes[root_end..];
        let suffix = if bytes[root_end - 1] == b'\\' {
            suffix
        } else {
            suffix.strip_prefix(b"\\").unwrap_or(suffix)
        };
        let minimum_root = key.len();
        key.extend_from_slice(suffix);
        while key.len() > minimum_root && key.ends_with(b"\\") {
            key.pop();
        }
        key.make_ascii_lowercase();
        return Some(WindowsPathKey(key));
    }
    let mut components = Vec::new();
    for component in bytes[root_end..].split(|byte| separator(*byte)) {
        if component.is_empty() {
            continue;
        }
        if component == b"." {
            continue;
        }
        if component == b".." {
            components.pop();
            continue;
        }
        components.push(component);
    }
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            key.push(b'\\');
        }
        key.extend_from_slice(component);
    }
    key.make_ascii_lowercase();
    Some(WindowsPathKey(key))
}

fn valid(value: &OsStr) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PATH_BYTES
        && !value.to_string_lossy().chars().any(char::is_control)
}

fn windows_form(value: &OsStr) -> WindowsForm {
    let bytes = value.as_encoded_bytes();
    if let Some(rest) = bytes.strip_prefix(b"\\\\?\\") {
        // Extended paths do not perform ordinary slash/dot normalization.
        if rest.contains(&b'/')
            || rest
                .split(|byte| *byte == b'\\')
                .any(|part| part == b"." || part == b"..")
        {
            return WindowsForm::Invalid;
        }
        let root_end = if disk_root(rest) {
            Some(7)
        } else {
            rest.strip_prefix(b"UNC\\").and_then(|_| unc_root(bytes, 8))
        };
        return root_end.map_or(WindowsForm::Invalid, |root_end| WindowsForm::Absolute {
            root_end,
            verbatim: true,
        });
    }
    if disk_root(bytes) {
        return WindowsForm::Absolute {
            root_end: 3,
            verbatim: false,
        };
    }
    if bytes
        .get(..2)
        .is_some_and(|start| start.iter().all(|byte| separator(*byte)))
    {
        return unc_root(bytes, 2).map_or(WindowsForm::Invalid, |root_end| WindowsForm::Absolute {
            root_end,
            verbatim: false,
        });
    }
    if bytes.first().is_some_and(|byte| separator(*byte)) || bytes.contains(&b':') {
        WindowsForm::Invalid
    } else {
        WindowsForm::Relative
    }
}

fn disk_root(bytes: &[u8]) -> bool {
    bytes.get(..3).is_some_and(|prefix| {
        prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && separator(prefix[2])
    })
}

fn unc_root(bytes: &[u8], start: usize) -> Option<usize> {
    let rest = bytes.get(start..)?;
    let server_end = rest.iter().position(|byte| separator(*byte))?;
    let server = &rest[..server_end];
    let share = &rest[server_end + 1..];
    let share_end = share
        .iter()
        .position(|byte| separator(*byte))
        .unwrap_or(share.len());
    if [server, &share[..share_end]]
        .iter()
        .any(|part| part.is_empty() || matches!(*part, b"." | b".." | b"?"))
    {
        return None;
    }
    Some(start + server_end + 1 + share_end)
}

const fn separator(byte: u8) -> bool {
    byte == b'/' || byte == b'\\'
}

fn problem(detail: &str) -> SourceConfigError {
    SourceConfigError::new(format!("configuration path {detail}"))
}

#[cfg(test)]
mod tests;
