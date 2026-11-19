//! BODY evidence uses executable source roles and terminal word effects.

use libmandoc_rs::{Node, NodeKind, NormalizedListKind};

use mant_codec::{RoffWordEffect as WordEffect, RoffWordEvidence as NativeWord};

use super::{
    FontEvidence, SourceContext,
    controls::{SourceControl, source_control},
};

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Glyphs {
    #[default]
    None,
    OwnedBlock,
    Text,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Spacing {
    #[default]
    None,
    Boundary,
    Row,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Uncertainty {
    #[default]
    None,
    OwnedBlock,
    Field,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Payload {
    glyphs: Glyphs,
    spacing: Spacing,
    uncertainty: Uncertainty,
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
        uncertainty: Uncertainty::Field,
        ..Self::ABSENT
    };
    const ABSENT: Self = Self {
        glyphs: Glyphs::None,
        spacing: Spacing::None,
        uncertainty: Uncertainty::None,
    };

    pub(crate) fn absent(self) -> bool {
        self.glyphs == Glyphs::None
            && self.spacing == Spacing::None
            && self.uncertainty == Uncertainty::None
    }

    pub(crate) fn needs_review(self) -> bool {
        (self.unproved_literal() || self.uncertainty == Uncertainty::OwnedBlock)
            && self.glyphs == Glyphs::None
            && !self.has_row()
    }

    pub(crate) fn has_text(self) -> bool {
        self.glyphs == Glyphs::Text
    }

    pub(crate) fn has_row(self) -> bool {
        self.spacing == Spacing::Row
    }

    pub(crate) fn unproved_literal(self) -> bool {
        self.uncertainty == Uncertainty::Field && !self.has_text()
    }

    fn merge(self, another: Self) -> Self {
        Self {
            glyphs: self.glyphs.max(another.glyphs),
            spacing: self.spacing.max(another.spacing),
            uncertainty: self.uncertainty.max(another.uncertainty),
        }
    }

    fn owned_block(mut self) -> Self {
        if self.has_text() {
            self.glyphs = Glyphs::OwnedBlock;
        }
        if self.uncertainty == Uncertainty::Field {
            self.uncertainty = Uncertainty::OwnedBlock;
        }
        self
    }
}

pub(crate) fn description(node: &Node, continues: bool, context: &SourceContext) -> Payload {
    DescriptionProbe {
        continues,
        zero_advance: false,
        display: None,
        no_space_mode: context.no_space_at(node),
        font: context.font,
        space_debt: 0,
    }
    .visit(node)
}

struct DescriptionProbe {
    continues: bool,
    zero_advance: bool,
    display: Option<DisplayField>,
    no_space_mode: bool,
    font: FontEvidence,
    space_debt: i32,
}

#[derive(Default)]
struct DisplayField {
    word: NativeWord,
    saw_word: bool,
    join_next: bool,
    font_changed: bool,
}

impl DescriptionProbe {
    fn visit(&mut self, node: &Node) -> Payload {
        // print_mdoc_node() handles the input line before its macro,
        // including ignored operands. A continuation keeps it live.
        let prefix = if node.flags.no_fill && node.flags.line_start && !self.continues {
            self.close_field()
        } else {
            Payload::ABSENT
        };
        prefix.merge(self.visit_node(node))
    }

    fn visit_node(&mut self, node: &Node) -> Payload {
        if super::skips_source_children(node) {
            return Payload::ABSENT;
        }
        if generated_body_glyph(node) {
            return Payload::GLYPHS;
        }
        if let Some(control) = source_control(node) {
            return self.control(node, control);
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
                if !node.equation_inline.unwrap_or(!node.flags.line_start) {
                    payload = payload.owned_block();
                }
                return self.close_field().merge(payload);
            }
            (NodeKind::Element, Some("Fn")) if !node.children.is_empty() => {
                // termp_fn_pre() writes both parentheses after the name,
                // even when that first word has no visible glyph.
                return Payload::GLYPHS;
            }
            (NodeKind::Body, Some("En")) => return self.enclosure(node),
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
            (NodeKind::Table, _) => {
                // tbl_term.c::term_tbl() clears NONOSPACE when it finishes.
                self.no_space_mode = false;
                return self.close_field().merge(Payload::ROW);
            }
            (NodeKind::Block, Some("Bd" | "D1" | "Dl")) => {
                return self.visit_display(node);
            }
            (NodeKind::Block, Some("Bl")) if node.list_kind.is_some() => return self.list(node),
            (_, Some("Ns")) if !node.flags.line_start => {
                if let Some(field) = &mut self.display {
                    field.join_next = true;
                }
            }
            (NodeKind::Element, Some(mac)) if !matches!(mac, "No" | "Ns") => {
                if let Some(field) = &mut self.display {
                    field.word.leave_plain_slots();
                }
            }
            _ => {}
        }
        self.visit_children(node.children.iter())
    }

    fn control(&mut self, node: &Node, control: SourceControl) -> Payload {
        match control {
            SourceControl::VerticalSpace => {
                let argument = node
                    .children
                    .first()
                    .and_then(|child| child.text.as_deref());
                self.close_field()
                    .merge(self.vertical_space(mant_codec::roff_vertical_space_lines(argument)))
            }
            SourceControl::AlignedRows => {
                // roff_term_pre_ce() closes the existing line, skips the
                // first count child, and then flushes its generated rows.
                self.close_field()
                    .merge(self.visit_children(node.children.iter().skip(1)))
                    .merge(self.close_field())
            }
            SourceControl::SpacingMode => {
                self.no_space_mode = super::spacing_mode(node, self.no_space_mode);
                if !self.no_space_mode
                    && let Some(field) = &mut self.display
                {
                    field.join_next = false;
                }
                Payload::ABSENT
            }
            SourceControl::LineBoundary | SourceControl::Margin => self.close_field(),
            SourceControl::State | SourceControl::Skip => {
                if node.macro_token.as_deref() == Some("ft")
                    && let Some(field) = &mut self.display
                {
                    field.font_changed = true;
                    field.word.leave_plain_slots();
                }
                Payload::ABSENT
            }
        }
    }

    fn enclosure(&mut self, node: &Node) -> Payload {
        // termp_quote_pre/post execute normalized Es words around the
        // ordinary children. A glyphless delimiter cannot stand in for BODY.
        let Some(enclosure) = &node.enclosure else {
            return self.visit_children(node.children.iter());
        };
        let opening = self.word(&enclosure.opening, false, true, false);
        let children = self.visit_children(node.children.iter());
        let closing = if let Some(operand) = &enclosure.closing {
            self.word(operand, true, false, false)
        } else {
            if let Some(field) = &mut self.display {
                field.join_next = false;
            }
            Payload::ABSENT
        };
        opening.merge(children).merge(closing)
    }

    fn list(&mut self, node: &Node) -> Payload {
        let before = self.close_field();
        let mut result = Payload::ABSENT;
        if self.display.is_none() {
            result = self.visit_children(node.children.iter());
        } else if node.list_kind != Some(NormalizedListKind::Column) {
            // Bl/It close their native fields independently. Column
            // cells and definition HEADs retain their own content roles.
            // Ordinary item BODY prose still inherits the display mode.
            for item in node
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Body)
                .flat_map(|part| &part.children)
                .filter(|item| {
                    item.kind == NodeKind::Block && item.macro_token.as_deref() == Some("It")
                })
            {
                result = result
                    .merge(
                        self.visit_children(
                            item.children
                                .iter()
                                .filter(|part| part.kind == NodeKind::Body),
                        ),
                    )
                    .merge(self.close_field());
            }
        }
        // A nested list can own substantive layout even with no glyphs
        // in this display. Without any active cell a bare \z may survive.
        before.merge(result).merge(if self.zero_advance {
            Payload::UNRESOLVED
        } else {
            Payload::ROW
        })
    }

    fn close_field(&mut self) -> Payload {
        let Some(field) = &mut self.display else {
            return Payload::ABSENT;
        };
        let word = std::mem::take(&mut field.word);
        let result = word_payload(&word, true);
        if field.font_changed {
            field.word.leave_plain_slots();
        }
        field.saw_word = false;
        field.join_next = false;
        if word.has(WordEffect::Buffered) || !word.text.is_empty() {
            self.zero_advance = false;
        }
        result
    }

    fn visit_display(&mut self, node: &Node) -> Payload {
        // termp_d1_pre/termp_bl_post and termp_bd_post close the actual
        // buffer. A word can own an empty row when it writes a zero-width
        // cell or marker. A compact display alone writes no blank.
        let before = self.close_field();
        let mut field = DisplayField::default();
        if self.font == FontEvidence::Unproved {
            field.font_changed = true;
            field.word.leave_plain_slots();
        }
        let previous = self.display.replace(field);
        let mut result = self.visit_children(
            node.children
                .iter()
                .filter(|part| part.kind == NodeKind::Body),
        );
        result = result.merge(self.close_field());
        self.display = previous;
        if self.display.is_some() {
            // The nested display has its own literal obligation.
            result = result.owned_block();
        }
        if node.macro_token.as_deref() == Some("Bd") && !node.compact {
            // Inside a definition BODY, print_bvspace() can supply a row
            // independently of the display payload.
            result = result.merge(Payload {
                spacing: Spacing::Boundary,
                ..Payload::ABSENT
            });
        }
        before.merge(result)
    }

    fn visit_children<'a>(&mut self, children: impl Iterator<Item = &'a Node>) -> Payload {
        let mut result = Payload::ABSENT;
        for child in children {
            result = result.merge(self.visit(child));
            if self.display.is_none() && result.has_text() {
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
                self.close_field().merge(self.vertical_space(1))
            };
        }
        self.word(
            source,
            node.flags.delimiter_close,
            node.flags.delimiter_open,
            node.flags.no_fill,
        )
    }

    fn word(&mut self, source: &str, closes: bool, opens: bool, no_fill: bool) -> Payload {
        // Actual term_word() clears skipvsp even for a nonprinting word.
        self.space_debt = 0;
        if let Some(field) = &mut self.display {
            if field.saw_word && !field.join_next && !closes && !self.no_space_mode {
                field.word.extend(" ");
            }
            field.word.extend(source);
            self.continues = field.word.has(WordEffect::Continues);
            self.zero_advance = field.word.has(WordEffect::ZeroAdvance);
            field.saw_word = true;
            field.join_next = opens || self.continues;
            return Payload::ABSENT;
        }
        let word = NativeWord::read(source);
        self.continues = word.has(WordEffect::Continues);
        self.zero_advance =
            word.has(WordEffect::ZeroAdvance) || self.zero_advance && word.text.is_empty();
        word_payload(&word, no_fill)
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
    let unresolved = if closes_row {
        word.has_unproved_graphs()
    } else {
        word.has(WordEffect::Unclassified)
    };
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
        uncertainty: if unresolved {
            Uncertainty::Field
        } else {
            Uncertainty::None
        },
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
        _ => false,
    }
}
