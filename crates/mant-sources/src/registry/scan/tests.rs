use super::{scan_directory, source_directory_ready};
use crate::document_path::{normalize_document_path, normalize_relative_document_path};
use std::{fs, path::PathBuf};

fn temporary_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mant-document-{label}-{}-{:?}",
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

fn created_link(result: std::io::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        #[cfg(windows)]
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            false
        }
        Err(error) => panic!("create fixture symlink: {error}"),
    }
}

#[test]
fn discovery_is_hierarchical_and_markdown_only() {
    let root = temporary_root("flat");
    fs::create_dir_all(root.join("nested")).expect("create directories");
    fs::write(root.join("alpha.md"), "# alpha").expect("write alpha");
    fs::write(root.join("beta.markdown"), "# beta").expect("write beta");
    fs::write(root.join("ignored.txt"), "ignored").expect("write text");
    fs::write(root.join("nested/hidden.md"), "# hidden").expect("write nested");

    let documents = scan_directory(&root, false).expect("scan documents");
    assert_eq!(
        documents
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "nested/hidden"]
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn logical_paths_reject_non_portable_components() {
    assert_eq!(
        normalize_document_path(" docs/tool "),
        Some("docs/tool".to_owned())
    );
    assert_eq!(normalize_document_path("docs/back\\slash"), None);
    assert_eq!(normalize_document_path("docs//tool"), None);
    assert_eq!(normalize_document_path("./docs/tool"), None);
    assert_eq!(normalize_document_path("docs/../tool"), None);
    assert_eq!(
        normalize_relative_document_path(&PathBuf::from("docs").join("tool")),
        Some("docs/tool".to_owned())
    );
    assert_eq!(normalize_document_path("docs/bad\u{1f}name"), None);
    for hidden in ['\u{202e}', '\u{200b}', '\u{feff}', '\u{e0020}'] {
        assert_eq!(
            normalize_document_path(&format!("docs/trusted{hidden}name")),
            None
        );
    }
    assert_eq!(
        normalize_document_path("文档/可信名称"),
        Some("文档/可信名称".to_owned())
    );
}

#[test]
fn personal_documents_do_not_reserve_a_sources_subdirectory() {
    let root = temporary_root("personal-sources-name");
    fs::create_dir_all(root.join("sources")).expect("create personal directory");
    fs::write(root.join("sources/tool.md"), "# Personal tool").expect("write personal document");

    let documents = scan_directory(&root, true).expect("scan documents");
    assert_eq!(
        documents,
        vec![("sources/tool".to_owned(), root.join("sources/tool.md"))]
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn md_wins_over_markdown_for_the_same_logical_path() {
    let root = temporary_root("extension");
    fs::create_dir_all(&root).expect("create directory");
    fs::write(root.join("tool.markdown"), "long").expect("write markdown");
    fs::write(root.join("tool.md"), "short").expect("write md");
    let documents = scan_directory(&root, false).expect("scan documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].1, root.join("tool.md"));
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn personal_documents_accept_only_regular_leaf_file_links() {
    let base = temporary_root("leaf-links");
    let documents = base.join("documents");
    let outside = base.join("outside");
    fs::create_dir_all(&documents).expect("create documents");
    fs::create_dir_all(&outside).expect("create outside directory");
    fs::write(outside.join("mant-source.md"), "# ManT").expect("write external document");
    fs::write(outside.join("nested.md"), "# Nested").expect("write linked directory file");

    if !created_link(symlink_file(
        &outside.join("mant-source.md"),
        &documents.join("mant.md"),
    )) || !created_link(symlink_file(
        &outside.join("missing.md"),
        &documents.join("broken.md"),
    )) || !created_link(symlink_directory(
        &outside,
        &documents.join("linked-directory"),
    )) {
        fs::remove_dir_all(base).expect("remove unsupported symlink fixture");
        return;
    }

    let personal = scan_directory(&documents, true).expect("scan personal documents");
    assert_eq!(
        personal,
        vec![("mant".to_owned(), documents.join("mant.md"))]
    );
    let managed = scan_directory(&documents, false).expect("scan managed source");
    assert!(managed.is_empty());
    fs::remove_dir_all(base).expect("remove fixture");
}

#[test]
fn managed_source_readiness_rejects_linked_roots_and_metadata() {
    let base = temporary_root("managed-root-links");
    let installed = base.join("installed");
    let linked_root = base.join("linked-root");
    fs::create_dir_all(&installed).expect("create installed source");
    fs::write(installed.join(crate::SOURCE_METADATA_FILE), "metadata")
        .expect("write source metadata");
    if !created_link(symlink_directory(&installed, &linked_root)) {
        fs::remove_dir_all(base).expect("remove unsupported symlink fixture");
        return;
    }
    assert!(source_directory_ready(&installed));
    assert!(!source_directory_ready(&linked_root));

    let linked_metadata_root = base.join("linked-metadata");
    fs::create_dir_all(&linked_metadata_root).expect("create linked metadata root");
    if !created_link(symlink_file(
        &installed.join(crate::SOURCE_METADATA_FILE),
        &linked_metadata_root.join(crate::SOURCE_METADATA_FILE),
    )) {
        fs::remove_dir_all(base).expect("remove unsupported symlink fixture");
        return;
    }
    assert!(!source_directory_ready(&linked_metadata_root));
    fs::remove_dir_all(base).expect("remove fixture");
}

#[test]
fn scan_depth_limit_keeps_the_last_allowed_level() {
    let root = temporary_root("depth-budget");
    fs::create_dir_all(&root).expect("create fixture");
    fs::write(root.join("tool.md"), "# Tool").expect("write document");
    let mut candidates = std::collections::BTreeMap::new();
    super::scan_directory_into(&root, &root, false, 32, &mut candidates)
        .expect("last permitted depth");
    assert_eq!(candidates.len(), 1);
    assert!(candidates.contains_key("tool"));

    let error = super::scan_directory_into(&root, &root, false, 33, &mut candidates)
        .expect_err("the next directory level exceeds the bound");
    assert!(error.to_string().contains("exceeds 32 directory levels"));
    assert_eq!(candidates.len(), 1, "rejection does not add candidates");
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn full_scan_budget_allows_extension_replacement_but_not_a_new_identity() {
    let root = temporary_root("document-budget");
    fs::create_dir_all(&root).expect("create fixture");
    fs::write(root.join("tool.md"), "# Tool").expect("write document");
    // Seed the already-scanned identities instead of creating 10,000 files.
    // The real next file must still pass the normal scan and extension logic.
    let mut candidates = (0..9_999)
        .map(|index| (format!("seed-{index}"), (0, PathBuf::from("previous.md"))))
        .collect::<std::collections::BTreeMap<_, _>>();
    candidates.insert("tool".to_owned(), (1, root.join("tool.markdown")));
    super::scan_directory_into(&root, &root, false, 0, &mut candidates)
        .expect("replace an existing identity at the limit");
    assert_eq!(candidates.len(), 10_000);
    assert_eq!(candidates["tool"], (0, root.join("tool.md")));

    fs::write(root.join("zz-new.md"), "# New").expect("write extra document");
    let error = super::scan_directory_into(&root, &root, false, 0, &mut candidates)
        .expect_err("a new identity must not be silently truncated");
    assert!(error.to_string().contains("exceeds 10000 Markdown files"));
    assert_eq!(candidates.len(), 10_000);
    assert!(!candidates.contains_key("zz-new"));
    fs::remove_dir_all(root).expect("remove fixture");
}
