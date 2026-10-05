//! Assemble source contributions independently of their navigation spelling.
use super::super::mapped::{BlockSyntax, MappedText};

/// Select framing from the actual adjacent syntax, retaining source spacing.
/// Inline hard rows are content: they do not request another blank source line.
/// `CommonMark` `firstpass::scan_paragraph_interrupt_no_table` permits fences,
/// rules and nonempty bullet/ordinal-one lists to interrupt ordinary phrasing.
pub(super) fn join(values: impl IntoIterator<Item = MappedText>) -> MappedText {
    let mut values = values.into_iter();
    let Some(mut result) = values.next() else {
        return MappedText::default();
    };
    for next in values {
        append(&mut result, next);
    }
    result
}

pub(super) fn append(left: &mut MappedText, mut right: MappedText) {
    let positive = left.contribution.after || right.contribution.before;
    let phrasing = left.tail.grammar == BlockSyntax::Phrasing;
    let list = left.tail.columns > 0;
    let paragraph = positive
        || (phrasing && matches!(right.syntax, BlockSyntax::List { needs_blank: true }))
        || (right.syntax == BlockSyntax::Phrasing
            && !right.hard_rows
            && (list || (phrasing && !left.tail.hard_rows)));
    if positive || !paragraph {
        complete_term_tail(left);
    }
    if paragraph {
        newline(left, 2);
    } else if phrasing && right.hard_rows {
        // A soft source newline before an empty hard-row root would add a
        // space. Join the actual closed row with an ordinary hard delimiter.
        hard_row(left);
        right.tail.columns = left.tail.columns;
    } else if phrasing && right.has_navigation_preamble() {
        // The destination precedes a structural receiver on its own syntax
        // line. Attach it to existing phrasing without a soft separator cell.
    } else {
        newline(left, 1);
    }
    left.append(right);
}

/// Separate phrasing on a real hard row, using an existing open tail once.
/// Owner changes do not add another row after an already emitted hard break.
pub(super) fn hard_row(left: &mut MappedText) {
    complete_term_tail(left);
    if left.tail.open_row {
        newline(left, 1);
    } else {
        left.text.push_str("  \n");
    }
}

/// Sibling markers are already inside the same list grammar. Unlike a root
/// scope exit, their syntax does not supply a completed term's empty row.
pub(super) fn append_item(left: &mut MappedText, right: MappedText, gap: u16) {
    complete_term_tail(left);
    newline(left, usize::from(gap) + 1);
    left.append(right);
}

fn complete_term_tail(value: &mut MappedText) {
    if std::mem::take(&mut value.tail.term_tail) {
        // Definition terms retain their final row where Paragraph would
        // close a provisional tail. A required scope blank already represents
        // that completion; otherwise give the source row its hard delimiter.
        newline(value, 1);
        value.text.push_str(&" ".repeat(value.tail.columns));
        value.text.push_str("<br>");
    }
}

fn newline(left: &mut MappedText, required: usize) {
    let present = left
        .text
        .bytes()
        .rev()
        .take_while(|&byte| byte == b'\n')
        .count();
    left.text
        .extend(std::iter::repeat_n('\n', required.saturating_sub(present)));
}

pub(super) fn coalesce(values: impl IntoIterator<Item = MappedText>) -> Vec<MappedText> {
    let mut rendered = Vec::new();
    let mut pending = MappedText::default();
    for mut block in values {
        if block.contribution.rows {
            block.contribution.before |= pending.contribution.spacing();
            block.attach_navigation_text(std::mem::take(&mut pending), false);
            rendered.push(block);
        } else {
            pending.append(block);
        }
    }
    if let Some(last) = rendered.last_mut() {
        last.contribution.after |= pending.contribution.spacing();
        rendered
            .first_mut()
            .expect("a physical block")
            .attach_navigation_text(pending, true);
    } else if let Some(pending) = pending.nonempty() {
        rendered.push(pending);
    }
    rendered
}

/// Materialize only resolved boundaries at a complete Markdown block root.
/// A destination alone never needs a paragraph; positive spacing does need an
/// explicit hard-row spelling when no neighboring body can retain its gap.
pub(super) fn finish_boundaries(mut blocks: Vec<MappedText>) -> Vec<MappedText> {
    if let Some(first) = blocks.first_mut() {
        if !first.contribution.rows && first.contribution.spacing() {
            first.text.push_str("<br />");
        } else if first.contribution.before {
            let row = if matches!(first.syntax, BlockSyntax::List { needs_blank: true }) {
                "&#10;\n\n"
            } else {
                // The entity is the requested newline itself, not a sentinel
                // or styled glyph. Inline grammar admits raw anchor navigation
                // and block interruption without an HTML-block delimiter.
                "&#10;\n"
            };
            first.insert(0, row);
        }
        first.contribution.before = false;
        if !first.contribution.rows {
            first.contribution.after = false;
        }
    }
    if let Some(last) = blocks.last_mut()
        && last.contribution.rows
        && last.contribution.after
    {
        complete_term_tail(last);
        let padding = " ".repeat(last.tail.columns);
        if last.tail.grammar == BlockSyntax::Phrasing {
            if !last.tail.open_row {
                last.text.push_str("<br>\n");
                last.text.push_str(&padding);
            }
            last.text.push_str("<br>");
        } else {
            last.text.push('\n');
            last.text.push_str(&padding);
            last.text.push_str("<br />");
        }
        last.contribution.after = false;
    }
    blocks
}

pub(super) fn list_body(blocks: Vec<MappedText>) -> Vec<MappedText> {
    let mut blocks = blocks;
    if let Some(first) = blocks.first_mut()
        && first.contribution.rows
        && first.contribution.before
    {
        first.contribution.before = false;
        let marker = if matches!(first.syntax, BlockSyntax::List { needs_blank: true }) {
            // A non-1 marker needs a blank source line after phrasing. That
            // syntax separator supplies the requested gap after the marker
            // row, instead of adding it to another explicit hard blank.
            "&#10;"
        } else {
            // The generated marker occupies the first row. The second hard
            // row is the requested blank, not another source paragraph gap.
            "&#10;\n<br>"
        };
        blocks.insert(
            0,
            MappedText::from(marker.to_owned())
                .syntax_site(BlockSyntax::Phrasing)
                .tail_grammar(BlockSyntax::Phrasing, true)
                .hard_rows(true),
        );
    }
    blocks
}
