//! Independent source-side declaration and presentation checks.
use crate::inline_text;
use libmandoc_rs::{Node, NodeKind};
use mant_ir::DefinitionItem;

pub(super) fn readable(node: &Node) -> bool {
    if node.flags.no_print || matches!(node.macro_token.as_deref(), Some("Tg" | "PD" | "Sm" | "ft"))
    {
        return false;
    }
    node.text.as_ref().is_some_and(|t| !t.trim().is_empty()) || node.children.iter().any(readable)
}
pub(super) fn body(node: &Node) -> bool {
    node.children
        .iter()
        .filter(|n| n.kind == NodeKind::Body)
        .any(readable)
}
pub(super) fn paragraph_boundary(node: &Node) -> bool {
    matches!(
        node.macro_token.as_deref(),
        Some("PP" | "P" | "LP" | "Pp" | "sp" | "br")
    ) || node
        .children
        .iter()
        .filter(|n| n.kind == NodeKind::Body)
        .any(paragraph_boundary)
}

pub(super) fn bracket_head(node: &Node, item: &DefinitionItem) -> bool {
    // mdoc quote handlers can generate the same visible bracket around
    // arbitrary syntax. Only an authored literal supplies this exemption.
    if node.macro_token.as_deref() == Some("It") && source_head_text(node).is_none() {
        return false;
    }
    // Native text can still contain font escapes before `[`. Inspect the
    // source-correlated visible head, not raw roff bytes or a derived ID.
    item.terms
        .first()
        .is_some_and(|term| inline_text(term).trim_start().starts_with('['))
}

fn native_numeric_label(node: &Node) -> Option<String> {
    // IP's first HEAD child is the label; subsequent children are width
    // operands. Other macro shapes need their own independent source witness.
    if node.macro_token.as_deref() != Some("IP") {
        return None;
    }
    let head = node.children.iter().find(|n| n.kind == NodeKind::Head)?;
    let label = head.children.first()?;
    if label.kind != NodeKind::Text || label.flags.no_print || !label.children.is_empty() {
        return None;
    }
    literal_numeric_label(label.text.as_deref()?)
}

pub(super) fn literal_numeric_label(text: &str) -> Option<String> {
    // This is deliberately not a roff evaluator. Only literal digits and
    // known one-character font switches prove a numeric label; every other
    // escape, request, or internal space leaves the source obligation intact.
    let mut bytes = text.trim_matches([' ', '\t']).bytes();
    let mut label = String::new();
    while let Some(byte) = bytes.next() {
        if byte.is_ascii_digit() {
            label.push(char::from(byte));
        } else if byte != b'\\'
            || bytes.next() != Some(b'f')
            || !matches!(bytes.next(), Some(b'B' | b'I' | b'R' | b'P' | b'1'..=b'4'))
        {
            return None;
        }
    }
    (!label.is_empty()).then_some(label)
}

pub(super) fn unsigned_numeric_head(node: &Node, item: &DefinitionItem) -> bool {
    let [term] = item.terms.as_slice() else {
        return false;
    };
    let text = inline_text(term);
    native_numeric_label(node).is_some_and(|label| label == text.trim_matches([' ', '\t']))
}

pub(super) fn source_presentation_head(node: &Node, item: &DefinitionItem) -> bool {
    // The AST remains immutable during corruption tests. An empty name set
    // or a template-like spelling alone never permits an omitted owner.
    item.entry
        .as_ref()
        .is_none_or(|entry| entry.names.is_empty())
        && (native_mdoc_search_template(node, item)
            || native_man_option_template(node)
            || native_man_search_template(node)
            || native_title_head(node))
}

pub(super) fn native_mdoc_search_template(node: &Node, item: &DefinitionItem) -> bool {
    if node.macro_token.as_deref() != Some("It") {
        return false;
    }
    let Some(part) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)
    else {
        return false;
    };
    if node
        .children
        .iter()
        .take_while(|child| child.kind != NodeKind::Head)
        .any(|child| !search_control(child))
    {
        return false;
    }
    let mut words = Vec::new();
    collect_search_words(&part.children, &mut words);
    let mut source = words.into_iter();
    let Some(prefix) = source.next() else {
        return false;
    };
    let Some(raw_sign) = text_operand(prefix) else {
        return false;
    };
    let Some(sign) = literal_search_sign(raw_sign) else {
        return false;
    };
    let Some(argument) = source
        .next()
        .filter(|arg| arg.macro_token.as_deref() == Some("Ar"))
    else {
        return false;
    };
    let Some(argument_start) = literal_argument_start(argument) else {
        return false;
    };
    if argument_start == sign.glyph && sign.zero_advance {
        return false;
    }
    let mut closing = None;
    for operand in source {
        if operand.macro_token.as_deref() == Some("Ar") {
            // Detached IR cannot show that a zero-advance closer still owns
            // its glyph once a later word can write to that position.
            if closing.is_some_and(|close: SourceSign| close.zero_advance) && readable(operand) {
                return false;
            }
            continue;
        }
        let Some(close) = text_operand(operand).and_then(literal_search_sign) else {
            return false;
        };
        if closing.is_some() || close.glyph != sign.glyph {
            return false;
        }
        closing = Some(close);
    }
    // Correlate the immutable operand grammar with the complete form of
    // this physical owner. Sm and input continuations can join across input
    // lines; neither the producer's name inference nor a second spacing
    // interpreter can tell this observer what survived execution.
    let [term] = item.terms.as_slice() else {
        return false;
    };
    let text = inline_text(term);
    text.strip_prefix(sign.glyph).is_some_and(|tail| {
        !tail.contains('\n')
            && !argument_start.is_whitespace()
            && tail.starts_with(argument_start)
            && (closing.is_none() || tail.contains(sign.glyph))
    })
}

