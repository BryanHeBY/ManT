use super::super::Inline;

mod accepted_owners;
pub(super) mod painted_order;
pub(in crate::mandoc::inline::flow) use accepted_owners::{
    remove_covered_native_glyphs, retain_native_field_owners,
};

// A word-time pass has already ended this row. Its delimiter is a fact
// about native acceptance even if the adjacent source owner lost all ink.
const ACCEPTED_NATIVE_ENDLINE: &str = "\0mant:output-scope:accepted-native-endline";

pub(super) fn push_accepted_native_endline(nodes: &mut Vec<Inline>, origin: u16) {
    if origin != 0 {
        super::row_origins::set_next_origin(nodes, origin);
    }
    nodes.push(Inline::anchor(ACCEPTED_NATIVE_ENDLINE));
    nodes.push(Inline::line_break());
}

// A section transition already represents the device row end in public
// output. Its private event still consumes the same separator as a break.
const SECTION_END: &str = "\0mant:output-scope:section-row-end";

#[derive(Clone, Copy)]
pub(super) struct PassBoundary {
    pub(super) scalar: usize,
    pub(super) section_seam: bool,
}

impl PassBoundary {
    pub(super) const fn hard(scalar: usize) -> Self {
        Self {
            scalar,
            section_seam: false,
        }
    }

    pub(super) fn projection(self) -> Inline {
        if self.section_seam {
            Inline::anchor(SECTION_END)
        } else {
            Inline::line_break()
        }
    }

    pub(super) fn is_section_end(node: &Inline) -> bool {
        matches!(node, Inline::Anchor { id, .. } if id.as_str() == SECTION_END)
    }
}

/// Split a formatter word at the row boundaries its field passes decided
/// (term.c:220 with 205-207): each boundary is a cell offset inside the
/// word's content; the breakable blanks immediately before it were
/// consumed by the break, and the continuation starts at the boundary.
#[cfg(test)]
pub(super) fn split_word_at_row_boundaries(
    incoming: &[Inline],
    boundaries: &mut Vec<usize>,
) -> Vec<Inline> {
    boundaries.sort_unstable();
    boundaries.dedup();
    let boundaries: Vec<_> = boundaries.iter().copied().map(PassBoundary::hard).collect();
    let mut output = Vec::with_capacity(incoming.len() + boundaries.len());
    let mut next_boundary = 0;
    split_nodes_at_boundaries(
        incoming,
        &mut 0,
        &mut next_boundary,
        &boundaries,
        &mut output,
    );
    output
}

/// Returns true when the whole remainder was emitted (a boundary fell in a
/// non-cell node, so no further split can apply).
#[cfg(test)]
fn split_nodes_at_boundaries(
    nodes: &[Inline],
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[PassBoundary],
    output: &mut Vec<Inline>,
) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                split_text_at_boundaries(value, node, cell, next_boundary, boundaries, output);
            }
            Inline::LineBreak { .. } => {
                output.push(node.clone());
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
            Inline::Link {
                target,
                title,
                children,
            } => {
                output.push(Inline::Link {
                    target: target.clone(),
                    title: title.clone(),
                    children: split_nodes_at_boundaries_owned(
                        children,
                        cell,
                        next_boundary,
                        boundaries,
                    ),
                });
            }
            other => output.push(other.clone()),
        }
    }
}

#[cfg(test)]
fn split_nodes_at_boundaries_owned(
    nodes: &[Inline],
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[PassBoundary],
) -> Vec<Inline> {
    let mut output = Vec::with_capacity(nodes.len());
    split_nodes_at_boundaries(nodes, cell, next_boundary, boundaries, &mut output);
    output
}

pub(super) fn split_text_at_boundaries(
    value: &str,
    node: &Inline,
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[PassBoundary],
    output: &mut Vec<Inline>,
) {
    split_text_with_blank_run(
        value,
        node,
        cell,
        next_boundary,
        boundaries,
        output,
        &mut BlankRun::default(),
    );
}

/// A blank run belongs to the current operand, not to each scalar visited
/// while projecting it. Keep its end until the cursor leaves that run.
#[derive(Default)]
struct BlankRun {
    end: usize,
    #[cfg(test)]
    inspected_cells: usize,
}

