//! Check only the recorded Markdown split at effective owner row corrections.

use super::{Links, description, first_content};
use mant_ir::{Inline, InlineContentRef, LinkTarget, ResolvedContent, visit::Visit};

type Fragment = (LinkTarget, String, Option<String>, std::ops::Range<usize>);

pub(super) fn reader_links_are_preserved(
    original: &ResolvedContent,
    reader: &ResolvedContent,
) -> bool {
    let root = first_content(description(original));
    let expected = expected_fragments(root);
    let owner_links = reader_fragments(first_content(description(reader)).content, root);
    let mut all_links = Links(Vec::new());
    all_links.visit_document(reader.document.as_ref().unwrap());
    let identities: Vec<_> = expected
        .iter()
        .map(|(target, label, title, _)| (target.clone(), label.clone(), title.clone()))
        .collect();
    !expected.is_empty() && owner_links == expected && all_links.0 == identities
}

fn expected_fragments(root: InlineContentRef<'_>) -> Vec<Fragment> {
    fn append(
        nodes: &[Inline],
        layout: &mant_ir::InlineLayout,
        row: &mut usize,
        scalar: &mut usize,
        output: &mut Vec<Fragment>,
    ) {
        for node in nodes {
            match node {
                Inline::Link {
                    target,
                    title,
                    children,
                } => {
                    let label = mant_ir::inline_plain_text(children);
                    output.extend(labels(&label, *row, layout).into_iter().map(
                        |(label, chars)| {
                            (
                                target.clone(),
                                label,
                                title.clone(),
                                *scalar + chars.start..*scalar + chars.end,
                            )
                        },
                    ));
                    *row += label.bytes().filter(|byte| *byte == b'\n').count();
                    *scalar += label.chars().count();
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    append(children, layout, row, scalar, output);
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    *row += value.bytes().filter(|byte| *byte == b'\n').count();
                    *scalar += value.chars().count();
                }
                Inline::LineBreak {} => {
                    *row += 1;
                    *scalar += 1;
                }
                Inline::Anchor { .. } => {}
            }
        }
    }
    let mut output = Vec::new();
    append(root.content, root.layout, &mut 0, &mut 0, &mut output);
    output
}

fn labels(
    label: &str,
    start_row: usize,
    layout: &mant_ir::InlineLayout,
) -> Vec<(String, std::ops::Range<usize>)> {
    let rows: Vec<_> = label.split('\n').collect();
    let effective = |index: usize| {
        !rows[index].trim_matches([' ', '\t']).is_empty()
            && mant_ir::geometry::padding(layout.row_indent(start_row + index)) > 0
    };
    if !(0..rows.len()).any(effective) {
        return vec![(label.to_owned(), 0..label.chars().count())];
    }
    let mut boundaries = vec![0];
    let mut byte = 0;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 && effective(index) {
            boundaries.push(byte);
        }
        byte += row.len() + 1;
    }
    boundaries.push(label.len());
    boundaries
        .windows(2)
        .filter_map(|pair| {
            let piece = &label[pair[0]..pair[1]];
            let core = piece.trim_matches('\n');
            let start_byte = pair[0] + piece.len() - piece.trim_start_matches('\n').len();
            let start = label[..start_byte].chars().count();
            (!core.is_empty()).then(|| (core.to_owned(), start..start + core.chars().count()))
        })
        .collect()
}

/// This native cohort authors no leading blanks/NBSP. Exclude only the exact
/// count of NBSP cells that its original occupied rows can generate; no body
/// scalar or unrelated whitespace is normalized away. BODY follows the HEAD
/// in the portable bullet paragraph, but its links cannot borrow HEAD ranges.
fn reader_fragments(nodes: &[Inline], original: InlineContentRef<'_>) -> Vec<Fragment> {
    let plain = mant_ir::inline_plain_text(original.content);
    let padding = plain
        .split('\n')
        .enumerate()
        .map(|(row, value)| {
            if value.trim_matches([' ', '\t']).is_empty() {
                0
            } else {
                mant_ir::geometry::padding(original.layout.row_indent(row))
            }
        })
        .collect::<Vec<_>>();
    let mut cursor = ReaderCursor {
        scalar: 0,
        row: 0,
        remaining: padding.first().copied().unwrap_or(0),
        padding: &padding,
    };
    let mut output = Vec::new();
    cursor.append(nodes, &mut output);
    output
}

struct ReaderCursor<'a> {
    scalar: usize,
    row: usize,
    remaining: usize,
    padding: &'a [usize],
}

impl ReaderCursor<'_> {
    fn text(&mut self, value: &str) {
        for character in value.chars() {
            if character == '\u{a0}' && self.remaining > 0 {
                self.remaining -= 1;
                continue;
            }
            self.scalar += 1;
            if character == '\n' {
                self.row += 1;
                self.remaining = self.padding.get(self.row).copied().unwrap_or(0);
            } else {
                self.remaining = 0;
            }
        }
    }

    fn append(&mut self, nodes: &[Inline], output: &mut Vec<Fragment>) {
        for node in nodes {
            match node {
                Inline::Link {
                    target,
                    title,
                    children,
                } => {
                    let index = output.len();
                    output.push((
                        target.clone(),
                        mant_ir::inline_plain_text(children),
                        title.clone(),
                        self.scalar..self.scalar,
                    ));
                    self.append(children, output);
                    output[index].3.end = self.scalar;
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    self.append(children, output);
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => self.text(value),
                Inline::LineBreak {} => self.text("\n"),
                Inline::Anchor { .. } => {}
            }
        }
    }
}
