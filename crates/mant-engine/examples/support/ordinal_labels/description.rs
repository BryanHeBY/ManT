//! BODY evidence uses executable source roles and terminal word effects.

use libmandoc_rs::{Node, NodeKind};

use mant_codec::{RoffWordEffect as WordEffect, RoffWordEvidence as NativeWord};

use super::controls::{SourceControl, source_control};

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Glyphs {
    #[default]
    None,
    Equation,
    Text,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Spacing {
    #[default]
    None,
    Boundary,
    Row,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Payload {
    glyphs: Glyphs,
    spacing: Spacing,
    unresolved: bool,
}

impl Payload {
    const ROW: Self = Self {
        spacing: Spacing::Row,
        ..Self::ABSENT
    };
    const GLYPHS: Self = Self {
        glyphs: Glyphs::Text,
        ..Self::ABSENT
    };
    const UNRESOLVED: Self = Self {
        unresolved: true,
        ..Self::ABSENT
    };
    const ABSENT: Self = Self {
        glyphs: Glyphs::None,
        spacing: Spacing::None,
        unresolved: false,
    };

    pub(crate) fn absent(self) -> bool {
        self.glyphs == Glyphs::None && self.spacing == Spacing::None && !self.unresolved
    }

    pub(crate) fn needs_review(self) -> bool {
        self.unresolved && self.glyphs == Glyphs::None && !self.has_row()
    }

    pub(crate) fn has_text(self) -> bool {
        self.glyphs == Glyphs::Text
    }

    pub(crate) fn has_row(self) -> bool {
        self.spacing == Spacing::Row
    }

    fn merge(self, another: Self) -> Self {
        Self {
            glyphs: self.glyphs.max(another.glyphs),
            spacing: self.spacing.max(another.spacing),
            unresolved: self.unresolved || another.unresolved,
        }
    }
}

pub(crate) fn description(node: &Node, continues: bool) -> Payload {
    DescriptionProbe {
        continues,
        zero_advance: false,
        display_row: false,
        space_debt: 0,
    }
    .visit(node)
}

struct DescriptionProbe {
    continues: bool,
    zero_advance: bool,
    display_row: bool,
    space_debt: i32,
}

impl DescriptionProbe {
    fn visit(&mut self, node: &Node) -> Payload {
        if super::skips_source_children(node) {
            return Payload::ABSENT;
        }
        if generated_body_glyph(node) {
            return Payload::GLYPHS;
        }
        if let Some(control) = source_control(node) {
            return match control {
                SourceControl::VerticalSpace => {
                    let argument = node
                        .children
                        .first()
                        .and_then(|child| child.text.as_deref());
                    self.vertical_space(mant_codec::roff_vertical_space_lines(argument))
                }
                SourceControl::AlignedRows => self.visit_children(node.children.iter().skip(1)),
                SourceControl::State
                | SourceControl::SpacingMode
                | SourceControl::LineBoundary
                | SourceControl::Skip => Payload::ABSENT,
            };
        }
        let token = node.macro_token.as_deref();
        match (node.kind, token) {
            (NodeKind::Text, _) => return self.visit_text(node),
            // A configuration-only root has no text or child expression.
            // eqn_term.c::eqn_box() also generates delimiters/operators that
            // cannot be inferred from the responsive scalar projection.
            (NodeKind::Equation, _) => {
                let mut payload = node
                    .equation
                    .as_ref()
                    .map_or(Payload::ABSENT, equation_description);
                // A display EQ owns its glyphs in a separate block.
                if payload.has_text() && !node.equation_inline.unwrap_or(!node.flags.line_start) {
                    payload.glyphs = Glyphs::Equation;
                }
                return payload;
            }
            (NodeKind::Element, Some("Fn")) if !node.children.is_empty() => {
                // termp_fn_pre() writes both parentheses after the name,
                // even when that first word has no visible glyph.
                return Payload::GLYPHS;
            }
            (NodeKind::Body, Some("En")) => {
                // termp_quote_pre/post use parser-normalized Es operands,
                // whose presence alone does not prove printable glyphs.
                if let Some(enclosure) = &node.enclosure {
                    let delimiters = NativeWord::read(&enclosure.opening);
                    let closing =
                        NativeWord::read(enclosure.closing.as_deref().unwrap_or_default());
                    let generated =
                        word_payload(&delimiters, false).merge(word_payload(&closing, false));
                    if !generated.absent() {
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
            ) => return Payload::GLYPHS,
            (NodeKind::Table, _) => return Payload::ROW,
            (NodeKind::Block, Some("Bd" | "D1" | "Dl")) => {
                return self.visit_display(node);
            }
            (NodeKind::Block, Some("Bl")) if node.list_kind.is_some() => {
                // A native list or column owner remains substantive after
                // its children project to no glyphs. termp_bl_pre/post()
                // own the whole structure. An armed \z with no buffer may
                // survive the post and affect the following ordinal.
                let result = self.visit_children(node.children.iter());
                return result.merge(if self.zero_advance {
                    Payload::UNRESOLVED
                } else {
                    Payload::ROW
                });
            }
            _ => {}
        }
        self.visit_children(node.children.iter())
    }

    fn visit_display(&mut self, node: &Node) -> Payload {
        // termp_d1_pre/termp_bl_post and termp_bd_post close the actual
        // buffer. A word can own an empty row when it writes a zero-width
        // cell or marker. A compact display alone writes no blank.
        let previous = std::mem::replace(&mut self.display_row, true);
        let result = self.visit_children(
            node.children
                .iter()
                .filter(|part| part.kind == NodeKind::Body),
        );
        self.display_row = previous;
        if node.macro_token.as_deref() == Some("Bd") && !node.compact {
            // Inside a definition BODY, print_bvspace() can supply a row
            // independently of the display payload.
            result.merge(Payload {
                spacing: Spacing::Boundary,
                ..Payload::ABSENT
            })
        } else {
            result
        }
    }

    fn visit_children<'a>(&mut self, children: impl Iterator<Item = &'a Node>) -> Payload {
        let mut result = Payload::ABSENT;
        for child in children {
            result = result.merge(self.visit(child));
            if result.has_text() {
                break;
            }
        }
        result
    }

    fn visit_text(&mut self, node: &Node) -> Payload {
        let source = node.text.as_deref().unwrap_or_default();
        if source.is_empty() && node.flags.line_start {
            // print_mdoc_node() only calls term_vspace() for empty source
            // TEXT when the preceding word did not set TERMP_NONEWLINE.
            return if self.continues {
                Payload::ABSENT
            } else if self.zero_advance {
                // An unconsumed \z may survive this empty boundary and
                // change the next owner. This case needs source review.
                Payload::UNRESOLVED
            } else {
                self.vertical_space(1)
            };
        }
        // Actual term_word() clears skipvsp even for a nonprinting word.
        self.space_debt = 0;
        let word = NativeWord::read(source);
        self.continues = word.has(WordEffect::Continues);
        self.zero_advance =
            word.has(WordEffect::ZeroAdvance) || self.zero_advance && word.text.is_empty();
        word_payload(&word, node.flags.no_fill || self.display_row)
    }

    fn vertical_space(&mut self, rows: i32) -> Payload {
        // roff_term_pre_sp() accumulates negative requests. term_vspace()
        // consumes the debt before completing blank rows. The explicit
        // request still marks a layout boundary when it produces zero rows.
        let completes_row = rows > self.space_debt;
        self.space_debt = self.space_debt.saturating_sub(rows).max(0);
        if completes_row {
            Payload::ROW
        } else {
            Payload {
                spacing: Spacing::Boundary,
                ..Payload::ABSENT
            }
        }
    }
}

pub(crate) fn word_payload(word: &NativeWord, closes_row: bool) -> Payload {
    if closes_row && word.rejects_leading_pass() {
        return Payload::ROW;
    }
    let unresolved = word.has(WordEffect::Unclassified);
    let glyphs = closes_row && word.has_leading_graph()
        || !unresolved
            && word
                .text
                .chars()
                .any(|ch| !ch.is_whitespace() || ch == '\u{a0}');
    // Without a proved close, buffered controls are just part of the
    // current field. An unclassified edit can undo such writes.
    let row =
        closes_row && !unresolved && (word.has(WordEffect::Buffered) || !word.text.is_empty());
    Payload {
        glyphs: if glyphs { Glyphs::Text } else { Glyphs::None },
        spacing: if row { Spacing::Row } else { Spacing::None },
        unresolved,
    }
}

fn equation_description(equation: &libmandoc_rs::EquationBox) -> Payload {
    equation_context(equation, false)
}

fn equation_context(equation: &libmandoc_rs::EquationBox, has_siblings: bool) -> Payload {
    use libmandoc_rs::{EquationKind, EquationPosition};
    let mut result = equation.text.as_deref().map_or(Payload::ABSENT, |text| {
        word_payload(&NativeWord::read(text), false)
    });
    if equation.kind == EquationKind::List && equation.expected_args > 1
        || equation.kind == EquationKind::Pile && has_siblings
    {
        for fence in [
            equation.left.as_deref().unwrap_or("("),
            equation.right.as_deref().unwrap_or(")"),
        ] {
            result = result.merge(word_payload(&NativeWord::read(fence), false));
        }
    }
    if equation.position != EquationPosition::None || equation.bottom.is_some() {
        // Native positional operators and bottom decorations emit a glyph.
        result.glyphs = Glyphs::Text;
    }
    if let Some(top) = &equation.top {
        result = result.merge(word_payload(&NativeWord::read(top), false));
    }
    for child in &equation.children {
        if equation.kind == EquationKind::Matrix
            && child.kind == EquationKind::List
            && child.expected_args > 1
        {
            // eqn_box() bypasses the matrix's outer column-list fences.
            for column in &child.children {
                result = result.merge(equation_context(column, child.children.len() > 1));
            }
        } else if equation.kind == EquationKind::Pile
            && child.kind == EquationKind::List
            && child.expected_args > 1
            && child.actual_args == 1
        {
            // eqn_box() passes only the row's first child in this exact
            // shape. Empty rows with no child still execute their fences.
            if let Some(first) = child.children.first() {
                result = result.merge(equation_context(first, false));
            }
        } else {
            result = result.merge(equation_context(child, equation.children.len() > 1));
        }
    }
    if result.absent() && (equation.text.is_some() || !equation.children.is_empty()) {
        // A populated but glyphless expression still needs an independent
        // source review. Its owner and equation census remain obligatory.
        Payload::UNRESOLVED
    } else {
        result
    }
}

fn generated_body_glyph(node: &Node) -> bool {
    match (node.kind, node.macro_token.as_deref()) {
        // termp_in_pre/post write both angle delimiters independently of
        // the include operand's printable scalar content.
        (NodeKind::Element, Some("In")) => true,
        // termp_lk_pre writes ':' when a description precedes the suffix
        // punctuation, including a description made only of zero-width cells.
        (NodeKind::Element, Some("Lk")) => node
            .children
            .iter()
            .skip(1)
            .any(|child| !child.flags.delimiter_close && !child.flags.no_print),
        // termp____post writes a period/comma for the executed reference
        // fields under Rs. Merely entering an empty reference prints none.
        (NodeKind::Body, Some("Rs")) => node.children.iter().any(|child| {
            !child.flags.no_print
                && matches!(
                    child.macro_token.as_deref(),
                    Some(
                        "%A" | "%B"
                            | "%C"
                            | "%D"
                            | "%I"
                            | "%J"
                            | "%N"
                            | "%O"
                            | "%P"
                            | "%Q"
                            | "%R"
                            | "%T"
                            | "%U"
                            | "%V"
                    )
                )
        }),
        // termp_it_pre prints a marker at each actual HEAD for these kinds.
        // An empty list has no It and therefore no generated marker.
        (NodeKind::Block, Some("Bl"))
            if matches!(
                node.list_kind,
                Some(
                    libmandoc_rs::NormalizedListKind::Bullet
                        | libmandoc_rs::NormalizedListKind::Dash
                        | libmandoc_rs::NormalizedListKind::Ordered
                )
            ) =>
        {
            node.children
                .iter()
                .filter(|part| part.kind == NodeKind::Body)
                .flat_map(|body| &body.children)
                .any(|item| {
                    item.kind == NodeKind::Block
                        && item.macro_token.as_deref() == Some("It")
                        && !item.flags.no_print
                })
        }
        _ => false,
    }
}
