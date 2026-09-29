use super::super::Inline;
/// Feed a word's cells, inserting `ASCII_BREAK` cells at the graph counts
/// where `\:` executed (term.c:287-300). The graph count is word-relative
/// and accumulates across nested styling; the breakpoint list is sorted.
pub(super) fn feed_field_inline_with_breakpoints(
    buffer: &mut super::super::field_buffer::FieldBuffer,
    nodes: &[Inline],
    graph_count: &mut usize,
    next_breakpoint: &mut usize,
    breakpoints: &[usize],
) {
    for node in nodes {
        match node {
            Inline::LineBreak { .. } => buffer.push_break_marker(),
            Inline::Text { value } | Inline::Code { value } => {
                for character in value.chars() {
                    if character == '\n' {
                        buffer.push_break_marker();
                    } else if super::super::super::is_formatter_word_blank(character) {
                        buffer.push_separator_blank();
                    } else {
                        if *next_breakpoint < breakpoints.len()
                            && breakpoints[*next_breakpoint] == *graph_count
                        {
                            buffer.push_breakpoint();
                            *next_breakpoint += 1;
                        }
                        let width = mant_ir::geometry::text_width(&character.to_string());
                        buffer.push_graph(character, width);
                        *graph_count += 1;
                    }
                }
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                feed_field_inline_with_breakpoints(
                    buffer,
                    children,
                    graph_count,
                    next_breakpoint,
                    breakpoints,
                );
            }
            // Links own navigation metadata: an empty-label link keeps the
            // URI as an IR fallback that never reaches term_word(), so its
            // cells must not enter the native field buffer.
            _ => {}
        }
    }
}
