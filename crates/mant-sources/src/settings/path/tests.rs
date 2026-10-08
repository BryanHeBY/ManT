use super::*;

#[test]
fn shared_windows_cases_have_host_independent_results() {
    for line in include_str!("../windows_paths.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        let expected = fields[1].parse::<bool>().unwrap();
        match fields[0] {
            "absolute" => assert_eq!(
                is_absolute_configuration_path(Path::new(fields[2]), true),
                expected,
                "{line}"
            ),
            "equal" => {
                let left = windows_path_key(Path::new(fields[2]));
                let right = windows_path_key(Path::new(fields[3]));
                assert_eq!(left.is_some() && left == right, expected, "{line}");
            }
            _ => panic!("unknown path fixture: {line}"),
        }
    }
}

#[test]
fn unix_classification_does_not_inherit_the_test_hosts_windows_rules() {
    assert!(is_absolute_configuration_path(Path::new("/manuals"), false));
    assert!(!is_absolute_configuration_path(
        Path::new(r"C:\manuals"),
        false
    ));
    assert!(is_relative_configuration_path(
        Path::new(r"C:\manuals"),
        false
    ));
    assert!(!is_relative_configuration_path(
        Path::new("/manuals"),
        false
    ));
}

#[test]
fn relative_resolution_rejects_drive_root_and_namespace_dependence() {
    let base = Path::new(r"C:\config\mant");
    for value in [
        r"C:manuals",
        r"\manuals",
        "/manuals",
        r"\\server",
        r"\\.\pipe\mant",
    ] {
        assert!(
            resolve(Path::new(value), Some(base), true).is_err(),
            "{value}"
        );
    }
    assert_eq!(
        resolve(Path::new("manuals"), Some(base), true).unwrap(),
        base.join("manuals")
    );
    assert!(resolve(Path::new("manuals"), None, true).is_err());
    for value in [r"D:\manuals", "D:/manuals", r"\\server\share\manuals"] {
        assert_eq!(
            resolve(Path::new(value), None, true).unwrap(),
            Path::new(value)
        );
    }
}

#[test]
fn resolved_base_is_included_in_the_bound() {
    let base = std::path::PathBuf::from(format!("C:/{}", "x".repeat(4080)));
    assert!(resolve(Path::new(&"y".repeat(32)), Some(&base), true).is_err());
    for value in ["C:/bad\npath", "C:/bad\u{85}path", "C:/bad\0path"] {
        assert!(!is_absolute_configuration_path(Path::new(value), true));
        assert_eq!(windows_path_key(Path::new(value)), None);
    }
}

#[cfg(unix)]
#[test]
fn unix_backslashes_and_parent_components_remain_literal() {
    let base = Path::new("/config/mant");
    assert_eq!(
        resolve(Path::new(r"relative\manuals"), Some(base), false).unwrap(),
        base.join(r"relative\manuals")
    );
    assert_eq!(
        resolve(Path::new("../manuals"), Some(base), false).unwrap(),
        base.join("../manuals")
    );
}

#[cfg(unix)]
#[test]
fn comparison_does_not_lossily_merge_non_utf8_names() {
    use std::os::unix::ffi::OsStrExt;
    let first = Path::new(OsStr::from_bytes(b"C:/manuals/\xff"));
    let second = Path::new(OsStr::from_bytes(b"C:/manuals/\xfe"));
    assert_ne!(windows_path_key(first), windows_path_key(second));
}

#[cfg(windows)]
#[test]
fn comparison_preserves_unpaired_native_wide_units() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let mut first = "C:/manuals/".encode_utf16().collect::<Vec<_>>();
    first.push(0xd800);
    let mut second = first.clone();
    *second.last_mut().unwrap() = 0xd801;
    let first = std::path::PathBuf::from(OsString::from_wide(&first));
    let second = std::path::PathBuf::from(OsString::from_wide(&second));
    assert_ne!(windows_path_key(&first), windows_path_key(&second));
}
