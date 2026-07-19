//! Normalizes roff escapes and semantic mdoc macros into inline AST nodes.

use mant_ast::Inline;
use mant_mandoc_sys::{Node, NodeKind};

use super::part_children;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Font {
    Regular,
    Strong,
    Emphasis,
    Code,
}

pub(super) struct InlineBuilder {
    nodes: Vec<Inline>,
    suppress_space: bool,
}

impl InlineBuilder {
    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            suppress_space: false,
        }
    }

    pub(super) fn suppress_next_space(&mut self) {
        self.suppress_space = true;
    }

    pub(super) fn append(&mut self, mut incoming: Vec<Inline>) {
        if incoming.is_empty() {
            return;
        }
        if !self.suppress_space && needs_space(&self.nodes, &incoming) {
            push_text(&mut self.nodes, " ".to_owned());
        }
        self.suppress_space = false;
        self.nodes.append(&mut incoming);
    }

    pub(super) fn finish(self) -> Vec<Inline> {
        self.nodes
    }
}

pub(super) fn lower_inline_nodes(nodes: &[Node], default_name: Option<&str>) -> Vec<Inline> {
    let mut builder = InlineBuilder::new();
    for node in nodes {
        match node.macro_name.as_deref() {
            Some("Ns" | "Pf") => builder.suppress_next_space(),
            Some("Sm") => {}
            Some("Ap") => {
                builder.suppress_next_space();
                builder.append(vec![Inline::Text { value: "'".into() }]);
                builder.suppress_next_space();
            }
            _ => builder.append(lower_inline_node(node, default_name)),
        }
    }
    builder.finish()
}

pub(super) fn plain_text(nodes: &[Inline]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. }
            | Inline::ManualReference { children, .. } => output.push_str(&plain_text(children)),
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

fn lower_inline_node(node: &Node, default_name: Option<&str>) -> Vec<Inline> {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return Vec::new();
    }
    if node.kind == NodeKind::Text {
        return parse_roff_text(node.text.as_deref().unwrap_or_default());
    }

    let children = inline_children(node);
    let lowered = lower_inline_nodes(children, default_name);
    match node.macro_name.as_deref() {
        Some("Nm") => wrap_strong(if lowered.is_empty() {
            default_name.map_or_else(Vec::new, text_node)
        } else {
            lowered
        }),
        Some("Fl") => {
            let mut content = if plain_text(&lowered).starts_with('-') {
                Vec::new()
            } else {
                vec![Inline::Text { value: "-".into() }]
            };
            content.extend(lowered);
            wrap_strong(content)
        }
        Some("Cm" | "Ic" | "Sy" | "B" | "SB") => wrap_strong(lowered),
        Some("Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa" | "I") => wrap_emphasis(lowered),
        Some("Li") => vec![Inline::Code {
            value: plain_text(&lowered),
        }],
        Some("Xr") => lower_manual_reference(children, default_name),
        Some("Lk") => lower_link(children, default_name, false),
        Some("Mt") => lower_link(children, default_name, true),
        Some("Nd") => {
            let mut content = text_node("—");
            content.extend(lowered);
            content
        }
        Some("Op" | "Oo" | "Bq" | "Bo") => surround("[", lowered, "]"),
        Some("Dq" | "Do" | "Qq" | "Qo") => surround("“", lowered, "”"),
        Some("Sq" | "So" | "Ql") => surround("‘", lowered, "’"),
        Some("Pq" | "Po") => surround("(", lowered, ")"),
        Some("Brq" | "Bro") => surround("{", lowered, "}"),
        Some("Aq" | "Ao") => surround("<", lowered, ">"),
        _ => lowered,
    }
}

fn inline_children(node: &Node) -> &[Node] {
    let body = part_children(node, NodeKind::Body);
    if body.is_empty() {
        &node.children
    } else {
        body
    }
}

fn lower_manual_reference(children: &[Node], default_name: Option<&str>) -> Vec<Inline> {
    let values: Vec<String> = children
        .iter()
        .map(|child| plain_text(&lower_inline_node(child, default_name)))
        .filter(|value| !value.is_empty())
        .collect();
    let Some(name) = values.first().cloned() else {
        return Vec::new();
    };
    let section = values.get(1).cloned();
    let display = section
        .as_ref()
        .map_or_else(|| name.clone(), |section| format!("{name}({section})"));
    vec![Inline::ManualReference {
        name,
        section,
        children: text_node(&display),
    }]
}

fn lower_link(children: &[Node], default_name: Option<&str>, email: bool) -> Vec<Inline> {
    let Some(first) = children.first() else {
        return Vec::new();
    };
    let address = plain_text(&lower_inline_node(first, default_name));
    if address.is_empty() {
        return Vec::new();
    }
    let label = lower_inline_nodes(&children[1..], default_name);
    vec![Inline::Link {
        target: if email {
            format!("mailto:{address}")
        } else {
            address.clone()
        },
        title: None,
        children: if label.is_empty() {
            text_node(&address)
        } else {
            label
        },
    }]
}

fn wrap_strong(children: Vec<Inline>) -> Vec<Inline> {
    (!children.is_empty())
        .then_some(Inline::Strong { children })
        .into_iter()
        .collect()
}

fn wrap_emphasis(children: Vec<Inline>) -> Vec<Inline> {
    (!children.is_empty())
        .then_some(Inline::Emphasis { children })
        .into_iter()
        .collect()
}

