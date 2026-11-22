//! Source roles for search templates; the accepted HEAD proves their join.
use std::iter::Peekable;

use crate::definitions::NativeOperandRole;
use libmandoc_rs::{
    MacroToken::{Mdoc, Roff},
    MdocMacro, Node, NodeKind, RoffMacro,
};

use crate::mandoc::roff_escape::{RoffInlineEvent, decode, decode_with_status};
use crate::mandoc::{containers::TextScope, inline::mdoc_text_font};

/// Search roles belong to a source expression. Only a complete, unquoted
/// expression can also provide the owner's presentation-head evidence.
#[derive(Clone, Default)]
pub(in crate::mandoc) struct SearchHead {
    arguments: Vec<u32>,
    signs: Vec<u32>,
    bare_template: Option<BareTemplate>,
}

#[derive(Clone)]
struct BareTemplate {
    opener: u32,
    sign: char,
    first_argument: u32,
    closer: Option<u32>,
}

#[derive(Clone, Copy)]
struct SearchSign {
    glyph: char,
    proven: bool,
}

#[derive(Default)]
struct QuotedArguments {
    first: Option<u32>,
    complete: bool,
}

impl SearchHead {
    pub(in crate::mandoc) fn from_head(before: &[Node], head: &[Node]) -> Option<Self> {
        let mut search = Self::default();
        search.scan(head, content_nodes(before).next().is_none());
        search.arguments.sort_unstable();
        search.signs.sort_unstable();
        (!search.arguments.is_empty()).then_some(search)
    }

    fn scan(&mut self, head: &[Node], whole_head: bool) {
        self.scan_words(&mut search_words(head).peekable(), whole_head);
    }

    fn scan_words<'a>(
        &mut self,
        words: &mut Peekable<impl Iterator<Item = &'a Node>>,
        mut whole_head: bool,
    ) {
        while let Some(node) = words.next() {
            if let Some(body) = quote_body(node) {
                // Quote handlers write their own words. A quoted expression
                // can still supply roles inside its own scope.
                self.scan(body, false);
            } else if let Some(opener) = text_operand(node)
                && let Some(sign) = search_sign(opener)
            {
                self.scan_form(opener, sign, words, whole_head);
            }
            whole_head = false;
        }
    }

    fn scan_form<'a>(
        &mut self,
        opener: &Node,
        sign: SearchSign,
        words: &mut Peekable<impl Iterator<Item = &'a Node>>,
        mut whole_head: bool,
    ) {
        whole_head &= sign.proven;
        let mut first_argument = None;
        let mut closer = None;
        while let Some(&word) = words.peek() {
            if argument(word) {
                self.arguments.push(word.id);
                first_argument.get_or_insert(word.id);
            } else if let Some(body) = quote_body(word) {
                whole_head = false;
                let prefix = self.scan_quoted_arguments(body);
                first_argument = first_argument.or(prefix.first);
                words.next();
                if !prefix.complete || prefix.first.is_none() {
                    break;
                }
                continue;
            } else if inline_glyphless_word(word) {
                // Unlike Ns/Sm this operand executes term_word. A glyphless
                // inline word can be part of the source expression, but the
                // whole owner no longer matches the bare template grammar.
                whole_head = false;
            } else if first_argument.is_some()
                && closer.is_none()
                && let Some(operand) = text_operand(word)
                && let Some(close) = search_sign(operand)
                && close.glyph == sign.glyph
            {
                closer = Some(operand.id);
                whole_head &= close.proven;
            } else {
                break;
            }
            words.next();
        }
        // Only accepted HEAD receipts can prove that the sign survives
        // and that term_word inserted no gap at the argument boundary.
        if let Some(first_argument) = first_argument {
            if whole_head && words.peek().is_none() {
                self.bare_template = Some(BareTemplate {
                    opener: opener.id,
                    sign: sign.glyph,
                    first_argument,
                    closer,
                });
            } else {
                self.signs.push(opener.id);
                self.signs.extend(closer);
            }
        }
    }

    fn scan_quoted_arguments(&mut self, body: &[Node]) -> QuotedArguments {
        let mut words = search_words(body).peekable();
        let mut prefix = QuotedArguments::default();
        prefix.complete = loop {
            let Some(&word) = words.peek() else {
                break true;
            };
            if argument(word) {
                self.arguments.push(word.id);
                prefix.first.get_or_insert(word.id);
            } else if let Some(body) = quote_body(word) {
                let nested = self.scan_quoted_arguments(body);
                prefix.first = prefix.first.or(nested.first);
                words.next();
                if !nested.complete || nested.first.is_none() {
                    break false;
                }
                continue;
            } else if !inline_glyphless_word(word) {
                break false;
            }
            words.next();
        };
        // The inherited sign ends at this boundary. Continue from the
        // unconsumed word so a later, independent sign can still start its
        // own expression. The prefix retains its original Ar identities.
        self.scan_words(&mut words, false);
        prefix
    }

    pub(in crate::mandoc) fn role(&self, node: u32) -> Option<NativeOperandRole> {
        if let Some(bare) = &self.bare_template
            && node == bare.opener
        {
            Some(NativeOperandRole::SearchOpen {
                sign: bare.sign,
                argument: bare.first_argument,
                closed: bare.closer.is_some(),
            })
        } else if self
            .bare_template
            .as_ref()
            .is_some_and(|bare| bare.closer == Some(node))
        {
            Some(NativeOperandRole::SearchClose)
        } else if self.signs.binary_search(&node).is_ok() {
            Some(NativeOperandRole::SearchSign)
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
            let token = node.macro_token.as_ref();
            match (TextScope::from_token(token), node.kind) {
                (Some(scope), NodeKind::Block) => {
                    parents.push(std::mem::take(&mut words));
                    parents.extend(
                        node.children
                            .iter()
                            .rev()
                            .map(|part| scope.part_words(part).iter()),
                    );
                }
                (Some(_), _) if node.scope_end.is_some() => {
                    parents.push(std::mem::replace(&mut words, node.children.iter()));
                }
                (_, NodeKind::Element)
                    if mdoc_text_font(token).is_some() && !node.children.is_empty() =>
                {
                    parents.push(std::mem::replace(&mut words, node.children.iter()));
                }
                _ => return Some(node),
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

fn search_sign(node: &Node) -> Option<SearchSign> {
    let source = node.decoder_text()?;
    if !source.contains(['/', '?', '\\']) {
        return None;
    }
    let decoded = decode_with_status(source);
    let proven = decoded.proves_native_glyphs();
    let mut sign = None;
    for event in decoded.events {
        match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _)
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
    // term.c::term_word() decodes SPECIAL/UNICODE/NUMBERED before
    // encode1() applies BACKAFTER. A recovered spelling can still protect
    // the parameter, but it cannot prove a surviving native template.
    Some(SearchSign {
        glyph: sign?,
        proven,
    })
}
