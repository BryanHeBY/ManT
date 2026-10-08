//! One bounded, read-only path grammar for personal TOML and man.conf.

use super::{SourceConfigError, environment_value, home};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
};

const MAX_PATH_BYTES: usize = 4096;

/// Expand an authored configuration path without resolving relative paths.
///
/// Leading `~`, `~/` and `~\` select the shared home. `%NAME%` inserts a process
/// environment value once, and `%%` is a literal percent sign. Inserted values
/// are never rescanned, globbed or interpreted by a shell. Windows environment
/// names match ASCII case-insensitively; Unix names match exactly. Native path
/// bytes are preserved. Authored and expanded paths are bounded to 4096 bytes.
///
/// # Errors
/// Reports empty/control-containing or oversized paths, missing home, undefined
/// or empty variables, and unterminated percent expressions.
pub fn expand_configuration_path(
    value: &str,
    environment: &BTreeMap<OsString, OsString>,
    windows: bool,
) -> Result<PathBuf, SourceConfigError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(problem(
            "must be nonempty and contain no control characters",
        ));
    }
    if value.len() > MAX_PATH_BYTES {
        return Err(problem("exceeds the 4096-byte limit"));
    }
    let mut output = OsString::new();
    let mut remaining = value;
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let root = home(environment, windows)
            .ok_or_else(|| problem("requires an absolute HOME or Windows USERPROFILE"))?;
        if value == "~" {
            append(&mut output, root.as_os_str())?;
            remaining = "";
        } else {
            append(&mut output, root.join("").as_os_str())?;
            remaining = &value[2..];
        }
    }
    while let Some(start) = remaining.find('%') {
        append(&mut output, OsStr::new(&remaining[..start]))?;
        remaining = &remaining[start + 1..];
        if let Some(literal) = remaining.strip_prefix('%') {
            append(&mut output, OsStr::new("%"))?;
            remaining = literal;
            continue;
        }
        let end = remaining
            .find('%')
            .ok_or_else(|| problem("contains an unterminated %NAME% expression"))?;
        let name = &remaining[..end];
        let replacement = environment_value(environment, name, windows)
            .ok_or_else(|| problem(&format!("references undefined or empty variable %{name}%")))?;
        append(&mut output, replacement)?;
        remaining = &remaining[end + 1..];
    }
    append(&mut output, OsStr::new(remaining))?;
    Ok(PathBuf::from(output))
}

fn append(output: &mut OsString, value: &OsStr) -> Result<(), SourceConfigError> {
    if value.len() > MAX_PATH_BYTES.saturating_sub(output.len()) {
        return Err(problem("exceeds the 4096-byte limit"));
    }
    if value.to_string_lossy().chars().any(char::is_control) {
        return Err(problem("contains control characters after expansion"));
    }
    output.push(value);
    Ok(())
}

fn problem(detail: &str) -> SourceConfigError {
    SourceConfigError::new(format!("configuration path {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_forms_use_the_shared_home_and_windows_fallback() {
        for windows in [false, true] {
            let root = if windows {
                r"C:\Users\Alice"
            } else {
                "/home/alice"
            };
            let environment = BTreeMap::from([("HOME".into(), root.into())]);
            for (value, expected) in [
                ("~", PathBuf::from(root)),
                ("~/manuals", PathBuf::from(root).join("manuals")),
                (r"~\manuals", PathBuf::from(root).join("manuals")),
            ] {
                assert_eq!(
                    expand_configuration_path(value, &environment, windows).unwrap(),
                    expected
                );
            }
        }
        let environment = BTreeMap::from([
            ("HOME".into(), "relative".into()),
            ("UserProfile".into(), r"C:\Users\Fallback".into()),
        ]);
        assert_eq!(
            expand_configuration_path("~", &environment, true).unwrap(),
            PathBuf::from(r"C:\Users\Fallback")
        );
        assert!(expand_configuration_path("~", &environment, false).is_err());
    }

    #[test]
    fn authored_markers_expand_once_without_rescanning_inserted_values() {
        let environment = BTreeMap::from([
            ("HOME".into(), "/home/%LITERAL%".into()),
            ("DIR".into(), "%NESTED%".into()),
            ("NESTED".into(), "must-not-expand".into()),
            ("TILDE".into(), "~/literal".into()),
        ]);
        assert_eq!(
            expand_configuration_path("~/%DIR%/100%%", &environment, false).unwrap(),
            PathBuf::from("/home/%LITERAL%/%NESTED%/100%")
        );
        assert_eq!(
            expand_configuration_path("%TILDE%", &environment, false).unwrap(),
            PathBuf::from("~/literal")
        );
        assert_eq!(
            expand_configuration_path("%%DIR%%", &environment, false).unwrap(),
            PathBuf::from("%DIR%")
        );
        assert_eq!(
            expand_configuration_path(
                "~/%ABS%",
                &BTreeMap::from([
                    ("HOME".into(), "/home/alice".into()),
                    ("ABS".into(), "/elsewhere".into()),
                ]),
                false
            )
            .unwrap(),
            PathBuf::from("/home/alice//elsewhere")
        );
    }

    #[test]
    fn environment_case_follows_the_host_and_exact_spelling_wins() {
        let environment = BTreeMap::from([
            ("Root".into(), "/mixed".into()),
            ("ROOT".into(), "/exact".into()),
        ]);
        assert_eq!(
            expand_configuration_path("%ROOT%", &environment, true).unwrap(),
            PathBuf::from("/exact")
        );
        assert_eq!(
            expand_configuration_path("%root%", &environment, true).unwrap(),
            PathBuf::from("/exact")
        );
        assert!(expand_configuration_path("%root%", &environment, false).is_err());
        assert_eq!(
            expand_configuration_path("%Root%", &environment, false).unwrap(),
            PathBuf::from("/mixed")
        );
    }

    #[test]
    fn malformed_empty_missing_and_control_values_are_errors() {
        let environment = BTreeMap::from([
            ("EMPTY".into(), "".into()),
            ("BAD".into(), "/path\nsecret".into()),
        ]);
        for value in ["", "%", "%MISSING%", "%EMPTY%", "%BAD%", "/raw\0path", "~"] {
            assert!(
                expand_configuration_path(value, &environment, false).is_err(),
                "{value:?}"
            );
        }
        assert!(
            !expand_configuration_path("%BAD%", &environment, false)
                .unwrap_err()
                .to_string()
                .contains("secret")
        );
    }

    #[test]
    fn limits_cover_literal_suffixes_and_inserted_values() {
        let environment = BTreeMap::from([("BIG".into(), "x".repeat(4096).into())]);
        assert!(expand_configuration_path(&"x".repeat(4096), &environment, false).is_ok());
        assert!(expand_configuration_path(&"x".repeat(4097), &environment, false).is_err());
        assert!(expand_configuration_path("%BIG%", &environment, false).is_ok());
        assert!(expand_configuration_path("%BIG%/suffix", &environment, false).is_err());
        assert!(expand_configuration_path("prefix/%BIG%", &environment, false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn native_non_utf8_environment_paths_are_not_lossily_decoded() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let environment =
            BTreeMap::from([("ROOT".into(), OsString::from_vec(b"/tmp/\xff".to_vec()))]);
        let result = expand_configuration_path("%ROOT%/manuals", &environment, false).unwrap();
        assert_eq!(result.as_os_str().as_bytes(), b"/tmp/\xff/manuals");
    }
}