fn surround(open: &str, mut children: Vec<Inline>, close: &str) -> Vec<Inline> {
    let mut result = text_node(open);
    result.append(&mut children);
    result.extend(text_node(close));
    result
}

fn text_node(value: &str) -> Vec<Inline> {
    vec![Inline::Text {
        value: value.to_owned(),
    }]
}

pub(super) fn parse_roff_text(source: &str) -> Vec<Inline> {
    let characters: Vec<char> = source.chars().collect();
    let mut output = Vec::new();
    let mut buffer = String::new();
    let mut font = Font::Regular;
    let mut link: Option<String> = None;
    let mut index = 0;

    while index < characters.len() {
        if characters[index] != '\\' {
            buffer.push(characters[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(escape) = characters.get(index).copied() else {
            buffer.push('\\');
            break;
        };
        index += 1;
        match escape {
            'f' => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                font = parse_font(&characters, &mut index);
            }
            'X' if characters.get(index) == Some(&'\'') => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                index += 1;
                let start = index;
                while characters.get(index) != Some(&'\'') && index < characters.len() {
                    index += 1;
                }
                let command: String = characters[start..index].iter().collect();
                index += usize::from(index < characters.len());
                if let Some(target) = command.strip_prefix("tty: link ") {
                    link = Some(target.to_owned());
                } else if command == "tty: link" {
                    link = None;
                }
            }
            '(' => {
                let name: String = characters.iter().skip(index).take(2).collect();
                index += name.chars().count();
                buffer.push_str(special_character(&name));
            }
            '[' => {
                let start = index;
                while characters.get(index) != Some(&']') && index < characters.len() {
                    index += 1;
                }
                let name: String = characters[start..index].iter().collect();
                index += usize::from(index < characters.len());
                buffer.push_str(special_character(&name));
            }
            '-' => buffer.push('-'),
            'e' | '\\' => buffer.push('\\'),
            ' ' | '~' | '0' => buffer.push(' '),
            '&' | '/' | ',' | '^' | ':' | 'c' => {}
            other => buffer.push(other),
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    output
}

fn parse_font(characters: &[char], index: &mut usize) -> Font {
    let name = match characters.get(*index) {
        Some('[') => {
            *index += 1;
            let start = *index;
            while characters.get(*index) != Some(&']') && *index < characters.len() {
                *index += 1;
            }
            let name: String = characters[start..*index].iter().collect();
            *index += usize::from(*index < characters.len());
            name
        }
        Some('(') => {
            *index += 1;
            let name: String = characters.iter().skip(*index).take(2).collect();
            *index += name.chars().count();
            name
        }
        Some(character) => {
            *index += 1;
            character.to_string()
        }
        None => return Font::Regular,
    };
    match name.as_str() {
        "B" | "3" => Font::Strong,
        "I" | "2" => Font::Emphasis,
        "CW" | "C" => Font::Code,
        _ => Font::Regular,
    }
}

fn special_character(name: &str) -> &'static str {
    match name {
        "en" => "–",
        "em" => "—",
        "aq" | "cq" => "'",
        "dq" | "lq" | "rq" => "\"",
        "co" => "©",
        "rg" => "®",
        "tm" => "™",
        "bu" => "•",
        "ha" => "^",
        "ti" => "~",
        _ => "",
    }
}

fn flush_segment(output: &mut Vec<Inline>, buffer: &mut String, font: Font, link: Option<&str>) {
    if buffer.is_empty() {
        return;
    }
    let value = std::mem::take(buffer);
    let styled = match font {
        Font::Regular => Inline::Text { value },
        Font::Strong => Inline::Strong {
            children: vec![Inline::Text { value }],
        },
        Font::Emphasis => Inline::Emphasis {
            children: vec![Inline::Text { value }],
        },
        Font::Code => Inline::Code { value },
    };
    if let Some(target) = link {
        output.push(Inline::Link {
            target: target.to_owned(),
            title: None,
            children: vec![styled],
        });
    } else {
        output.push(styled);
    }
}

fn needs_space(existing: &[Inline], incoming: &[Inline]) -> bool {
    let left = plain_text(existing).chars().next_back();
    let right = plain_text(incoming).chars().next();
    match (left, right) {
        (Some(left), Some(right)) => {
            !left.is_whitespace()
                && !right.is_whitespace()
                && !matches!(left, '(' | '[' | '{' | '<' | '“' | '‘' | '/' | '-')
                && !matches!(
                    right,
                    ')' | ']' | '}' | '>' | '”' | '’' | ',' | '.' | ':' | ';' | '!' | '?' | '/'
                )
        }
        _ => false,
    }
}

fn push_text(nodes: &mut Vec<Inline>, value: String) {
    if let Some(Inline::Text { value: previous }) = nodes.last_mut() {
        previous.push_str(&value);
    } else {
        nodes.push(Inline::Text { value });
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_roff_text, plain_text};
    use mant_ast::Inline;

    #[test]
    fn decodes_fonts_hyphens_and_renderer_links() {
        let nodes =
            parse_roff_text("\\X'tty: link https://example.test'\\fB\\-h\\fR\\X'tty: link' FILE");

        assert_eq!(plain_text(&nodes), "-h FILE");
        assert!(matches!(nodes[0], Inline::Link { .. }));
    }
}