fn text_operand(node: &Node) -> Option<&str> {
    match (
        node.kind,
        node.macro_token.as_deref(),
        node.children.as_slice(),
    ) {
        (NodeKind::Text, _, []) => node.text.as_deref(),
        (NodeKind::Element, Some("Pf"), [word])
            if word.kind == NodeKind::Text && !word.flags.no_print =>
        {
            word.text.as_deref()
        }
        _ => None,
    }
}

fn search_control(node: &Node) -> bool {
    node.flags.no_print || matches!(node.macro_token.as_deref(), Some("Ns" | "Sm" | "Tg" | "ft"))
}

fn collect_search_words<'a>(nodes: &'a [Node], words: &mut Vec<&'a Node>) {
    for node in nodes.iter().filter(|node| !search_control(node)) {
        // Keep the source observer independent from producer roles. CVS's
        // Bf/Bk handlers skip their selector HEAD but retain punctuation
        // before it. Xo has no terminal handler. Style elements change only
        // their own operands' font, without absorbing separately typed Ar.
        let token = node.macro_token.as_deref();
        match (token, node.kind) {
            (Some("Xo" | "Bf" | "Bk"), NodeKind::Block) => {
                for part in &node.children {
                    match part.kind {
                        NodeKind::Head if token != Some("Xo") => {}
                        NodeKind::Head | NodeKind::Body | NodeKind::Tail => {
                            collect_search_words(&part.children, words);
                        }
                        _ => collect_search_words(std::slice::from_ref(part), words),
                    }
                }
            }
            (Some("Xo" | "Bf" | "Bk"), _) if node.scope_end.is_some() => {
                collect_search_words(&node.children, words);
            }
            (Some("No" | "Sy" | "Em" | "Li"), NodeKind::Element) if !node.children.is_empty() => {
                collect_search_words(&node.children, words);
            }
            _ => words.push(node),
        }
    }
}

fn literal_argument_start(argument: &Node) -> Option<char> {
    // An observer without the producer's word receipts can corroborate a
    // literal lead glyph. An empty Ar or a motion at its start cannot prove
    // that a later argument didn't supply the first accepted scalar.
    for word in argument.children.iter().filter(|word| !word.flags.no_print) {
        let mut source = word.text.as_deref()?.chars();
        while let Some(character) = source.next() {
            if character != '\\' {
                return Some(character);
            }
            match escape_trigger(&mut source)? {
                '%' | '&' | ')' | ',' | '/' | '^' | 'a' | 'd' | 't' | 'u' | '{' | '|' | '}' => {}
                'f' => skip_font(&mut source)?,
                trigger => return source_character(trigger, &mut source),
            }
        }
    }
    None
}

fn skip_font(source: &mut std::str::Chars<'_>) -> Option<()> {
    let mut operand = String::new();
    match source.next() {
        Some('[') => operand = named_operand(source, ']')?,
        Some('(') => operand.extend([source.next()?, source.next()?]),
        Some(name) => operand.push(name),
        None => {}
    }
    matches!(
        operand.as_str(),
        "" | "1"
            | "2"
            | "3"
            | "4"
            | "B"
            | "I"
            | "P"
            | "R"
            | "BI"
            | "CB"
            | "CI"
            | "CR"
            | "CW"
            | "C"
            | "V"
            | "VB"
            | "VI"
    )
    .then_some(())
}

#[derive(Clone, Copy)]
struct SourceSign {
    glyph: char,
    zero_advance: bool,
}

