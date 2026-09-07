//! Logical Markdown names and native-relative conversion have distinct entry points.
use std::path::{Component, Path};
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];
pub(crate) fn normalize_document_path(document: &str) -> Option<String> {
    let document = document.trim();
    if document.starts_with('/') || document.contains('\\') {
        return None;
    }
    let components = document
        .split('/')
        .map(|value| {
            (!value.is_empty()
                && !matches!(value, "." | "..")
                && !value.chars().any(is_unsafe_logical_path_character))
            .then_some(value)
        })
        .collect::<Option<Vec<_>>>()?;
    (!components.is_empty()).then(|| components.join("/"))
}

pub(crate) fn normalize_relative_document_path(path: &Path) -> Option<String> {
    let components = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().filter(|value| {
                !value.is_empty()
                    && !value.contains(['/', '\\'])
                    && !value.chars().any(is_unsafe_logical_path_character)
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!components.is_empty()).then(|| components.join("/"))
}

pub(crate) fn is_unsafe_logical_path_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00ad}'
                | '\u{600}'..='\u{605}'
                | '\u{61c}'
                | '\u{6dd}'
                | '\u{70f}'
                | '\u{890}'..='\u{891}'
                | '\u{8e2}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{110bd}'
                | '\u{110cd}'
                | '\u{13430}'..='\u{1343f}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0001}'
                | '\u{e0020}'..='\u{e007f}'
        )
}

pub(crate) fn markdown_extension_priority(path: &Path) -> Option<u8> {
    let extension = path.extension()?.to_str()?;
    MARKDOWN_EXTENSIONS
        .iter()
        .position(|candidate| extension.eq_ignore_ascii_case(candidate))
        .and_then(|index| u8::try_from(index).ok())
}
