//! BODY evidence uses executable source roles and terminal word effects.

use libmandoc_rs::{Node, NodeKind};

use mant_codec::{RoffWordEffect as WordEffect, RoffWordEvidence as NativeWord};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Description {
    Absent,
    Uncertain,
    Layout,
    Visible,
}

pub(super) fn description(node: &Node, continues: bool) -> Description {
    DescriptionProbe {
        continues,
        zero_advance: false,
    }
    .visit(node)
}

struct DescriptionProbe {
    continues: bool,
    zero_advance: bool,
}

impl DescriptionProbe {
    fn visit(&mut self, node: &Node) -> Description {
        if node.flags.no_print || node.kind == NodeKind::Comment {
            return Description::Absent;
        }
        let token = node.macro_token.as_deref();
        match (node.kind, token) {
            (NodeKind::Text, _) => return self.visit_text(node),
            // mdoc_term.c :: termp_quote_pre/post write the delimiters even
            // with no BODY children. The macro label itself is not an operand.
            (NodeKind::Equation, _)
            | (NodeKind::Element, Some("Ap" | "Fl"))
            | (
                NodeKind::Body | NodeKind::Element,
                Some(
                    "Ao" | "Aq" | "Bo" | "Bq" | "Bro" | "Brq" | "Do" | "Dq" | "Oo" | "Op" | "Po"
                    | "Pq" | "Ql" | "Qo" | "Qq" | "So" | "Sq",
                ),
            ) => return Description::Visible,
            (NodeKind::Table, _)
            | (NodeKind::Block, Some("Bd"))
            | (NodeKind::Element, Some("sp" | "Pp")) => return Description::Layout,
            // State-only macro operands do not go through term_word().
            (
                _,
                Some(
                    "Sm" | "ft" | "ll" | "po" | "in" | "ti" | "ta" | "nf" | "fi" | "DT" | "PD"
                    | "Tg" | "br" | "Es",
                ),
            ) => return Description::Absent,
            _ => {}
        }
        let mut result = Description::Absent;
        for child in &node.children {
            result = result.max(self.visit(child));
            if result == Description::Visible {
                break;
            }
        }
        result
    }

    fn visit_text(&mut self, node: &Node) -> Description {
        let source = node.text.as_deref().unwrap_or_default();
        if source.is_empty() && node.flags.line_start {
            // print_mdoc_node() only calls term_vspace() for empty source
            // TEXT when the preceding word did not set TERMP_NONEWLINE.
            return if self.continues {
                Description::Absent
            } else if self.zero_advance {
                // An unconsumed \z may survive this empty boundary and
                // change the next owner. This case needs source review.
                Description::Uncertain
            } else {
                Description::Layout
            };
        }
        let word = NativeWord::read(source);
        self.continues = word.has(WordEffect::Continues);
        self.zero_advance =
            word.has(WordEffect::ZeroAdvance) || self.zero_advance && word.text.is_empty();
        if word.has(WordEffect::Unclassified) {
            Description::Uncertain
        } else if word
            .text
            .chars()
            .any(|ch| !ch.is_whitespace() || ch == '\u{a0}')
        {
            Description::Visible
        } else if node.flags.no_fill && (word.has(WordEffect::Buffered) || !word.text.is_empty()) {
            // A buffered zero-width cell or line marker can still own a
            // physical no-fill row. A bare control writes no cell.
            Description::Layout
        } else {
            Description::Absent
        }
    }
}
