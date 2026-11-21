//! Local row witnesses for source-proved display fields in a definition HEAD.

use super::{
    DisplayRow, Node, NodeKind, Origin, RoffWordEffect, RoffWordEvidence, SourceContext,
    TermPayload, source_evidence,
};
use libmandoc_rs::{DefinitionListStyle, DisplayKind, TableColumnMetrics};

#[derive(Clone, Copy, Default)]
pub(super) enum Field {
    #[default]
    Unknown,
    Ordinary,
    Tagged(usize),
    Hanging,
}

impl Field {
    pub(super) fn for_item(list: Option<&Node>, item: &Node) -> Self {
        let Some(list) = list else {
            return Self::Unknown;
        };
        match list.definition_list_style {
            Some(DefinitionListStyle::Tag) => {
                // termp_it_pre() sets HANG for a tag without a BODY.
                if !item
                    .children
                    .iter()
                    .any(|part| part.kind == NodeKind::Body && !part.children.is_empty())
                {
                    return Self::Hanging;
                }
                match list.width.as_deref() {
                    None => Self::Tagged(8),
                    Some(width) => simple_width(width)
                        .map_or(Self::Unknown, |width| Self::Tagged(width.saturating_add(2))),
                }
            }
            Some(DefinitionListStyle::Hang) => Self::Hanging,
            Some(DefinitionListStyle::Inset | DefinitionListStyle::Overhang) => Self::Ordinary,
            _ => Self::Unknown,
        }
    }

    fn closes(self, columns: usize, advance: usize, origin: usize) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Hanging => Some(false),
            Self::Ordinary => Some(true),
            Self::Tagged(width) => {
                // term_flushln() tests the remaining margin with the tag's
                // two trailing spaces. Integer cell widths leave the half
                // cell tolerance between, rather than on, the boundaries.
                let gap = origin
                    .saturating_sub(columns)
                    .max(if columns == 0 { 0 } else { 2 });
                Some(
                    columns
                        .saturating_add(gap)
                        .saturating_add(advance)
                        .saturating_add(2)
                        > width,
                )
            }
        }
    }
}

fn simple_width(width: &str) -> Option<usize> {
    if let Some((units, "")) = TableColumnMetrics::motion_argument(width)
        && units >= 0
        && units % 24 == 0
    {
        return usize::try_from(units / 24).ok();
    }
    width
        .bytes()
        .all(|ch| ch.is_ascii_alphabetic())
        .then_some(width.len())
}

pub(super) fn record_short(node: &Node, source: &SourceContext, payload: &mut TermPayload) {
    let mut rows = payload
        .short_rows
        .take()
        .unwrap_or_else(|| Rows::begin(payload));
    // termp_d1_pre calls term_newln at the enclosing origin before
    // setting the display's offset. Even an empty input field may end a
    // row that the previous tag field left open with minbl = trailspace.
    rows.flush_empty_field(payload.field, 0);
    // termp_d1_pre adds defindent + 1 to offset for this short display.
    // A field longer than the entire tag width must close regardless of
    // previously printed columns. A fitting field may share its row.
    let offset = if matches!(payload.field, Field::Tagged(_)) {
        6
    } else {
        0
    };
    rows.record(&node.children, source, payload, offset);
    payload.short_rows = Some(rows);
}

pub(super) fn record(
    display: &Node,
    body: &Node,
    source: &SourceContext,
    payload: &mut TermPayload,
) -> bool {
    if !matches!(
        display.display_kind,
        Some(DisplayKind::Literal | DisplayKind::Unfilled)
    ) {
        return false;
    }
    if !matches!(display.offset.as_deref(), None | Some("left")) {
        payload.partial = true;
        return false;
    }
    let mut rows = Rows::begin(payload);
    let mut start = 0;
    let mut continues = false;
    let mut hang_without_gap = false;
    for (index, node) in body.children.iter().enumerate() {
        let empty_text = node.kind == NodeKind::Text
            && node.text.as_deref() == Some("")
            && node.flags.line_start;
        let space_started = node.kind == NodeKind::Text
            && node.flags.line_start
            && node
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with(' '));
        let state_only = matches!(node.macro_token.as_deref(), Some("ft" | "ta" | "Sm" | "Tg"));
        if !simple_node(node) && !state_only {
            rows.record_literal(
                &body.children[start..index],
                source,
                payload,
                hang_without_gap,
            );
            // roff_term_pre_br(), including the requests that call it,
            // flushes before clearing the tag's NOBREAK/BRIND field.
            let clears_brind = matches!(
                node.macro_token.as_deref(),
                Some("br" | "sp" | "ce" | "rj" | "fi" | "nf" | "ti")
            );
            let hanging_break = clears_brind && matches!(payload.field, Field::Hanging);
            hang_without_gap |= hanging_break;
            super::HeadPayloads::walk_part_slice(
                std::slice::from_ref(node),
                source,
                Origin::Display,
                payload,
            );
            payload.partial = true;
            if clears_brind && matches!(payload.field, Field::Tagged(_)) {
                // roff_term_pre_br() clears NOBREAK and BRIND after
                // flushing. Later literal fields can now close their rows.
                payload.field = Field::Ordinary;
            }
            let previous = rows.last_literal_atom;
            rows.forget(payload);
            if hanging_break {
                rows.last_literal_atom = previous;
            }
            start = index + 1;
            continues = false;
            continue;
        }
        if node.flags.no_fill && node.flags.line_start && (!continues || empty_text) {
            if start == index {
                // A state-only source row may leave viscol without any
                // input cells. term_newln() still calls term_flushln()
                // in that state (term.c:474–481).
                rows.flush_empty_field(payload.field, 0);
            } else {
                rows.record_literal(
                    &body.children[start..index],
                    source,
                    payload,
                    hang_without_gap,
                );
            }
            start = index;
        }
        if node.flags.no_fill && !continues && (empty_text || space_started) {
            // print_mdoc_node() can flush a second time for an empty or
            // space-started TEXT after the NODE_NOFILL line-start flush.
            rows.flush_empty_field(payload.field, 0);
        }
        if empty_text {
            // print_mdoc_node() uses term_vspace() for empty TEXT, except
            // after NONEWLINE, in which case it only flushes the field.
            if !continues {
                // NODE_LINE flushed the preceding input above. The empty
                // TEXT's term_vspace calls term_newln again before emitting
                // an independent vertical row (term.c:474–496).
                rows.end_line();
            }
            if !continues {
                rows.last_literal_atom = None;
            }
            start = index + 1;
            // The empty TEXT's term_newln() does not consume NONEWLINE.
            // It is still active at the next actual term_word().
            continue;
        }
        if state_only {
            // ft/ta and the mdoc state handlers do not call term_word().
            // The node entry already closed the preceding field above.
            // An otherwise empty run of requests has no word to submit;
            // the next payload reads their updated SourceContext.
            if start == index {
                start += 1;
            }
            continue;
        }
        continues = node
            .children
            .last()
            .unwrap_or(node)
            .text
            .as_deref()
            .is_some_and(|text| RoffWordEvidence::read(text).has(RoffWordEffect::Continues));
    }
    rows.record_literal(&body.children[start..], source, payload, hang_without_gap);
    true
}

