//! Discovers registered Markdown below platform-native application data roots.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Component, Path, PathBuf},
};

const LINUX_APPLICATION_DIR: &str = "mant";
const MACOS_APPLICATION_DIR: &str = "ManT";
const DOCUMENTS_DIR: &str = "documents";
const DEFAULT_SYSTEM_DATA_DIRS: [&str; 2] = ["/usr/local/share", "/usr/share"];
const MACOS_SYSTEM_DATA_DIR: &str = "/Library/Application Support";
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];
const MAX_DIRECTORY_DEPTH: usize = 32;
const MAX_VISITED_DIRECTORIES: usize = 4096;
const MAX_DISCOVERED_DOCUMENTS: usize = 10_000;

#[derive(Debug)]
struct DocumentCandidate {
    name: String,
    depth: usize,
    parent: PathBuf,
    extension_priority: u8,
    path: PathBuf,
}

/// Precedence class for one registered Markdown document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredDocumentOrigin {
    User,
    System,
}

/// One Markdown document explicitly registered in `ManT`'s document namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredDocument {
    pub name: String,
    pub path: PathBuf,
    pub origin: RegisteredDocumentOrigin,
}

/// Find the highest-precedence registered Markdown document for `document`.
#[must_use]
pub fn find_registered_document(document: &str) -> Option<RegisteredDocument> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    find_registered_document_with(document, &environment)
}

/// List effective registered documents after applying directory precedence.
#[must_use]
pub fn list_registered_documents() -> Vec<RegisteredDocument> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    list_registered_documents_with(&environment)
}

fn find_registered_document_with(
    document: &str,
    environment: &HashMap<OsString, OsString>,
) -> Option<RegisteredDocument> {
    let document = document.trim();
    if !is_safe_document_name(document) {
        return None;
    }
    list_registered_documents_with(environment)
        .into_iter()
        .find(|candidate| candidate.name == document)
}

fn list_registered_documents_with(
    environment: &HashMap<OsString, OsString>,
) -> Vec<RegisteredDocument> {
    let mut documents = BTreeMap::<String, RegisteredDocument>::new();
    for (root, origin) in registration_roots(environment) {
        for candidate in scan_registration_root(&root) {
            documents
                .entry(candidate.name.clone())
                .or_insert(RegisteredDocument {
                    name: candidate.name,
                    path: candidate.path,
                    origin,
                });
        }
    }
    documents.into_values().collect()
}

fn registration_roots(
    environment: &HashMap<OsString, OsString>,
) -> Vec<(PathBuf, RegisteredDocumentOrigin)> {
    if cfg!(target_os = "macos") {
        return macos_registration_roots(environment);
    }
    linux_registration_roots(environment)
}

fn linux_registration_roots(
    environment: &HashMap<OsString, OsString>,
) -> Vec<(PathBuf, RegisteredDocumentOrigin)> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    let user_data = absolute_environment_path(environment, "XDG_DATA_HOME").or_else(|| {
        environment
            .get(OsStr::new("HOME"))
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(".local/share"))
    });
    if let Some(root) = user_data {
        push_registration_root(
            &mut roots,
            &mut seen,
            &root,
            LINUX_APPLICATION_DIR,
            RegisteredDocumentOrigin::User,
        );
    }

    let system_roots = environment.get(OsStr::new("XDG_DATA_DIRS")).map_or_else(
        || {
            DEFAULT_SYSTEM_DATA_DIRS
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        },
        |value| {
            env::split_paths(value)
                .filter(|path| path.is_absolute())
                .collect()
        },
    );
    for root in system_roots {
        push_registration_root(
            &mut roots,
            &mut seen,
            &root,
            LINUX_APPLICATION_DIR,
            RegisteredDocumentOrigin::System,
        );
    }
    roots
}

fn macos_registration_roots(
    environment: &HashMap<OsString, OsString>,
) -> Vec<(PathBuf, RegisteredDocumentOrigin)> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    if let Some(home) = absolute_environment_path(environment, "HOME") {
        push_registration_root(
            &mut roots,
            &mut seen,
            &home.join("Library/Application Support"),
            MACOS_APPLICATION_DIR,
            RegisteredDocumentOrigin::User,
        );
    }
    push_registration_root(
        &mut roots,
        &mut seen,
        Path::new(MACOS_SYSTEM_DATA_DIR),
        MACOS_APPLICATION_DIR,
        RegisteredDocumentOrigin::System,
    );
    roots
}

