//! Source roles for search templates; the accepted HEAD proves their join.
use std::iter::Peekable;

use crate::definitions::NativeOperandRole;
use libmandoc_rs::{
    MacroToken::{Mdoc, Roff},
    MdocMacro, Node, NodeKind, RoffMacro,
};

use crate::mandoc::roff_escape::{RoffInlineEvent, decode};

/// Search roles belong to a source expression. Only a complete, unquoted
/// expression can also provide the owner's presentation-head evidence.
#[derive(Clone, Default)]
pub(in crate::mandoc) struct SearchHead {
    arguments: Vec<u32>,
    bare_template: Option<BareTemplate>,
}

#[derive(Clone)]
struct BareTemplate {
    opener: u32,
    sign: char,
    first_argument: u32,
    closer: Option<u32>,
}

impl SearchHead {
    pub(in crate::mandoc) fn from_head(before: &[Node], head: &[Node]) -> Option<Self> {
        let mut search = Self::default();
        search.scan(head, content_nodes(before).next().is_none());
        search.arguments.sort_unstable();
        (!search.arguments.is_empty()).then_some(search)
    }

    fn scan(&mut self, head: &[Node], mut whole_head: bool) {
        let mut words = search_words(head).peekable();
        while let Some(node) = words.next() {
            if let Some(body) = quote_body(node) {
                // Quote handlers write their own words. A quoted expression
                // can still supply roles inside its own scope.
                self.scan(body, false);
            } else if let Some(opener) = text_operand(node)
                && let Some(sign) = search_sign(opener)
            {
                self.scan_form(opener, sign, &mut words, whole_head);
            }
            whole_head = false;
        }
    }

    fn scan_form<'a>(
        &mut self,
        opener: &Node,
        sign: char,
        words: &mut Peekable<impl Iterator<Item = &'a Node>>,
        mut whole_head: bool,
    ) {
        let mut first_argument = None;
        let mut closer = None;
        while let Some(word) = words.peek() {
            let start = self.arguments.len();
            if argument(word) {
                self.arguments.push(word.id);
            } else if quote_body(word).is_some_and(|body| self.append_quoted_arguments(body)) {
                whole_head = false;
            } else if inline_glyphless_word(word) {
                // Unlike Ns/Sm this operand executes term_word. A glyphless
                // inline word can be part of the source expression, but the
                // whole owner no longer matches the bare template grammar.
                whole_head = false;
            } else if first_argument.is_some()
                && closer.is_none()
                && word.kind == NodeKind::Text
                && search_sign(word) == Some(sign)
            {
                closer = Some(word.id);
            } else {
                break;
            }
            if let Some(&argument) = self.arguments.get(start) {
                first_argument.get_or_insert(argument);
            }
            words.next();
        }
        // Only accepted HEAD receipts can prove that the sign survives
        // and that term_word inserted no gap at the argument boundary.
        if let Some(first_argument) = first_argument
            && whole_head
            && words.peek().is_none()
        {
            self.bare_template = Some(BareTemplate {
                opener: opener.id,
                sign,
                first_argument,
                closer,
            });
        }
    }

    fn append_quoted_arguments(&mut self, body: &[Node]) -> bool {
        let start = self.arguments.len();
        for word in search_words(body) {
            if argument(word) {
                self.arguments.push(word.id);
            } else if !inline_glyphless_word(word)
                && !quote_body(word).is_some_and(|body| self.append_quoted_arguments(body))
            {
                self.arguments.truncate(start);
                return false;
            }
        }
        self.arguments.len() > start
    }

    pub(in crate::mandoc) fn role(&self, node: u32) -> Option<NativeOperandRole> {
        let bare = self.bare_template.as_ref()?;
        if node == bare.opener {
            Some(NativeOperandRole::SearchOpen {
                sign: bare.sign,
                argument: bare.first_argument,
                closed: bare.closer.is_some(),
            })
        } else if bare.closer == Some(node) {
            Some(NativeOperandRole::SearchClose)
        } else {
            None
        }
    }

    pub(in crate::mandoc) fn has_argument(&self, node: u32) -> bool {
        self.arguments.binary_search(&node).is_ok()
    }
}

fn quote_body(node: &Node) -> Option<&[Node]> {
    if !crate::mandoc::inline::is_enclosure_macro(node.macro_token.as_ref()) {
        return None;
    }
    match node.kind {
        NodeKind::Block => block_body(node),
        NodeKind::Element => Some(&node.children),
        _ => None,
    }
}

fn block_body(node: &Node) -> Option<&[Node]> {
    Some(
        &node
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Body && part.scope_end.is_none())?
            .children,
    )
}

fn transparent_children(node: &Node) -> Option<&[Node]> {
    match (node.macro_token.as_ref(), node.kind) {
        // Xo has no terminal pre/post handlers. No pushes the regular font
        // but otherwise executes its operands (mdoc_term.c::termp_li_pre).
        (Some(Mdoc(MdocMacro::Xo)), NodeKind::Block) => block_body(node),
        (Some(Mdoc(MdocMacro::No)), NodeKind::Element) if !node.children.is_empty() => {
            Some(&node.children)
        }
        _ => None,
    }
}

fn search_words(head: &[Node]) -> impl Iterator<Item = &Node> {
    let mut words = head.iter();
    let mut parents = Vec::new();
    std::iter::from_fn(move || {
        loop {
            let Some(node) = words.next() else {
                words = parents.pop()?;
                continue;
            };
            if control(node) {
                continue;
            }
            if let Some(children) = transparent_children(node) {
                parents.push(std::mem::replace(&mut words, children.iter()));
            } else {
                return Some(node);
            }
        }
    })
}

fn text_operand(node: &Node) -> Option<&Node> {
    match (
        node.macro_token.as_ref(),
        node.kind,
        node.children.as_slice(),
    ) {
        (_, NodeKind::Text, []) => Some(node),
        (Some(Mdoc(MdocMacro::Pf)), NodeKind::Element, [word])
            if word.kind == NodeKind::Text && !word.flags.no_print =>
        {
            Some(word)
        }
        _ => None,
    }
}

fn inline_glyphless_word(node: &Node) -> bool {
    if node.flags.line_start || node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Pf)) {
        return false;
    }
    text_operand(node)
        .and_then(|node| node.text.as_deref())
        .is_some_and(|source| {
            (source.is_empty() || source.starts_with('\\'))
                && decode(source).iter().all(|event| {
                    matches!(
                        event,
                        RoffInlineEvent::ZeroWidthGlyph
                            | RoffInlineEvent::Font(_)
                            | RoffInlineEvent::PreviousFont
                    )
                })
        })
}

fn argument(node: &Node) -> bool {
    node.kind == NodeKind::Element && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Ar))
}

/// Only the source-role walk skips these nodes. The formatter still sees the
/// original order, including node entry boundaries and native tag ownership.
pub(super) fn content_nodes(nodes: &[Node]) -> impl Iterator<Item = &Node> {
    nodes.iter().filter(|node| !control(node))
}

fn control(node: &Node) -> bool {
    node.flags.no_print
        || matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::Sm | MdocMacro::Ns | MdocMacro::Tg) | Roff(RoffMacro::Ft))
        )
}

fn search_sign(node: &Node) -> Option<char> {
    let source = node.text.as_deref()?;
    if !source.contains(['/', '?']) {
        return None;
    }
    let mut sign = None;
    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value)
                if sign.is_none() && matches!(value.as_str(), "/" | "?") =>
            {
                sign = value.chars().next();
            }
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::ZeroAdvance
            | RoffInlineEvent::NoSpace => {}
            _ => return None,
        }
    }
    sign
}
