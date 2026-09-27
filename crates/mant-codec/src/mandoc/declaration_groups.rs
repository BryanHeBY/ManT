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

fn source_head_node(node: &Node) -> Option<&Node> {
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    // CVS man_term.c::pre_TP skips same-line width operands and prints only
    // NODE_LINE children. IP and It use their existing first-head contract.
    if node.macro_name.as_deref() == Some("TP") {
        head.children.iter().find(|child| child.flags.line_start)
    } else {
        head.children.first()
    }
}

fn source_head_text(node: &Node) -> Option<&str> {
    let mut current = source_head_node(node)?;
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
        && source_head_node(node).is_some_and(|head| {
            // A bare text HEAD can be a presentation-only `[ argument ]`.
            // The initial B/Cm/Ic/Fl macro is authored declaration evidence,
            // even when it happens to print bracketed glyphs. Do not let a
            // template exception override that independent native role.
            if head.kind != NodeKind::Text || head.flags.no_print || !head.children.is_empty() {
                return false;
            }
            let Some(text) = head.text.as_deref() else {
                return false;
            };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn source_block<'a>(node: &'a Node, macro_name: &str, line: u32) -> Option<&'a Node> {
        (node.kind == NodeKind::Block
            && node.macro_name.as_deref() == Some(macro_name)
            && node.line == line)
            .then_some(node)
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| source_block(child, macro_name, line))
            })
    }

    #[test]
    fn tp_source_templates_use_the_executed_head_and_not_an_authored_macro_role() {
        // All exact inputs ran the pinned CVS -Tutf8 reference first. CVS
        // man_term.c::pre_TP skips same-line width operands; pre_B and
        // mdoc_term.c::termp_bold_pre execute explicit B/Cm instances as
        // visible heads, not as an unstyled parameter-only template.
        for (source, macro_name, line, visible, parameter) in [
            (
                b".TH PROBE 1\n.SH OPTIONS\n.TP [hidden]\n.B --actual\nActual description.\n"
                    .as_slice(),
                "TP",
                3,
                Some("--actual"),
                false,
            ),
            (
                b".TH PROBE 1\n.SH COMMANDS\n.TP [foo]\n.B [foo]\n.TP\n.B bar\nBody.\n",
                "TP",
                3,
                Some("[foo]"),
                false,
            ),
            (
                b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP 4\n\\fB      \\fP[ \\fIargument\\fP ]\n.TP\n.B second\n.TP\n.B third\nBody.\n",
                "TP",
                5,
                Some("\\fB      \\fP[ \\fIargument\\fP ]"),
                true,
            ),
            (
                b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag\n.It Cm [foo]\n.It Cm bar\nBody.\n.El\n",
                "It",
                6,
                Some("[foo]"),
                false,
            ),
        ] {
            let parsed = libmandoc_rs::Parser::default()
                .parse_bytes("probe.1", source)
                .expect("parse exact pinned-CVS input");
            let block = source_block(&parsed.document.root, macro_name, line)
                .expect("native source block");
            assert_eq!(source_head_text(block), visible, "{macro_name} line {line}");
            assert_eq!(
                native_parameter_template(block),
                parameter,
                "{macro_name} line {line}"
            );
        }
    }
}