fn literal_search_sign(text: &str) -> Option<SourceSign> {
    // CVS roff_escape.c and term_word() consume the one-byte IGNORE escapes
    // and font selectors without printing another glyph.
    let mut sign = None;
    let mut zero_advance = false;
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        match character {
            '/' | '?' if sign.is_none() => sign = Some(character),
            '\\' => match escape_trigger(&mut chars)? {
                '%' | '&' | ')' | ',' | '/' | '^' | 'a' | 'd' | 't' | 'u' | '{' | '|' | '}' => {}
                'z' => zero_advance |= sign.is_none(),
                'c' => zero_advance &= sign.is_some(),
                'f' => skip_font(&mut chars)?,
                trigger if sign.is_none() => {
                    let glyph = source_character(trigger, &mut chars)?;
                    sign = matches!(glyph, '/' | '?').then_some(glyph);
                    sign?;
                }
                _ => return None,
            },
            _ => return None,
        }
    }
    Some(SourceSign {
        glyph: sign?,
        zero_advance,
    })
}

fn escape_trigger(source: &mut std::str::Chars<'_>) -> Option<char> {
    source.find(|&trigger| trigger != 'E')
}

fn named_operand(source: &mut std::str::Chars<'_>, delimiter: char) -> Option<String> {
    let name: String = source.as_str().split_once(delimiter)?.0.into();
    if name.contains('\\') {
        return None;
    }
    source.nth(name.chars().count())?;
    Some(name)
}

/// Independently corroborate a scalar source operand against the pinned
/// `chars.c` catalog and `roff_escape.c`'s Unicode/numbered spellings. This
/// observer does not interpret motion, rendering recovery or source macros.
fn source_character(trigger: char, source: &mut std::str::Chars<'_>) -> Option<char> {
    let name = match trigger {
        '(' => String::from_iter([source.next()?, source.next()?]),
        '[' => named_operand(source, ']')?,
        'C' => {
            let delimiter = source.next()?;
            named_operand(source, delimiter)?
        }
        'N' => {
            let delimiter = source.next()?;
            if " %&()*+-./0123456789:<=>\\".contains(delimiter) {
                return None;
            }
            let mut digits = String::new();
            loop {
                let character = source.next()?;
                if !character.is_ascii_digit() {
                    break;
                }
                digits.push(character);
            }
            return digits.parse::<u8>().ok().map(char::from);
        }
        _ => return None,
    };
    if name.len() <= 1 && name != "-" {
        return None;
    }
    if let Some(digits) = name.strip_prefix('u')
        && (4..=6).contains(&digits.len())
        && (digits.len() == 4 || !digits.starts_with('0'))
        && digits.bytes().all(|digit| digit.is_ascii_hexdigit())
    {
        return char::from_u32(u32::from_str_radix(digits, 16).ok()?);
    }
    match libmandoc_rs::special_character(&name) {
        Some(libmandoc_rs::SpecialCharacter::Visible(glyph)) => Some(glyph),
        _ => None,
    }
}

fn source_head_text(node: &Node) -> Option<&str> {
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    let mut current = if node.macro_token.as_deref() == Some("It") {
        let mut visible = head.children.iter().filter(|child| !search_control(child));
        let first = visible.next()?;
        if visible.next().is_some() {
            return None;
        }
        first
    } else {
        head.children.first()?
    };
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

fn native_man_option_template(node: &Node) -> bool {
    // Perl-generated man pages use a historical spelling such as
    // `.IP \fB-\fR\fImin-len\fR`: it displays a dash followed by an italic
    // placeholder, rather than an option named `-min-len`. mandoc's man
    // formatter deliberately treats the head as visible layout text. Admit
    // this exact source witness as presentation-only so a later `-n`/`--long`
    // semantic group may still be proven, but never generalize it to a real
    // dash option or a damaged final owner.
    if !matches!(node.macro_token.as_deref(), Some("IP" | "TP")) {
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
    // Interpret only the historical two-byte font changes and escaped hyphen
    // emitted by generated man pages. This is intentionally not a roff
    // evaluator: an arbitrary escape cannot prove a presentation template.
    // The traditional bold/escaped-hyphen/reset and bold/hyphen/reset forms
    // both render exactly one dash.
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

pub(super) fn native_man_search_template(node: &Node) -> bool {
    // Historic nvi/ex manuals encode search commands as a man TP head with an
    // RE placeholder and a carriage-return marker. It is visible syntax, not
    // a literal command that can be selected safely. Modern mdoc forms are
    // handled separately by native_mdoc_search_template.
    matches!(node.macro_token.as_deref(), Some("IP" | "TP"))
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

fn native_title_head(node: &Node) -> bool {
    // A plain, bodyless title in native list layout is presentation structure
    // rather than a selector. This covers generated contents/taxonomy labels
    // while preserving styled syntax, lower-case commands, option spelling,
    // and every label with readable owner content as audit obligations.
    if !matches!(node.macro_token.as_deref(), Some("IP" | "TP" | "It")) || body(node) {
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
