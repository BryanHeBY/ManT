//! Source sibling boundaries, recorded before formatter/normalization flattening.
use crate::definitions::NativeHeadEvidence;
use libmandoc_rs::{Node, NodeKind};

use super::roff_escape::visible_text;

pub(super) fn record(root: &Node, evidence: &mut NativeHeadEvidence) {
    fn declaration(node: &Node) -> bool {
        node.kind == NodeKind::Block
            && matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "It"))
    }
    fn transparent(node: &Node) -> bool {
        matches!(node.macro_name.as_deref(), Some("PD" | "Sm" | "Tg" | "ft"))
    }
    let mut previous: Option<&Node> = None;
    for node in &root.children {
        if declaration(node) {
            let key = std::ptr::from_ref(node) as usize;
            if source_presentation_head(node) {
                evidence.groups.presentation_head(key);
            }
            if let Some(left) = previous
                && left.flow_epoch == node.flow_epoch
            {
                evidence.groups.adjacent(
                    std::ptr::from_ref(left) as usize,
                    key,
                    has_readable_body(left),
                );
            }
            previous = Some(node);
        } else if !transparent(node) {
            previous = None;
        }
        record(node, evidence);
    }
}

/// Detect source-proven list heads whose visible text is only a formatter
/// template.  This fact is intentionally recorded before lowering and is
/// used only when semantic recognition has already rejected that owner: a
/// real addressable definition never becomes presentation merely because its
/// spelling resembles one of these templates.
fn source_presentation_head(node: &Node) -> bool {
    native_numeric_label(node).is_some()
        || native_mdoc_search_template(node)
        || native_man_option_template(node)
        || native_man_search_template(node)
        || native_parameter_template(node)
        || native_title_head(node)
}

fn source_head_text(node: &Node) -> Option<&str> {
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    let mut current = head.children.first()?;
    loop {
        if current.kind == NodeKind::Text {
            return (!current.flags.no_print && current.children.is_empty())
                .then_some(current.text.as_deref()?);
        }
        if current.flags.no_print || current.text.is_some() || current.children.len() != 1 {
            return None;
        }
        current = &current.children[0];
    }
}

fn native_numeric_label(node: &Node) -> Option<()> {
    if node.macro_name.as_deref() != Some("IP") {
        return None;
    }
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    let label = head.children.first()?;
    if label.kind != NodeKind::Text || label.flags.no_print || !label.children.is_empty() {
        return None;
    }
    literal_numeric_label(label.text.as_deref()?).then_some(())
}

fn literal_numeric_label(text: &str) -> bool {
    // Do not evaluate roff here. Literal digits with only the classic
    // one-byte font selectors prove presentation numbering; a register,
    // expression, or escaped punctuation remains a normal declaration
    // obligation.
    let mut bytes = text.trim_matches([' ', '\t']).bytes();
    let mut digits = 0usize;
    while let Some(byte) = bytes.next() {
        if byte.is_ascii_digit() {
            digits = digits.saturating_add(1);
        } else if byte != b'\\'
            || bytes.next() != Some(b'f')
            || !matches!(bytes.next(), Some(b'B' | b'I' | b'R' | b'P' | b'1'..=b'4'))
        {
            return false;
        }
    }
    digits > 0
}

fn collect_mdoc_template_text(node: &Node, text: &mut String, has_argument: &mut bool) {
    if node.flags.no_print {
        return;
    }
    if node.macro_name.as_deref() == Some("Ar") {
        *has_argument = true;
    }
    if let Some(value) = node.text.as_deref() {
        text.push_str(value);
    }
    for child in &node.children {
        collect_mdoc_template_text(child, text, has_argument);
    }
}

fn native_mdoc_search_template(node: &Node) -> bool {
    let Some(head) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)
    else {
        return false;
    };
    let mut text = String::new();
    let mut has_argument = false;
    collect_mdoc_template_text(head, &mut text, &mut has_argument);
    has_argument && visible_text(&text).trim_start().starts_with('/')
}

fn native_man_option_template(node: &Node) -> bool {
    if !matches!(node.macro_name.as_deref(), Some("IP" | "TP")) {
        return false;
    }
    let Some(raw) = source_head_text(node) else {
        return false;
    };
    let Some((dash, parameter)) = raw.trim_matches([' ', '\t']).split_once(r"\fI") else {
        return false;
    };
    source_visible_dash(dash)
        && parameter
            .strip_suffix(r"\fR")
            .is_some_and(|name| !name.is_empty() && !name.contains(char::is_whitespace))
}

fn source_visible_dash(source: &str) -> bool {
    let mut visible = String::new();
    let mut characters = source.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            visible.push(character);
            continue;
        }
        match characters.next() {
            Some('f') if matches!(characters.next(), Some('B' | 'I' | 'R' | 'P' | '1'..='4')) => {}
            Some('-') => visible.push('-'),
            _ => return false,
        }
    }
    visible == "-"
}

fn native_man_search_template(node: &Node) -> bool {
    matches!(node.macro_name.as_deref(), Some("IP" | "TP"))
        && source_head_text(node).is_some_and(|text| {
            let text = text.trim_matches([' ', '\t']);
            if !matches!(text.as_bytes().first(), Some(b'/' | b'?')) {
                return false;
            }
            let rest = &text[1..];
            let Some(suffix) = rest.strip_prefix("RE") else {
                return false;
            };
            let Some(parameters) = suffix.strip_suffix("<carriage-return>") else {
                return false;
            };
            parameters.chars().all(|character| {
                character.is_ascii_alphabetic()
                    || matches!(character, '/' | '?' | '[' | ']' | ' ' | '-')
            })
        })
}

fn native_parameter_template(node: &Node) -> bool {
    matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "It"))
        && source_head_text(node).is_some_and(|text| {
            let text = visible_text(text);
            let text = text.trim_matches([' ', '\t']);
            text.starts_with('[')
                && text.ends_with(']')
                && text
                    .chars()
                    .any(|character| character.is_ascii_alphabetic())
        })
}

fn readable(node: &Node) -> bool {
    if node.flags.no_print || matches!(node.macro_name.as_deref(), Some("Tg" | "PD" | "Sm" | "ft"))
    {
        return false;
    }
    node.text
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty())
        || node.children.iter().any(readable)
}

fn has_readable_body(node: &Node) -> bool {
    node.children
        .iter()
        .filter(|child| child.kind == NodeKind::Body)
        .any(readable)
}

fn native_title_head(node: &Node) -> bool {
    if !matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "It")) || has_readable_body(node) {
        return false;
    }
    let Some(text) = source_head_text(node).map(str::trim) else {
        return false;
    };
    let mut characters = text.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_uppercase()
        || text.starts_with('-')
        || text.contains(['=', ':', '<', '>', '[', ']'])
    {
        return false;
    }
    if text.contains('/') {
        return text.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '/' | '-')
        });
    }
    characters.all(|character| {
        character.is_ascii_alphabetic() || character.is_ascii_whitespace() || character == '-'
    })
}
