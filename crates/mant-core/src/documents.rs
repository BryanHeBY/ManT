//! Discovers user-registered Markdown documents through the XDG data hierarchy.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Component, Path, PathBuf},
};

const APPLICATION_DIR: &str = "mant";
const DOCUMENTS_DIR: &str = "documents";
const DEFAULT_SYSTEM_DATA_DIRS: [&str; 2] = ["/usr/local/share", "/usr/share"];
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

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
    for (directory, origin) in document_directories(environment) {
        for extension in MARKDOWN_EXTENSIONS {
            let path = directory.join(format!("{document}.{extension}"));
            if path.is_file() {
                return Some(RegisteredDocument {
                    name: document.to_owned(),
                    path,
                    origin,
                });
            }
        }
    }
    None
}

fn list_registered_documents_with(
    environment: &HashMap<OsString, OsString>,
) -> Vec<RegisteredDocument> {
    let mut documents = BTreeMap::<String, RegisteredDocument>::new();
    for (directory, origin) in document_directories(environment) {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        let mut candidates = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.is_file().then(|| markdown_document_name(&path))??;
                let priority = markdown_extension_priority(&path)?;
                Some((name, priority, path))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        for (name, _, path) in candidates {
            documents
                .entry(name.clone())
                .or_insert(RegisteredDocument { name, path, origin });
        }
    }
    documents.into_values().collect()
}

fn document_directories(
    environment: &HashMap<OsString, OsString>,
) -> Vec<(PathBuf, RegisteredDocumentOrigin)> {
    let mut directories = Vec::new();
    let mut seen = HashSet::new();

    let user_data = absolute_environment_path(environment, "XDG_DATA_HOME").or_else(|| {
        environment
            .get(OsStr::new("HOME"))
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(".local/share"))
    });
    if let Some(root) = user_data {
        push_document_directory(
            &mut directories,
            &mut seen,
            &root,
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
        push_document_directory(
            &mut directories,
            &mut seen,
            &root,
            RegisteredDocumentOrigin::System,
        );
    }
    directories
}

fn push_document_directory(
    directories: &mut Vec<(PathBuf, RegisteredDocumentOrigin)>,
    seen: &mut HashSet<PathBuf>,
    root: &Path,
    origin: RegisteredDocumentOrigin,
) {
    let directory = root.join(APPLICATION_DIR).join(DOCUMENTS_DIR);
    if seen.insert(directory.clone()) {
        directories.push((directory, origin));
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
        RegisteredDocumentOrigin, document_directories, find_registered_document_with,
        list_registered_documents_with,
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
            document_directories(&environment),
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
}
