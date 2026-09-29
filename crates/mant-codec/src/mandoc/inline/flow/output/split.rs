use super::super::Inline;
/// Split a formatter word at the row boundaries its field passes decided
/// (term.c:220 with 205-207): each boundary is a cell offset inside the
/// word's content; the breakable blanks immediately before it were
/// consumed by the break, and the continuation starts at the boundary.
pub(super) fn split_word_at_row_boundaries(
    incoming: &[Inline],
    boundaries: &mut Vec<usize>,
) -> Vec<Inline> {
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut output = Vec::with_capacity(incoming.len() + boundaries.len());
    let mut next_boundary = boundaries.first().copied();
    split_nodes_at_boundaries(
        incoming,
        &mut 0,
        &mut next_boundary,
        boundaries,
        &mut output,
    );
    output
}

/// Returns true when the whole remainder was emitted (a boundary fell in a
/// non-cell node, so no further split can apply).
fn split_nodes_at_boundaries(
    nodes: &[Inline],
    cell: &mut usize,
    next_boundary: &mut Option<usize>,
    boundaries: &[usize],
    output: &mut Vec<Inline>,
) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                split_text_at_boundaries(value, node, cell, next_boundary, boundaries, output);
            }
            Inline::LineBreak { .. } => {
                output.push(Inline::line_break());
                *cell += 1;
                advance_boundary(cell, next_boundary, boundaries);
            }
            Inline::Strong { children } => {
                output.push(Inline::Strong {
                    children: std::mem::take(&mut split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    )),
                });
            }
            Inline::Emphasis { children } => {
                output.push(Inline::Emphasis {
                    children: std::mem::take(&mut split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    )),
                });
            }
            other => output.push(other.clone()),
        }
    }
}

fn split_nodes_at_boundaries_owned(
    nodes: &[Inline],
    cell: &mut usize,
    next_boundary: &mut Option<usize>,
    boundaries: &[usize],
) -> Vec<Inline> {
    let mut output = Vec::with_capacity(nodes.len());
    split_nodes_at_boundaries(nodes, cell, next_boundary, boundaries, &mut output);
    output
}

fn split_text_at_boundaries(
    value: &str,
    node: &Inline,
    cell: &mut usize,
    next_boundary: &mut Option<usize>,
    boundaries: &[usize],
    output: &mut Vec<Inline>,
) {
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;
    let mut run = String::with_capacity(value.len());
    while index < chars.len() {
        // A row boundary before this graph — the zero-width breakpoint's
        // cell (term.c:287-300; the pass truncated at it, 362-366) or a
        // word-tail candidate an overrun cut at (353-354): the row ends
        // here and the graph starts the next one. The breakpoint itself
        // never prints (term.c:396-398).
        if *next_boundary == Some(*cell) {
            push_split_text(&mut run, node, output);
            output.push(Inline::line_break());
            advance_boundary(cell, next_boundary, boundaries);
        }
        let character = chars[index];
        if character == '\n' {
            run.push(character);
            push_split_text(&mut run, node, output);
            *cell += 1;
            index += 1;
            advance_boundary(cell, next_boundary, boundaries);
            continue;
        }
        if super::super::super::is_formatter_word_blank(character) {
            // Measure the whole blank run. When its end lands on the next
            // row boundary, the run is the break's consumed separator
            // (term.c:205-207): the accepted row ends here and the word
            // continues on the next device row.
            let mut end = index;
            while end < chars.len() && super::super::super::is_formatter_word_blank(chars[end]) {
                end += 1;
            }
            if *next_boundary == Some(*cell + (end - index)) {
                while run
                    .chars()
                    .last()
                    .is_some_and(super::super::super::is_formatter_word_blank)
                {
                    run.pop();
                }
                push_split_text(&mut run, node, output);
                output.push(Inline::line_break());
                *cell += end - index;
                index = end;
                advance_boundary(cell, next_boundary, boundaries);
                continue;
            }
        }
        run.push(character);
        *cell += 1;
        index += 1;
    }
    push_split_text(&mut run, node, output);
}

fn advance_boundary(cell: &mut usize, next_boundary: &mut Option<usize>, boundaries: &[usize]) {
    if *next_boundary == Some(*cell) {
        *next_boundary = boundaries
            .iter()
            .copied()
            .find(|&boundary| boundary > *cell);
    }
}

fn push_split_text(run: &mut String, node: &Inline, output: &mut Vec<Inline>) {
    if run.is_empty() {
        return;
    }
    let text = std::mem::take(run);
    match node {
        Inline::Code { .. } => output.push(Inline::Code { value: text }),
        _ => output.push(Inline::Text { value: text }),
    }
}