fn push_registration_root(
    roots: &mut Vec<(PathBuf, RegisteredDocumentOrigin)>,
    seen: &mut HashSet<PathBuf>,
    root: &Path,
    application_dir: &str,
    origin: RegisteredDocumentOrigin,
) {
    let root = root.join(application_dir).join(DOCUMENTS_DIR);
    if seen.insert(root.clone()) {
        roots.push((root, origin));
    }
}

fn scan_registration_root(root: &Path) -> Vec<DocumentCandidate> {
    let mut visited = HashSet::new();
    let mut candidates = Vec::new();
    scan_registration_directory(root, root, 0, &mut visited, &mut candidates);
    candidates.sort_unstable_by(|left, right| {
        (
            &left.name,
            left.depth,
            &left.parent,
            left.extension_priority,
            &left.path,
        )
            .cmp(&(
                &right.name,
                right.depth,
                &right.parent,
                right.extension_priority,
                &right.path,
            ))
    });
    candidates
}

fn scan_registration_directory(
    root: &Path,
    directory: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    candidates: &mut Vec<DocumentCandidate>,
) {
    if depth > MAX_DIRECTORY_DEPTH
        || visited.len() >= MAX_VISITED_DIRECTORIES
        || candidates.len() >= MAX_DISCOVERED_DOCUMENTS
    {
        return;
    }
    let Ok(identity) = fs::canonicalize(directory) else {
        return;
    };
    if !visited.insert(identity) {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if candidates.len() >= MAX_DISCOVERED_DOCUMENTS {
            break;
        }
        let path = entry.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            scan_registration_directory(root, &path, depth + 1, visited, candidates);
        } else if metadata.is_file()
            && let Some(name) = markdown_document_name(&path)
            && let Some(extension_priority) = markdown_extension_priority(&path)
        {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            candidates.push(DocumentCandidate {
                name,
                depth,
                parent: relative
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_owned(),
                extension_priority,
                path,
            });
        }
    }
}

