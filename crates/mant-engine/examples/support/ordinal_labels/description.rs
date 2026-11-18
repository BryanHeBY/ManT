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
            // A configuration-only root has no text or child expression.
            // eqn_term.c::eqn_box() also generates delimiters/operators that
            // cannot be inferred from the responsive scalar projection.
            (NodeKind::Equation, _) => {
                return node
                    .equation
                    .as_ref()
                    .map_or(Description::Absent, equation_description);
            }
            (NodeKind::Element, Some("Fn")) if !node.children.is_empty() => {
                // termp_fn_pre() writes both parentheses after the name,
                // even when that first word has no visible glyph.
                return Description::Visible;
            }
            (_, Some("ce" | "rj")) => {
                // roff_term_pre_ce starts at n->child->next. The first TEXT
                // is the count, and is never passed to term_word().
                return self.visit_children(node.children.iter().skip(1));
            }
            (NodeKind::Body, Some("En")) => {
                // termp_quote_pre/post use parser-normalized Es operands,
                // whose presence alone does not prove printable glyphs.
                if let Some(enclosure) = &node.enclosure {
                    let delimiters = NativeWord::read(&enclosure.opening);
                    let closing =
                        NativeWord::read(enclosure.closing.as_deref().unwrap_or_default());
                    let generated = word_description(&delimiters).max(word_description(&closing));
                    if generated != Description::Absent {
                        return generated;
                    }
                }
            }
            // mdoc_term.c :: termp_quote_pre/post write the delimiters even
            // with no BODY children. The macro label itself is not an operand.
            (NodeKind::Element, Some("Ap" | "Fl"))
            | (NodeKind::Body, Some("Fo"))
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
        self.visit_children(node.children.iter())
    }

    fn visit_children<'a>(&mut self, children: impl Iterator<Item = &'a Node>) -> Description {
        let mut result = Description::Absent;
        for child in children {
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
        let result = word_description(&word);
        if result != Description::Absent {
            result
        } else if node.flags.no_fill && (word.has(WordEffect::Buffered) || !word.text.is_empty()) {
            // A buffered zero-width cell or line marker can still own a
            // physical no-fill row. A bare control writes no cell.
            Description::Layout
        } else {
            Description::Absent
        }
    }
}

fn word_description(word: &NativeWord) -> Description {
    if word.has(WordEffect::Unclassified) {
        Description::Uncertain
    } else if word
        .text
        .chars()
        .any(|ch| !ch.is_whitespace() || ch == '\u{a0}')
    {
        Description::Visible
    } else {
        Description::Absent
    }
}

fn equation_description(equation: &libmandoc_rs::EquationBox) -> Description {
    use libmandoc_rs::{EquationKind, EquationPosition};
    let mut result = equation
        .text
        .as_deref()
        .map_or(Description::Absent, |text| {
            word_description(&NativeWord::read(text))
        });
    if equation.kind == EquationKind::List && equation.expected_args > 1 {
        for fence in [
            equation.left.as_deref().unwrap_or("("),
            equation.right.as_deref().unwrap_or(")"),
        ] {
            result = result.max(word_description(&NativeWord::read(fence)));
        }
    }
    if equation.position != EquationPosition::None || equation.bottom.is_some() {
        // Native positional operators and bottom decorations emit a glyph.
        result = Description::Visible;
    }
    if let Some(top) = &equation.top {
        result = result.max(word_description(&NativeWord::read(top)));
    }
    for child in &equation.children {
        if equation.kind == EquationKind::Matrix
            && child.kind == EquationKind::List
            && child.expected_args > 1
        {
            // eqn_box() bypasses the matrix's outer column-list fences.
            for column in &child.children {
                result = result.max(equation_description(column));
            }
        } else if equation.kind == EquationKind::Pile
            && child.kind == EquationKind::List
            && child.expected_args > 1
            && child.actual_args == 1
        {
            // eqn_box() passes only the row's first child in this exact
            // shape. Empty rows with no child still execute their fences.
            if let Some(first) = child.children.first() {
                result = result.max(equation_description(first));
            }
        } else {
            result = result.max(equation_description(child));
        }
    }
    if result == Description::Absent && (equation.text.is_some() || !equation.children.is_empty()) {
        // A populated but glyphless expression still needs an independent
        // source review. Its owner and equation census remain obligatory.
        Description::Uncertain
    } else {
        result
    }
}