fn simple_node(node: &Node) -> bool {
    node.kind == NodeKind::Text
        || node.kind == NodeKind::Element
            && matches!(node.macro_token.as_deref(), Some("No" | "Em" | "Sy" | "Li"))
            && node
                .children
                .iter()
                .all(|child| child.kind == NodeKind::Text)
}

pub(super) struct Rows {
    row: DisplayRow,
    columns: Option<usize>,
    last_literal_atom: Option<usize>,
}

impl Rows {
    fn record_literal(
        &mut self,
        nodes: &[Node],
        source: &SourceContext,
        payload: &mut TermPayload,
        hang_without_gap: bool,
    ) {
        let previous = self.last_literal_atom;
        let index = payload.atoms.len();
        self.record(nodes, source, payload, 0);
        if payload.atoms.len() > index {
            if hang_without_gap
                && previous == index.checked_sub(1)
                && let Some(previous) = previous
                && payload.atoms[previous].row.is_some_and(|row| !row.ended)
            {
                // roff_term_pre_br clears BRIND and trailspace, but HANG
                // survives. The two immediately neighboring literal fields
                // can join even when their exact starting column is unproved.
                payload.joins.insert(previous, "");
            }
            self.last_literal_atom = payload.atoms.len().checked_sub(1);
        }
    }

    fn begin(payload: &mut TermPayload) -> Self {
        payload.displays += 1;
        Self {
            row: DisplayRow {
                display: payload.displays,
                within: 0,
                ended: false,
            },
            columns: if payload
                .atoms
                .last()
                .is_none_or(|atom| atom.row.is_some_and(|row| row.ended))
            {
                Some(0)
            } else {
                None
            },
            last_literal_atom: None,
        }
    }

    fn forget(&mut self, payload: &mut TermPayload) {
        *self = Self::begin(payload);
        self.columns = None;
    }

    fn end_line(&mut self) {
        self.columns = Some(0);
        self.row.within += 1;
    }

    fn flush_empty_field(&mut self, field: Field, origin: usize) {
        if self
            .columns
            .is_some_and(|columns| columns > 0 && field.closes(columns, 0, origin) == Some(true))
        {
            self.end_line();
        }
    }

    fn record(
        &mut self,
        nodes: &[Node],
        source: &SourceContext,
        payload: &mut TermPayload,
        origin: usize,
    ) {
        let Some(first) = nodes.first() else { return };
        let (closed, word) = source_evidence(nodes, first, false, source, true);
        if closed.has_text() || closed.needs_review() {
            payload.leave_unproved_words();
            self.forget(payload);
        }
        let Some(word) = word else { return };
        // Width is only a proof for ordinary ASCII cells. In particular,
        // NBRZW is written to the buffer but prints no column.
        if !word
            .text
            .bytes()
            .all(|ch| ch.is_ascii_graphic() || ch == b' ')
            || word.has(RoffWordEffect::Unclassified)
        {
            payload.record(Origin::Display, &word, None);
            payload.partial = true;
            self.forget(payload);
            return;
        }
        let advance = word.text.len();
        if advance == 0 && !word.has(RoffWordEffect::Buffered) {
            return;
        }
        let ended = self
            .columns
            .and_then(|columns| payload.field.closes(columns, advance, origin));
        // A long field that would overflow even at a fresh start always
        // closes. This permits resuming an exact witness after an opaque
        // neighboring request without asserting its own row position.
        let guaranteed_close = payload.field.closes(0, advance, origin) == Some(true);
        self.row.ended = ended == Some(true) || guaranteed_close;
        payload.record(Origin::Display, &word, Some(self.row));
        if self.row.ended {
            self.end_line();
        } else if ended == Some(false) {
            if advance != 0
                && let Some(column) = self.columns.as_mut()
            {
                let gap = if *column == 0 {
                    origin
                } else {
                    2.max(origin.saturating_sub(*column))
                };
                *column += gap + advance;
            }
        } else {
            payload.partial = true;
            self.forget(payload);
        }
    }
}
