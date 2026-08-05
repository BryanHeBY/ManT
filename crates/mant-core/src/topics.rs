//! Discovers user-registered Markdown documents through the XDG data hierarchy.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Component, Path, PathBuf},
};

const APPLICATION_DIR: &str = "mant";
const TOPICS_DIR: &str = "topics";
const DEFAULT_SYSTEM_DATA_DIRS: [&str; 2] = ["/usr/local/share", "/usr/share"];
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// Precedence class for one registered Markdown topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredTopicOrigin {
    User,
    System,
}

/// One Markdown document explicitly registered in `ManT`'s topic namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredTopic {
    pub name: String,
    pub path: PathBuf,
    pub origin: RegisteredTopicOrigin,
}

/// Find the highest-precedence registered Markdown document for `topic`.
#[must_use]
pub fn find_registered_topic(topic: &str) -> Option<RegisteredTopic> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    find_registered_topic_with(topic, &environment)
}

/// List effective registered topics after applying directory precedence.
#[must_use]
pub fn list_registered_topics() -> Vec<RegisteredTopic> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    list_registered_topics_with(&environment)
}

fn find_registered_topic_with(
    topic: &str,
    environment: &HashMap<OsString, OsString>,
) -> Option<RegisteredTopic> {
    let topic = topic.trim();
    if !is_safe_topic_name(topic) {
        return None;
    }
    for (directory, origin) in topic_directories(environment) {
        for extension in MARKDOWN_EXTENSIONS {
            let path = directory.join(format!("{topic}.{extension}"));
            if path.is_file() {
                return Some(RegisteredTopic {
                    name: topic.to_owned(),
                    path,
                    origin,
                });
            }
        }
    }
    None
}

fn list_registered_topics_with(environment: &HashMap<OsString, OsString>) -> Vec<RegisteredTopic> {
    let mut topics = BTreeMap::<String, RegisteredTopic>::new();
    for (directory, origin) in topic_directories(environment) {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        let mut candidates = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.is_file().then(|| markdown_topic_name(&path))??;
                let priority = markdown_extension_priority(&path)?;
                Some((name, priority, path))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        for (name, _, path) in candidates {
            topics
                .entry(name.clone())
                .or_insert(RegisteredTopic { name, path, origin });
        }
    }
    topics.into_values().collect()
}

fn topic_directories(
    environment: &HashMap<OsString, OsString>,
) -> Vec<(PathBuf, RegisteredTopicOrigin)> {
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
        push_topic_directory(
            &mut directories,
            &mut seen,
            &root,
            RegisteredTopicOrigin::User,
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
        push_topic_directory(
            &mut directories,
            &mut seen,
            &root,
            RegisteredTopicOrigin::System,
        );
    }
    directories
}

fn push_topic_directory(
    directories: &mut Vec<(PathBuf, RegisteredTopicOrigin)>,
    seen: &mut HashSet<PathBuf>,
    root: &Path,
    origin: RegisteredTopicOrigin,
) {
    let directory = root.join(APPLICATION_DIR).join(TOPICS_DIR);
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

fn is_safe_topic_name(topic: &str) -> bool {
    !topic.is_empty()
        && Path::new(topic)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && Path::new(topic).file_name() == Some(OsStr::new(topic))
}

fn markdown_topic_name(path: &Path) -> Option<String> {
    markdown_extension_priority(path)?;
    let name = path.file_stem()?.to_str()?;
    is_safe_topic_name(name).then(|| name.to_owned())
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
        RegisteredTopicOrigin, find_registered_topic_with, list_registered_topics_with,
        topic_directories,
    };

    fn environment(values: &[(&str, &Path)]) -> HashMap<OsString, OsString> {
        values
            .iter()
            .map(|(name, value)| (OsString::from(name), value.as_os_str().to_owned()))
            .collect()
    }

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-topic-{label}-{}-{:?}",
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
            topic_directories(&environment),
            vec![
                (user.join("mant/topics"), RegisteredTopicOrigin::User),
                (system.join("mant/topics"), RegisteredTopicOrigin::System),
            ]
        );
    }

    #[test]
    fn lookup_rejects_paths_and_prefers_user_markdown() {
        let root = temporary_root("lookup");
        let user = root.join("user");
        let system = root.join("system");
        fs::create_dir_all(user.join("mant/topics")).expect("user topics");
        fs::create_dir_all(system.join("mant/topics")).expect("system topics");
        fs::write(user.join("mant/topics/tool.md"), "# User").expect("user topic");
        fs::write(system.join("mant/topics/tool.md"), "# System").expect("system topic");
        let environment = environment(&[("XDG_DATA_HOME", &user), ("XDG_DATA_DIRS", &system)]);

        let topic = find_registered_topic_with("tool", &environment).expect("registered topic");
        assert_eq!(topic.path, user.join("mant/topics/tool.md"));
        assert_eq!(topic.origin, RegisteredTopicOrigin::User);
        assert!(find_registered_topic_with("../tool", &environment).is_none());

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn listing_is_sorted_deduplicated_and_accepts_both_markdown_extensions() {
        let root = temporary_root("list");
        let user = root.join("user");
        let system = root.join("system");
        fs::create_dir_all(user.join("mant/topics")).expect("user topics");
        fs::create_dir_all(system.join("mant/topics")).expect("system topics");
        fs::write(user.join("mant/topics/zeta.markdown"), "# Zeta").expect("user topic");
        fs::write(user.join("mant/topics/alpha.md"), "# Alpha").expect("user topic");
        fs::write(user.join("mant/topics/alpha.markdown"), "# Lower priority")
            .expect("alternate user topic");
        fs::write(system.join("mant/topics/alpha.md"), "# Shadowed").expect("system topic");
        fs::write(system.join("mant/topics/not-markdown.txt"), "ignored").expect("other file");
        let environment = environment(&[("XDG_DATA_HOME", &user), ("XDG_DATA_DIRS", &system)]);

        let topics = list_registered_topics_with(&environment);
        assert_eq!(
            topics
                .iter()
                .map(|topic| topic.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(topics[0].path, user.join("mant/topics/alpha.md"));

        fs::remove_dir_all(root).expect("remove fixture");
    }
}
