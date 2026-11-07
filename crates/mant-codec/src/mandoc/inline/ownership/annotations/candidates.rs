//! Finalize source-proven reference candidates after native acceptance.
use mant_ir::{Inline, LinkTarget};

use crate::mandoc::reference::{
    SPHINX_EMPTY_DESTINATION, is_manual_reference_name, manual_section_operand,
    trailing_sphinx_manual_reference,
};

use super::markers;

pub(super) fn finish_source_candidates(nodes: &mut Vec<Inline>) -> bool {
    publish_candidates(nodes, false)
}

/// A source label can outlive its public wrapper, for example after a block
/// equation or a foreign reference cuts its first representable owner.
/// Freeze the source-label decision before that publication cuts the tree.
pub(super) fn protect_source_label_candidates(nodes: &mut [Inline]) {
    fn protect(nodes: &mut [Inline], in_link: bool) {
        for node in nodes {
            match node {
                Inline::Link {
                    target: LinkTarget::External { uri },
                    title,
                    ..
                } if markers::is_candidate(uri) => {
                    if in_link {
                        *title = None;
                    }
                }
                Inline::Link { children, .. } => protect(children, true),
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    protect(children, in_link);
                }
                _ => {}
            }
        }
    }
    protect(nodes, false);
}

fn publish_candidates(nodes: &mut Vec<Inline>, in_link: bool) -> bool {
    let mut changed = false;
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        match &mut node {
            Inline::Link {
                target: LinkTarget::External { uri },
                title,
                children,
            } if markers::is_candidate(uri) => {
                changed = true;
                if !in_link
                    && let Some(display) = title.as_deref()
                    && let Some(candidate) = eligible_candidate(children, display, output.last())
                {
                    // A traditional split-font reference is published by
                    // navigation's own name/section recognizer. Here only
                    // the source-proven suffix is removed.
                    let mut marker_bytes = SPHINX_EMPTY_DESTINATION.len();
                    trim_suffix(children, &mut marker_bytes);
                    if let Candidate::Complete(target) = candidate {
                        output.push(Inline::Link {
                            target,
                            title: None,
                            children: std::mem::take(children),
                        });
                    } else {
                        output.append(children);
                    }
                } else {
                    output.append(children);
                }
                continue;
            }
            Inline::Link { children, .. } => changed |= publish_candidates(children, true),
            Inline::Strong { children } | Inline::Emphasis { children } => {
                changed |= publish_candidates(children, in_link);
            }
            _ => {}
        }
        output.push(node);
    }
    *nodes = output;
    changed
}

enum Candidate {
    Complete(LinkTarget),
    SectionOperand,
}

fn eligible_candidate(
    children: &[Inline],
    display: &str,
    previous: Option<&Inline>,
) -> Option<Candidate> {
    let accepted = mant_ir::inline_plain_text(children);
    if accepted.strip_suffix(SPHINX_EMPTY_DESTINATION)? != display {
        return None;
    }
    if manual_section_operand(display).is_some() {
        let (Inline::Strong { children } | Inline::Emphasis { children }) = previous? else {
            return None;
        };
        let [Inline::Text { value }] = children.as_slice() else {
            return None;
        };
        return is_manual_reference_name(value).then_some(Candidate::SectionOperand);
    }
    let reference = trailing_sphinx_manual_reference(accepted.strip_suffix("<>")?)?;
    Some(Candidate::Complete(LinkTarget::Manual {
        name: reference.name.to_owned(),
        manual_section: Some(reference.manual_section.to_owned()),
    }))
}

fn trim_suffix(nodes: &mut [Inline], remaining: &mut usize) {
    for node in nodes.iter_mut().rev() {
        match node {
            Inline::Text { value } => {
                let removed = value.len().min(*remaining);
                value.truncate(value.len() - removed);
                *remaining -= removed;
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                trim_suffix(children, remaining);
            }
            _ => {}
        }
        if *remaining == 0 {
            break;
        }
    }
}
