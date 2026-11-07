use mant_ir::LinkTarget;

use super::{
    Font, Inline, TextEventState, TextExecutionPolicy, WordOutput, append_text_event, flush_segment,
};
use crate::mandoc::inline::font::style::styled_segment;
use crate::mandoc::reference::{
    is_manual_reference_name, is_manual_section, trailing_sphinx_manual_reference,
};

// A private, transparent span through native field acceptance and source
// annotation publication. It is never a navigation target in the document.
pub(in crate::mandoc) const SOURCE_CANDIDATE_PREFIX: &str = "\0mant:sphinx-candidate:";
const EMPTY_DESTINATION: &str = " <>";

pub(super) fn append_empty_destination(
    word: &mut WordOutput<'_>,
    policy: TextExecutionPolicy,
    text_state: &mut TextEventState,
) {
    let eligible = policy.recognize_generated_references
        && word.link.is_none()
        && !matches!(
            word.font,
            Font::Code | Font::CodeStrong | Font::CodeEmphasis
        )
        && !word.zero_advance.is_armed()
        && !word.zero_advance.has_buffered_glyph();
    // term_word() writes an invisible graph for \%, followed by two normal
    // glyphs. Those glyphs participate in overstrike and word break effects
    // even if their source spelling may later qualify for navigation.
    text_state.note_graph();
    text_state.zero_graph_seen = true;
    append_text_event(
        "<>",
        WordOutput {
            output: &mut *word.output,
            buffer: &mut *word.buffer,
            font: word.font,
            link: word.link,
            zero_advance: &mut *word.zero_advance,
        },
        text_state,
        policy.field_authoritative,
    );
    let Some(before_marker) = word.buffer.strip_suffix("<>").filter(|_| eligible) else {
        return;
    };
    let span = trailing_sphinx_manual_reference(before_marker)
        .map(|reference| (reference.prefix.len(), reference.display))
        .or_else(|| {
            // man_term.c::pre_alternate executes the section in a separate
            // Roman word after the bold/italic name. Carry only the exact
            // marker's span here. The name must survive alongside it.
            let suffix = before_marker.strip_suffix(' ')?;
            (word.font == Font::Regular && section_operand(suffix).is_some()).then_some((0, suffix))
        });
    let Some((prefix_bytes, display)) = span else {
        return;
    };
    let display = display.to_owned();
    word.buffer.truncate(prefix_bytes);
    flush_segment(word.output, word.buffer, word.font, None);
    let target = word
        .zero_advance
        .annotations
        .get_or_insert_with(Default::default)
        .register_source_candidate();
    word.output.push(Inline::Link {
        target,
        children: vec![styled_segment(
            format!("{display}{EMPTY_DESTINATION}"),
            word.font,
        )],
        // Match the exact accepted span before committing. A native field
        // may retain only a prefix, or split an occurrence across owners.
        title: Some(display),
    });
}

pub(in crate::mandoc) fn finish_source_candidates(nodes: &mut Vec<Inline>) -> bool {
    publish_candidates(nodes, false)
}

/// A source label can outlive its public wrapper, for example after a block
/// equation or a foreign reference cuts its first representable owner.
/// Freeze the source-label decision before that publication cuts the tree.
pub(in crate::mandoc) fn protect_source_label_candidates(nodes: &mut [Inline]) {
    fn protect(nodes: &mut [Inline], in_link: bool) {
        for node in nodes {
            match node {
                Inline::Link {
                    target: LinkTarget::External { uri },
                    title,
                    ..
                } if uri.starts_with(SOURCE_CANDIDATE_PREFIX) => {
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
            } if uri.starts_with(SOURCE_CANDIDATE_PREFIX) => {
                changed = true;
                if !in_link
                    && let Some(display) = title.as_deref()
                    && let Some(candidate) = eligible_candidate(children, display, output.last())
                {
                    // A traditional split-font reference is published by
                    // navigation's own name/section recognizer. Here only
                    // the source-proven suffix is removed.
                    let mut marker_bytes = EMPTY_DESTINATION.len();
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
    if accepted.strip_suffix(EMPTY_DESTINATION)? != display {
        return None;
    }
    if section_operand(display).is_some() {
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

fn section_operand(value: &str) -> Option<&str> {
    let section = value.strip_prefix('(')?.strip_suffix(')')?;
    is_manual_section(section).then_some(section)
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