impl BlankRun {
    fn end_from(&mut self, chars: &[char], start: usize) -> usize {
        if start >= self.end {
            self.end = start;
            while self.end < chars.len()
                && super::super::super::is_formatter_word_blank(chars[self.end])
            {
                #[cfg(test)]
                {
                    self.inspected_cells += 1;
                }
                self.end += 1;
            }
        }
        self.end
    }
}

fn split_text_with_blank_run(
    value: &str,
    node: &Inline,
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[PassBoundary],
    output: &mut Vec<Inline>,
    blank_run: &mut BlankRun,
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
        while let Some(boundary) = boundaries
            .get(*next_boundary)
            .filter(|end| end.scalar == *cell)
        {
            push_split_text(&mut run, node, output);
            output.push(boundary.projection());
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
            // Measure this blank run once. term.c:205-207 consumes a run
            // only at the accepted pass boundary; a later boundary must
            // preserve the same blanks in source order, without rescanning
            // their suffix at every scalar. When its end lands on the next
            // row boundary, the run is the break's consumed separator
            // (term.c:205-207): the accepted row ends here and the word
            // continues on the next device row.
            let end = blank_run.end_from(&chars, index);
            if let Some(boundary) = boundaries
                .get(*next_boundary)
                .filter(|cut| cut.scalar == *cell + (end - index))
            {
                while run
                    .chars()
                    .next_back()
                    .is_some_and(super::super::super::is_formatter_word_blank)
                {
                    run.pop();
                }
                push_split_text(&mut run, node, output);
                output.push(boundary.projection());
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

pub(super) fn advance_boundary(
    cell: &mut usize,
    next_boundary: &mut usize,
    boundaries: &[PassBoundary],
) {
    if boundaries
        .get(*next_boundary)
        .is_some_and(|end| end.scalar == *cell)
    {
        *next_boundary += 1;
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

#[cfg(test)]
mod blank_run_tests {
    use crate::mandoc::inline::plain_text;

    use super::{
        BlankRun, Inline, PassBoundary, split_text_with_blank_run, split_word_at_row_boundaries,
    };

    #[test]
    fn later_boundary_inspects_each_blank_once() {
        // Exact pristine CVS -Tascii/-Tutf8/-Tlint was run before this
        // assertion for HANG `.No "X                Y\p Z"`: the first
        // blanks stay before Y; only the accepted boundary before Z consumes
        // its separator. term.c:205-207,263-367 decides those boundaries.
        // This tests projection work, independently of device soft wrapping.
        for count in [1_024, 2_048, 4_096, 8_192] {
            let value = format!("X{}Y Z", " ".repeat(count));
            let node = Inline::Text {
                value: value.clone(),
            };
            let mut blank_run = BlankRun::default();
            let mut output = Vec::new();
            let mut cell = 0;
            let mut boundary = 0;
            split_text_with_blank_run(
                &value,
                &node,
                &mut cell,
                &mut boundary,
                &[PassBoundary::hard(count + 3)],
                &mut output,
                &mut blank_run,
            );
            assert_eq!(plain_text(&output), format!("X{}Y\nZ", " ".repeat(count)));
            assert_eq!(blank_run.inspected_cells, count + 1);
            assert_eq!(cell, count + 4);
            assert_eq!(boundary, 1);
        }
    }

    #[test]
    fn accepted_run_end_consumes_only_that_separator() {
        // Exact CVS profiles for HANG `.br` then `.No "X                Y Z"`
        // preserve X / Y Z on separate rows. term.c:205-207 consumes only
        // the blank run at the accepted boundary, retaining the later blank.
        let node = Inline::Text {
            value: "X                Y Z".to_owned(),
        };
        let output = split_word_at_row_boundaries(&[node], &mut vec![17]);
        assert_eq!(plain_text(&output), "X\nY Z");
    }

    #[test]
    fn multiple_run_boundaries_preserve_style_and_source_order() {
        // Exact CVS profiles for `.Sy "X\p    Y\p        Z"` preserve
        // three styled rows. term.c:205-207 consumes each breaking run;
        // wrapping accepted projection in Strong cannot change its order.
        let source = Inline::Strong {
            children: vec![Inline::Text {
                value: "X    Y        Z".to_owned(),
            }],
        };
        let output = split_word_at_row_boundaries(&[source], &mut vec![14, 5]);
        assert_eq!(plain_text(&output), "X\nY\nZ");
        assert!(matches!(&output[..], [Inline::Strong { .. }]));
    }
}
