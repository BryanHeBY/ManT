//! Assemble source contributions independently of their navigation spelling.
use super::super::mapped::{BlockSyntax, MappedText};

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
        blocks.insert(
            0,
            MappedText::from("<br />".to_owned()).syntax_site(BlockSyntax::Phrasing),
        );
    }
    blocks
}
