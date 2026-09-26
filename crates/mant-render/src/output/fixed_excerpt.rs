//! Display-only rendering of copied native Fixed structural excerpts.

use mant_protocol::FixedExcerptSelection;

use crate::presentation::{InlinePresentation, TextPresentation, TextRole};

/// Render the verified native rows and columns without consulting logical
/// search joins, inferred Flow blocks, or a second formatter.
pub(super) fn render_fixed_selection(
    view: &FixedExcerptSelection,
    decorate: &dyn Fn(TextPresentation, &str) -> String,
) -> String {
    if view.validate().is_err() {
        return String::new();
    }
    let mut output = String::new();
    let mut row = view.parts.first().map_or(0, |part| part.row.get());
    let mut column = 0;
    for part in &view.parts {
        while row < part.row.get() {
            output.push('\n');
            row += 1;
            column = 0;
        }
        output.extend(std::iter::repeat_n(' ', (part.column - column) as usize));
        output.push_str(&decorate(
            TextPresentation {
                role: TextRole::Body,
                inline: InlinePresentation {
                    strong: part.style.bold,
                    emphasis: part.style.underline,
                    ..InlinePresentation::default()
                },
                matched: false,
            },
            &part.text,
        ));
        column = part.column + part.width;
    }
    output
}
