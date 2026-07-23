//! Normalizes roff escapes and semantic mdoc macros into inline AST nodes.

use libmandoc_rs::{Node, NodeKind};
use mant_ast::Inline;

use super::part_children;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Font {
    Regular,
    Strong,
    Emphasis,
    Code,
}

// libmandoc stores line-breaking semantics in otherwise non-printing ASCII
// bytes inside text nodes. They are parser-internal markers, not document
// characters, so translate them before they cross the ManT AST boundary.
const ASCII_BREAK: char = '\u{1d}';
const ASCII_HYPH: char = '\u{1e}';
const ASCII_NBRSP: char = '\u{1f}';

/// Replace libmandoc's internal ASCII control markers with their
/// semantic equivalents. These 0x1d–0x1f bytes encode roff-level
/// line-breaking and hyphenation hints that must never leak into
/// `ManT`'s document model (anchor IDs, section targets, etc.).
pub(super) fn sanitize_roff_text(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            ASCII_BREAK => {}                // non-breaking line break → discard
            ASCII_HYPH => output.push('-'),  // hyphenation marker → hyphen
            ASCII_NBRSP => output.push(' '), // non-breaking space → space
            other => output.push(other),
        }
    }
    output
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

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(super) fn hard_break(&mut self) {
        self.suppress_space = false;
        if plain_text(&self.nodes)
            .chars()
            .any(|character| character != '\n')
            && !matches!(self.nodes.last(), Some(Inline::LineBreak))
        {
            self.nodes.push(Inline::LineBreak);
        }
    }

    pub(super) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming, false);
    }

    /// Append content beginning on a later filled roff input line.
    ///
    /// Filled lines contribute a word boundary even when the next visible
    /// character is punctuation. Macro arguments do not use this path because
    /// their own concatenation rules are authoritative.
    pub(super) fn append_across_source_line(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming, true);
    }

    fn append_at_boundary(&mut self, incoming: &mut Vec<Inline>, source_line_changed: bool) {
        if incoming.is_empty() {
            return;
        }
        let add_space = if source_line_changed {
            needs_filled_line_space(&self.nodes, incoming)
        } else {
            needs_space(&self.nodes, incoming)
        };
        if !self.suppress_space && add_space {
            push_text(&mut self.nodes, " ".to_owned());
        }
        self.suppress_space = false;
        self.nodes.append(incoming);
    }

    pub(super) fn finish(mut self) -> Vec<Inline> {
        while matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.pop();
        }
        self.nodes
    }
}

