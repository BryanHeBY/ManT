//! Capture declaration-head macros before their literal/argument styling loses
//! the native role. This is only called for an existing definition owner.
use crate::definitions::NativeHeadRole;
use libmandoc_rs::Node;
use std::{collections::HashMap, ops::Range};

const OPTION_START: &str = "\0mant-native-option-start:";
const OPTION_END: &str = "\0mant-native-option-end:";

/// Consume definition-head-only zero-width witnesses before committing draft
/// content. Each range is in the final visible term, not in roff source bytes;
/// a macro's generated dash and executed escapes therefore use one coordinate.
/// Unpaired marks are discarded, never promoted to evidence or public anchors.
pub(super) fn take_option_ranges(
    terms: &mut [Vec<crate::mandoc::inline::DraftInline>],
) -> Vec<Vec<Range<usize>>> {
    fn strip(
        nodes: &mut Vec<crate::mandoc::inline::DraftInline>,
        offset: &mut usize,
        open: &mut HashMap<usize, usize>,
        ranges: &mut Vec<Range<usize>>,
    ) {
        use crate::mandoc::inline::DraftInline;
        nodes.retain_mut(|node| match node {
            DraftInline::Anchor { id, .. } => {
                let parse = |prefix: &str| {
                    id.as_str()
                        .strip_prefix(prefix)
                        .and_then(|hex| usize::from_str_radix(hex, 16).ok())
                };
                if let Some(key) = parse(OPTION_START) {
                    open.insert(key, *offset);
                    false
                } else if let Some(key) = parse(OPTION_END) {
                    if let Some(start) = open.remove(&key)
                        && start < *offset
                    {
                        ranges.push(start..*offset);
                    }
                    false
                } else {
                    true
                }
            }
            DraftInline::Text { value } | DraftInline::Code { value } => {
                *offset += value.len();
                true
            }
            DraftInline::Strong { children }
            | DraftInline::Emphasis { children }
            | DraftInline::Link { children, .. } => {
                strip(children, offset, open, ranges);
                true
            }
            DraftInline::LineBreak => {
                *offset += 1;
                true
            }
        });
    }

    terms
        .iter_mut()
        .map(|term| {
            let mut offset = 0;
            let mut open = HashMap::new();
            let mut ranges = Vec::new();
            strip(term, &mut offset, &mut open, &mut ranges);
            ranges.sort_by_key(|range| range.start);
            ranges
        })
        .collect()
}

pub(super) fn leading_role(nodes: &[Node]) -> Option<NativeHeadRole> {
    enum HeadStart {
        Typed(NativeHeadRole),
        Other,
    }
    fn first(node: &Node) -> Option<HeadStart> {
        match node.macro_name.as_deref() {
            Some("Fl") => return Some(HeadStart::Typed(NativeHeadRole::Option)),
            Some("Ev") => return Some(HeadStart::Typed(NativeHeadRole::Environment)),
            Some("Ic" | "Cm") => return Some(HeadStart::Typed(NativeHeadRole::Literal)),
            Some("Ar" | "Em" | "Sy") => return Some(HeadStart::Other),
            Some("Tg" | "Ns" | "Sm") => return None,
            _ => {}
        }
        if node
            .text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
        {
            return Some(HeadStart::Other);
        }
        node.children.iter().find_map(first)
    }
    match nodes.iter().find_map(first)? {
        HeadStart::Typed(role) => Some(role),
        HeadStart::Other => None,
    }
}