fn absolute_environment_path(
    environment: &HashMap<OsString, OsString>,
    name: &str,
) -> Option<PathBuf> {
    environment
        .get(OsStr::new(name))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn is_safe_document_name(document: &str) -> bool {
    !document.is_empty()
        && Path::new(document)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && Path::new(document).file_name() == Some(OsStr::new(document))
}

fn markdown_document_name(path: &Path) -> Option<String> {
    markdown_extension_priority(path)?;
    let name = path.file_stem()?.to_str()?;
    is_safe_document_name(name).then(|| name.to_owned())
}

fn markdown_extension_priority(path: &Path) -> Option<u8> {
    let extension = path.extension()?.to_str()?;
    MARKDOWN_EXTENSIONS
        .iter()
        .position(|candidate| extension.eq_ignore_ascii_case(candidate))
        .and_then(|index| u8::try_from(index).ok())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };

    use super::{
        RegisteredDocumentOrigin, find_registered_document_with, linux_registration_roots,
        list_registered_documents_with, macos_registration_roots,
    };

    fn environment(values: &[(&str, &Path)]) -> HashMap<OsString, OsString> {
        values
            .iter()
            .map(|(name, value)| (OsString::from(name), value.as_os_str().to_owned()))
            .collect()
    }

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-document-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn xdg_user_and_system_directories_follow_documented_precedence() {
        let home = Path::new("/home/demo");
        let user = Path::new("/data/user");
        let system = Path::new("/data/system");
        let environment = environment(&[
            ("HOME", home),
            ("XDG_DATA_HOME", user),
            ("XDG_DATA_DIRS", system),
        ]);

        assert_eq!(
            linux_registration_roots(&environment),
            vec![
                (user.join("mant/documents"), RegisteredDocumentOrigin::User),
                (
                    system.join("mant/documents"),
                    RegisteredDocumentOrigin::System
                ),
            ]
        );
    }

    #[test]
    fn macos_uses_application_support_document_roots() {
        let home = Path::new("/Users/demo");
        let environment = environment(&[("HOME", home)]);

        assert_eq!(
            macos_registration_roots(&environment),
            vec![
                (
                    home.join("Library/Application Support/ManT/documents"),
                    RegisteredDocumentOrigin::User,
                ),
                (
                    PathBuf::from("/Library/Application Support/ManT/documents"),
                    RegisteredDocumentOrigin::System,
                ),
            ]
        );
    }

    #[test]
    fn lookup_rejects_paths_and_prefers_user_markdown() {
        let root = temporary_root("lookup");
        let user = root.join("user");
        let system = root.join("system");
        fs::create_dir_all(user.join("mant/documents")).expect("user documents");
        fs::create_dir_all(system.join("mant/documents")).expect("system documents");
        fs::write(user.join("mant/documents/tool.md"), "# User").expect("user document");
        fs::write(system.join("mant/documents/tool.md"), "# System").expect("system document");
        let environment = environment(&[("XDG_DATA_HOME", &user), ("XDG_DATA_DIRS", &system)]);

        let document =
            find_registered_document_with("tool", &environment).expect("registered document");
        assert_eq!(document.path, user.join("mant/documents/tool.md"));
        assert_eq!(document.origin, RegisteredDocumentOrigin::User);
        assert!(find_registered_document_with("../tool", &environment).is_none());

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn listing_is_sorted_deduplicated_and_accepts_both_markdown_extensions() {
        let root = temporary_root("list");
        let user = root.join("user");
        let system = root.join("system");
        fs::create_dir_all(user.join("mant/documents")).expect("user documents");
        fs::create_dir_all(system.join("mant/documents")).expect("system documents");
        fs::write(user.join("mant/documents/zeta.markdown"), "# Zeta").expect("user document");
        fs::write(user.join("mant/documents/alpha.md"), "# Alpha").expect("user document");
        fs::write(
            user.join("mant/documents/alpha.markdown"),
            "# Lower priority",
        )
        .expect("alternate user document");
        fs::write(system.join("mant/documents/alpha.md"), "# Shadowed").expect("system document");
        fs::write(system.join("mant/documents/not-markdown.txt"), "ignored").expect("other file");
        let environment = environment(&[("XDG_DATA_HOME", &user), ("XDG_DATA_DIRS", &system)]);

        let documents = list_registered_documents_with(&environment);
        assert_eq!(
            documents
                .iter()
                .map(|document| document.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(documents[0].path, user.join("mant/documents/alpha.md"));

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn document_discovery_is_confined_to_the_documents_layer() {
        let root = temporary_root("documents-layer");
        let user = root.join("user");
        fs::create_dir_all(user.join("mant/documents")).expect("document directory");
        fs::write(user.join("mant/current.md"), "# Outside scanner")
            .expect("non-document application data");
        fs::write(user.join("mant/documents/current.md"), "# Current")
            .expect("registered document");
        let environment = environment(&[("XDG_DATA_HOME", &user)]);

        let documents = list_registered_documents_with(&environment);
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].path, user.join("mant/documents/current.md"));

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn nested_directories_and_symlinks_are_discovered_without_cycles() {
        use std::os::unix::fs::symlink;

        let root = temporary_root("nested-links");
        let user = root.join("user");
        let registration = user.join("mant/documents");
        let external = root.join("external");
        fs::create_dir_all(registration.join("team")).expect("nested registration");
        fs::create_dir_all(&external).expect("external documents");
        fs::write(registration.join("guide.markdown"), "# Shallow").expect("shallow document");
        fs::write(registration.join("team/guide.md"), "# Nested").expect("nested duplicate");
        fs::write(external.join("linked.md"), "# Linked").expect("linked document");
        symlink(&external, registration.join("imported")).expect("linked directory");
        symlink(&registration, external.join("cycle")).expect("directory cycle");
        symlink(external.join("linked.md"), registration.join("alias.md")).expect("linked file");
        let environment = environment(&[("XDG_DATA_HOME", &user)]);

        let documents = list_registered_documents_with(&environment);
        assert_eq!(
            documents
                .iter()
                .map(|document| document.name.as_str())
                .collect::<Vec<_>>(),
            ["alias", "guide", "linked"]
        );
        assert_eq!(
            documents
                .iter()
                .find(|document| document.name == "guide")
                .expect("guide")
                .path,
            registration.join("guide.markdown")
        );
        assert_eq!(
            documents
                .iter()
                .find(|document| document.name == "linked")
                .expect("linked")
                .path,
            registration.join("imported/linked.md")
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }
}
