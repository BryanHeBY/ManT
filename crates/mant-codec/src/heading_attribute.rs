//! `ManT`'s single source-level Markdown heading attribute.

/// Return the opening brace offset and the exact authored fragment.
pub(crate) fn trailing_id(value: &str) -> Option<(usize, &str)> {
    let value = value.trim_end();
    let opening = value.rfind("{#")?;
    let prefix = &value[..opening];
    if !prefix.is_empty() && !prefix.ends_with(char::is_whitespace) {
        return None;
    }
    let id = value[opening + 2..].strip_suffix('}')?;
    (!id.is_empty()
        && !id.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '{' | '}' | '\\' | '<' | '>')
        }))
    .then_some((opening, id))
}
