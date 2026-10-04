//! Compare complete authored Link identity and exact owner scalar ranges.

use super::{Links, description, first_children};
use mant_ir::{Inline, LinkTarget, ResolvedContent, visit::Visit};

type LinkRange = (LinkTarget, String, Option<String>, std::ops::Range<usize>);

pub(super) fn reader_links_are_preserved(
    original: &ResolvedContent,
    reader: &ResolvedContent,
) -> bool {
    let expected = link_ranges(first_children(description(original)));
    let actual = link_ranges(first_children(description(reader)));
    let mut all_links = Links(Vec::new());
    all_links.visit_document(reader.document.as_ref().unwrap());
    let identities: Vec<_> = expected
        .iter()
        .map(|(target, label, title, _)| (target.clone(), label.clone(), title.clone()))
        .collect();
    !expected.is_empty() && actual == expected && all_links.0 == identities
}

fn link_ranges(nodes: &[Inline]) -> Vec<LinkRange> {
    fn append(nodes: &[Inline], scalar: &mut usize, output: &mut Vec<LinkRange>) {
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
                        *scalar..*scalar,
                    ));
                    append(children, scalar, output);
                    output[index].3.end = *scalar;
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    append(children, scalar, output);
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => *scalar += value.chars().count(),
                Inline::LineBreak {} => *scalar += 1,
                Inline::Anchor { .. } => {}
            }
        }
    }
    let mut output = Vec::new();
    append(nodes, &mut 0, &mut output);
    output
}