pub(super) fn lower_inline_nodes(nodes: &[Node], default_name: Option<&str>) -> Vec<Inline> {
    let mut builder = InlineBuilder::new();
    for node in nodes {
        match node.macro_name.as_deref() {
            Some("Ns" | "Pf") => builder.suppress_next_space(),
            // A roff break ends the current output line, not the paragraph.
            // Keeping it inline lets every renderer preserve the same flow.
            Some("br") => builder.hard_break(),
            // Formatting requests carry control arguments such as `CW` and
            // `R`. They change renderer state and are never document text.
            // Verbatim regions already retain their semantics through
            // libmandoc's no-fill flag, so leaking these arguments would only
            // create phantom paragraphs around preformatted blocks.
            Some(
                "Sm" | "PD" | "ad" | "fi" | "ft" | "hy" | "in" | "na" | "ne" | "nf" | "nh" | "nr"
                | "ta",
            ) => {}
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

pub(crate) fn plain_text(nodes: &[Inline]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => output.push_str(&plain_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

/// man(7) hanging tags no wider than this render inline with the first line of
/// their description, matching man(1)'s default `.TP`/`.IP` indent; wider tags
/// take their own line. Computed once in the model so every renderer agrees.
pub(crate) const INLINE_TERM_MAX_WIDTH: usize = 6;

/// Decide whether a definition item's term(s) hang inline with the first line
/// of the description, mirroring how renderers join multiple terms with `", "`.
pub(crate) fn terms_fit_inline(terms: &[Vec<Inline>]) -> bool {
    let width = terms
        .iter()
        .map(|term| plain_text(term))
        .collect::<Vec<_>>()
        .join(", ")
        .trim()
        .chars()
        .count();
    (1..=INLINE_TERM_MAX_WIDTH).contains(&width)
}

fn lower_inline_node(node: &Node, default_name: Option<&str>) -> Vec<Inline> {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return Vec::new();
    }
    if node.kind == NodeKind::Text {
        return lower_text_node(node, Font::Regular);
    }

    let macro_name = node.macro_name.as_deref();
    let children = inline_children(node);
    // man(7) alternating-font macros concatenate their arguments without
    // inserting spaces. Each argument switches to the next named font.
    let lowered = alternating_font_pair(macro_name).map_or_else(
        || lower_inline_nodes(children, default_name),
        |(first, second)| lower_alternating_fonts(children, default_name, first, second),
    );
    let anchor = navigation_anchor(node, &lowered);
    let mut output = match macro_name {
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
        // Keep the heading text as a private unresolved target until the
        // complete section tree is available. The document post-pass replaces
        // it with the stable Section::id or degrades it to ordinary text.
        Some("Sx") if !lowered.is_empty() => vec![Inline::SectionReference {
            target: plain_text(&lowered).trim().to_owned(),
            children: lowered,
        }],
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
    };
    if let Some(anchor) = anchor {
        output.insert(0, anchor);
    }
    output
}

/// Convert libmandoc's validated deep-link marker into a zero-width AST node.
/// Explicit `.Tg` tags carry `node.tag`; automatically discovered tags fall
/// back to the same first visible word that libmandoc uses.
fn navigation_anchor(node: &Node, lowered: &[Inline]) -> Option<Inline> {
    if !node.flags.deep_link_target {
        return None;
    }
    let id = node.tag.as_deref().map(sanitize_roff_text).or_else(|| {
        plain_text(lowered)
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
    })?;
    (!id.is_empty()).then_some(Inline::Anchor { id })
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
    let children = if label.is_empty() {
        text_node(&address)
    } else {
        label
    };
    if email {
        vec![Inline::EmailLink { address, children }]
    } else {
        vec![Inline::ExternalLink {
            uri: address,
            title: None,
            children,
        }]
    }
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

fn lower_alternating_fonts(
    children: &[Node],
    default_name: Option<&str>,
    first: Font,
    second: Font,
) -> Vec<Inline> {
    let mut output = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let font = if index % 2 == 0 { first } else { second };
        // An alternating man(7) macro establishes the *initial* font for
        // each argument. Explicit `\\f` escapes inside that argument must
        // still be able to reset or replace it; wrapping an already-lowered
        // argument would incorrectly nest the outer font around the reset.
        let lowered = if child.kind == NodeKind::Text {
            lower_text_node(child, font)
        } else {
            apply_font(lower_inline_node(child, default_name), font)
        };
        output.extend(lowered);
    }
    output
}

fn alternating_font_pair(macro_name: Option<&str>) -> Option<(Font, Font)> {
    match macro_name {
        Some("BI") => Some((Font::Strong, Font::Emphasis)),
        Some("BR") => Some((Font::Strong, Font::Regular)),
        Some("IB") => Some((Font::Emphasis, Font::Strong)),
        Some("IR") => Some((Font::Emphasis, Font::Regular)),
        Some("RB") => Some((Font::Regular, Font::Strong)),
        Some("RI") => Some((Font::Regular, Font::Emphasis)),
        _ => None,
    }
}

fn apply_font(children: Vec<Inline>, font: Font) -> Vec<Inline> {
    match font {
        Font::Regular => children,
        Font::Strong => wrap_strong(children),
        Font::Emphasis => wrap_emphasis(children),
        Font::Code => (!children.is_empty())
            .then(|| Inline::Code {
                value: plain_text(&children),
            })
            .into_iter()
            .collect(),
    }
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
    parse_roff_text_with_font(source, Font::Regular)
}

/// Decode one roff text run using the font selected by its enclosing macro.
/// Explicit `\\f` escapes change `font` while the run is scanned, so a reset
/// to regular text remains visible even inside an alternating `.BI` argument.
fn parse_roff_text_with_font(source: &str, initial_font: Font) -> Vec<Inline> {
    let characters: Vec<char> = source.chars().collect();
    let mut output = Vec::new();
    let mut buffer = String::new();
    let mut font = initial_font;
    let mut link: Option<String> = None;
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];
        if character != '\\' {
            match character {
                ASCII_BREAK => {}
                ASCII_HYPH => buffer.push('-'),
                ASCII_NBRSP => buffer.push(' '),
                _ => buffer.push(character),
            }
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
            // These requests affect formatter state or introduce zero-width
            // hints. They are not printable versions of their trigger byte.
            '!' | '?' | '%' | '&' | ')' | ',' | '/' | '^' | ':' | 'a' | 'c' | 'd' | 'r' | 't'
            | 'u' | '{' | '|' | '}' => {}
            other => buffer.push(other),
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    output
}

/// Lower a text node after honoring a macro-provided default font. Nodes marked
/// non-printing by libmandoc are never allowed to escape through this shortcut.
fn lower_text_node(node: &Node, initial_font: Font) -> Vec<Inline> {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        Vec::new()
    } else {
        parse_roff_text_with_font(node.text.as_deref().unwrap_or_default(), initial_font)
    }
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
        output.push(Inline::ExternalLink {
            uri: target.to_owned(),
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

fn needs_filled_line_space(existing: &[Inline], incoming: &[Inline]) -> bool {
    matches!(
        (
            plain_text(existing).chars().next_back(),
            plain_text(incoming).chars().next(),
        ),
        (Some(left), Some(right)) if !left.is_whitespace() && !right.is_whitespace()
    )
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
    use super::{ASCII_BREAK, ASCII_HYPH, ASCII_NBRSP, parse_roff_text, plain_text};
    use mant_ast::Inline;

    #[test]
    fn decodes_fonts_hyphens_and_renderer_links() {
        let nodes =
            parse_roff_text("\\X'tty: link https://example.test'\\fB\\-h\\fR\\X'tty: link' FILE");

        assert_eq!(plain_text(&nodes), "-h FILE");
        assert!(matches!(nodes[0], Inline::ExternalLink { .. }));
    }

    #[test]
    fn decodes_libmandoc_internal_breaking_markers() {
        let source = format!("git{ASCII_HYPH}config{ASCII_NBRSP}(1){ASCII_BREAK}next");

        assert_eq!(plain_text(&parse_roff_text(&source)), "git-config (1)next");
    }

    #[test]
    fn removes_roff_layout_escapes_without_hiding_literal_punctuation() {
        let source = r"[\|optional\|]\&.\|.\|. \||\|";

        assert_eq!(plain_text(&parse_roff_text(source)), "[optional]... |");
    }
}
