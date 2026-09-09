use std::{collections::HashMap, ffi::OsString, fs, path::PathBuf};

use super::{
    LocateError, ManualIndex, ManualRequest, deduplicate_paths, locate_manual_source_in,
    normalize_locale, parse_manual_section_order,
};
use crate::manual_paths::discover_manual_roots_with;

fn temporary_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mant-manual-index-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[cfg(unix)]
fn symlink_file(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn symlink_directory(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_directory(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(any(unix, windows))]
fn created_link(result: std::io::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) if cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied => {
            false
        }
        Err(error) => panic!("create fixture symlink: {error}"),
    }
}

#[test]
fn indexes_supported_sources_and_resolves_sections_without_man() {
    let root = temporary_root("lookup");
    fs::create_dir_all(root.join("man1")).expect("manual section");
    fs::create_dir_all(root.join("man3")).expect("manual section");
    fs::write(root.join("man1/printf.1.gz"), b"gzip placeholder").expect("manual");
    fs::write(root.join("man3/printf.3.zst"), b"zstd placeholder").expect("manual");
    fs::write(root.join("flat-tool.1"), b"flat manual").expect("flat manual");
    fs::write(root.join("README.md"), b"not a manual").expect("readme");
    fs::write(root.join("man1/ignored.1.xz"), b"unsupported").expect("manual");

    let index = ManualIndex::from_roots(vec![root.clone()]);
    assert_eq!(index.pages().len(), 3);
    assert_eq!(
        locate_manual_source_in(&ManualRequest::new("printf", None), &index)
            .expect("default section")
            .path,
        root.join("man1/printf.1.gz")
    );
    assert_eq!(
        locate_manual_source_in(&ManualRequest::new("printf", Some("3".to_owned())), &index,)
            .expect("selected section")
            .path,
        root.join("man3/printf.3.zst")
    );
    assert_eq!(index.pages()[0].manual_root, root);
    assert_eq!(
        locate_manual_source_in(&ManualRequest::new("flat-tool", None), &index)
            .expect("flat root page")
            .path,
        root.join("flat-tool.1")
    );
    assert!(matches!(
        locate_manual_source_in(&ManualRequest::new("ignored", None), &index),
        Err(LocateError::NotFound { .. })
    ));

    let invalid = locate_manual_source_in(
        &ManualRequest::new("printf", Some("DESCRIPTION".to_owned())),
        &index,
    )
    .expect_err("document heading is not a manual section");
    assert_eq!(invalid, LocateError::InvalidManualSection);

    let unavailable =
        locate_manual_source_in(&ManualRequest::new("printf", Some("5".to_owned())), &index)
            .expect_err("valid but unavailable manual section");
    assert_eq!(
        unavailable.to_string(),
        "requested manual section '5' is unavailable for 'printf'; available manual sections: 1, 3"
    );
    assert_eq!(
        unavailable.load_detail(),
        "manual section '5' is unavailable; available sections: 1, 3"
    );

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn unqualified_lookup_uses_manual_section_precedence_instead_of_lexical_order() {
    let root = temporary_root("section-precedence");
    fs::create_dir_all(root.join("man5")).expect("section 5");
    fs::create_dir_all(root.join("man8")).expect("section 8");
    fs::write(root.join("man5/btrfs.5"), b".TH BTRFS 5\n").expect("section 5 page");
    fs::write(root.join("man8/btrfs.8"), b".TH BTRFS 8\n").expect("section 8 page");

    let index = ManualIndex::from_roots_with_locale(vec![root.clone()], None);

    assert_eq!(
        index.find("btrfs", None).map(|page| page.section.as_str()),
        Some("8")
    );
    assert_eq!(index.available_manual_sections("btrfs"), ["8", "5"]);
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn explicit_manual_section_order_controls_unqualified_lookup() {
    let root = temporary_root("explicit-section-precedence");
    fs::create_dir_all(root.join("man5")).expect("section 5");
    fs::create_dir_all(root.join("man8")).expect("section 8");
    fs::write(root.join("man5/btrfs.5"), b".TH BTRFS 5\n").expect("section 5 page");
    fs::write(root.join("man8/btrfs.8"), b".TH BTRFS 8\n").expect("section 8 page");
    let order = vec!["5".to_owned(), "8".to_owned()];

    let index = ManualIndex::from_roots_with_locale_and_sections(vec![root.clone()], None, &order);

    assert_eq!(
        index.find("btrfs", None).map(|page| page.section.as_str()),
        Some("5")
    );
    assert_eq!(index.available_manual_sections("btrfs"), ["5", "8"]);
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn mansect_is_colon_separated_trimmed_and_deduplicated() {
    assert_eq!(
        parse_manual_section_order(" 8:1:8::5 "),
        Some(vec!["8".to_owned(), "1".to_owned(), "5".to_owned()])
    );
    assert_eq!(parse_manual_section_order(":: "), None);
}

#[cfg(any(unix, windows))]
#[test]
fn indexes_leaf_file_symlinks_without_traversing_linked_directories() {
    let base = temporary_root("symlink-boundary");
    let root = base.join("root");
    let man1 = root.join("man1");
    let linked_tree = base.join("linked-tree/man1");
    fs::create_dir_all(&man1).expect("manual section");
    fs::create_dir_all(&linked_tree).expect("linked manual section");
    fs::write(man1.join("target.1"), ".TH TARGET 1\n").expect("inside target");
    fs::write(base.join("outside.1"), ".TH OUTSIDE 1\n").expect("outside target");
    fs::write(linked_tree.join("nested.1"), ".TH NESTED 1\n").expect("nested target");
    if !created_link(symlink_file(&man1.join("target.1"), &man1.join("inside.1")))
        || !created_link(symlink_file(
            &base.join("outside.1"),
            &man1.join("outside.1"),
        ))
        || !created_link(symlink_file(
            &base.join("missing.1"),
            &man1.join("broken.1"),
        ))
        || !created_link(symlink_directory(
            &base.join("linked-tree"),
            &root.join("linked-tree"),
        ))
    {
        fs::remove_dir_all(base).expect("remove unsupported symlink fixture");
        return;
    }

    let index = ManualIndex::from_roots(vec![root]);
    assert!(index.find("inside", Some("1")).is_some());
    assert!(index.find("outside", Some("1")).is_some());
    assert!(index.find("broken", Some("1")).is_none());
    assert!(index.find("nested", Some("1")).is_none());
    fs::remove_dir_all(base).expect("remove fixture");
}

#[test]
fn root_and_locale_precedence_are_deterministic() {
    let root = temporary_root("precedence");
    let first = root.join("first");
    let second = root.join("second");
    for path in [
        first.join("man1/tool.1"),
        first.join("zh/man1/tool.1"),
        first.join("zh_CN/man1/tool.1"),
        second.join("man1/tool.1"),
    ] {
        fs::create_dir_all(path.parent().expect("manual parent")).expect("manual section");
        fs::write(path, b"manual").expect("manual");
    }

    let index = ManualIndex::from_roots_with_locale(vec![first.clone(), second], Some("zh_CN"));
    assert_eq!(index.pages()[0].path, first.join("zh_CN/man1/tool.1"));

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn locale_names_drop_encodings_modifiers_and_language_fallbacks() {
    assert_eq!(
        normalize_locale("zh_CN.UTF-8@variant"),
        Some("zh_CN".to_owned())
    );
    assert_eq!(normalize_locale("de_DE:en_US"), Some("de_DE".to_owned()));
    assert_eq!(normalize_locale("C"), None);
}

#[cfg(unix)]
#[test]
fn explicit_manpath_overrides_conventions_and_empty_components_restore_them() {
    let explicit = PathBuf::from("/opt/manuals");
    let mut environment = HashMap::from([
        (OsString::from("HOME"), OsString::from("/home/demo")),
        (OsString::from("PATH"), OsString::from("/opt/bin:/usr/bin")),
        (
            OsString::from("MANT_MANPATH"),
            explicit.as_os_str().to_owned(),
        ),
    ]);
    assert_eq!(discover_manual_roots_with(&environment), vec![explicit]);

    environment.remove(&OsString::from("MANT_MANPATH"));
    environment.insert(OsString::from("MANPATH"), OsString::from(":"));
    let roots = discover_manual_roots_with(&environment);
    assert!(roots.contains(&PathBuf::from("/home/demo/.local/share/man")));
    assert!(roots.contains(&PathBuf::from("/opt/share/man")));
    assert!(roots.contains(&PathBuf::from("/usr/share/man")));
}

#[cfg(windows)]
#[test]
fn windows_defaults_to_mant_data_then_user_share_and_honors_manpath() {
    let data_root = PathBuf::from(r"C:\Users\demo\AppData\Roaming");
    let profile = PathBuf::from(r"C:\Users\demo");
    let environment = HashMap::from([
        (OsString::from("APPDATA"), data_root.as_os_str().to_owned()),
        (
            OsString::from("USERPROFILE"),
            profile.as_os_str().to_owned(),
        ),
    ]);
    assert_eq!(
        discover_manual_roots_with(&environment),
        vec![
            data_root.join("ManT").join("man"),
            profile.join(".local/share/man")
        ]
    );

    let custom = PathBuf::from(r"D:\manuals");
    let mut environment = environment;
    environment.insert(
        OsString::from("MANPATH"),
        std::env::join_paths([&custom]).expect("join Windows MANPATH"),
    );
    assert_eq!(discover_manual_roots_with(&environment), vec![custom]);
}

#[test]
fn relative_manual_roots_are_resolved_for_stable_catalog_paths() {
    let roots = deduplicate_paths([PathBuf::from("project-man")]);
    assert_eq!(roots.len(), 1);
    assert!(roots[0].is_absolute());
    assert!(roots[0].ends_with("project-man"));
}

#[test]
fn invalid_requests_fail_before_lookup() {
    let index = ManualIndex::default();
    assert_eq!(
        locate_manual_source_in(&ManualRequest::new(" ", None), &index),
        Err(LocateError::EmptyName)
    );
    assert_eq!(
        locate_manual_source_in(&ManualRequest::new("git", Some(" ".to_owned())), &index,),
        Err(LocateError::InvalidManualSection)
    );
}

#[cfg(windows)]
#[test]
fn windows_manual_names_are_ascii_case_insensitive() {
    let index = ManualIndex {
        roots: vec![PathBuf::from(r"C:\man")],
        pages: vec![super::ManualPage {
            name: "cargo.exe".to_owned(),
            section: "1".to_owned(),
            path: PathBuf::from(r"C:\man\cargo.exe.1"),
            manual_root: PathBuf::from(r"C:\man"),
        }],
    };

    assert!(index.find("cargo.EXE", None).is_some());
}
