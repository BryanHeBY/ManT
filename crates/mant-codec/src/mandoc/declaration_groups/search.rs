//! Source roles for search templates; the accepted HEAD proves their join.
use crate::definitions::NativeOperandRole;
use libmandoc_rs::{
    MacroToken::{Mdoc, Roff},
    MdocMacro, Node, NodeKind, RoffMacro,
};

use crate::mandoc::roff_escape::{RoffInlineEvent, decode};

#[derive(Clone)]
pub(in crate::mandoc) struct SearchTemplate {
    opener: u32,
    sign: char,
    first_argument: u32,
    arguments: Vec<u32>,
    closer: Option<u32>,
    bare_template: bool,
}

impl SearchTemplate {
    pub(in crate::mandoc) fn from_head(before: &[Node], mut head: &[Node]) -> Option<Self> {
        if before.iter().any(|node| !control(node)) {
            return None;
        }
        let mut bare_template = true;
        while let [wrapper] = head {
            let token = wrapper.macro_token.as_ref();
            let quote = crate::mandoc::inline::is_enclosure_macro(token);
            if wrapper.flags.no_print || (!quote && token != Some(&Mdoc(MdocMacro::Xo))) {
                break;
            }
            // Xo is transparent in mdoc_term.c's dispatch. The quote
            // handlers write their own words. Preserve the Ar role inside
            // them, without using that enclosed head as a bare template.
            bare_template &= !quote;
            head = match wrapper.kind {
                NodeKind::Block => {
                    &wrapper
                        .children
                        .iter()
                        .find(|part| part.kind == NodeKind::Body && part.scope_end.is_none())?
                        .children
                }
                NodeKind::Element => &wrapper.children,
                _ => return None,
            };
        }
        // termp_sm_pre(), termp_ns_pre() and termp_pf_post() change the
        // state consumed by term_word(). The AST alone cannot prove spacing,
        // even if both operands come from the same input line.
        let mut words = head.iter().filter(|node| !control(node));
        let prefix = words.next()?;
        let opener = match (
            prefix.macro_token.as_ref(),
            prefix.kind,
            prefix.children.as_slice(),
        ) {
            (_, NodeKind::Text, []) => prefix,
            (Some(Mdoc(MdocMacro::Pf | MdocMacro::No)), NodeKind::Element, [word])
                if word.kind == NodeKind::Text && !word.flags.no_print =>
            {
                word
            }
            _ => return None,
        };
        let first_argument = words.next().filter(|word| argument(word))?;
        let sign = search_sign(opener)?;
        let mut arguments = vec![first_argument.id];
        let mut closer = None;
        for word in words {
            if argument(word) {
                arguments.push(word.id);
                continue;
            }
            if closer.is_some() || word.kind != NodeKind::Text || search_sign(word) != Some(sign) {
                // A later independent declaration is outside the template,
                // but cannot turn the earlier Ar into a standalone label.
                bare_template = false;
                break;
            }
            closer = Some(word.id);
        }
        arguments.sort_unstable();
        Some(Self {
            opener: opener.id,
            sign,
            first_argument: first_argument.id,
            arguments,
            closer,
            bare_template,
        })
    }

    pub(in crate::mandoc) fn role(&self, node: u32) -> Option<NativeOperandRole> {
        if !self.bare_template {
            None
        } else if node == self.opener {
            Some(NativeOperandRole::SearchOpen {
                sign: self.sign,
                argument: self.first_argument,
                closed: self.closer.is_some(),
            })
        } else if self.closer == Some(node) {
            Some(NativeOperandRole::SearchClose)
        } else {
            None
        }
    }

    pub(in crate::mandoc) fn has_argument(&self, node: u32) -> bool {
        self.arguments.binary_search(&node).is_ok()
    }
}

fn argument(node: &Node) -> bool {
    node.kind == NodeKind::Element && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Ar))
}

pub(super) fn control(node: &Node) -> bool {
    node.flags.no_print
        || matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::Sm | MdocMacro::Ns | MdocMacro::Tg) | Roff(RoffMacro::Ft))
        )
}

fn search_sign(node: &Node) -> Option<char> {
    let mut sign = None;
    for event in decode(node.text.as_deref()?) {
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
