//! Capture declaration-head macros before their literal/argument styling loses
//! the native role. This is only called for an existing definition owner.
use crate::definitions::NativeHeadRole;
use libmandoc_rs::Node;
use std::{collections::HashMap, ops::Range};

const OPTION_START: &str = "\0mant-native-option-start:";
const OPTION_END: &str = "\0mant-native-option-end:";
const OPERAND_START: &str = "\0mant-native-operand-start:";
const OPERAND_END: &str = "\0mant-native-operand-end:";

pub(super) struct HeadRanges {
    pub(super) option_ranges: Vec<Vec<Range<usize>>>,
    pub(super) operand_ranges: Vec<Vec<Range<usize>>>,
}

/// Consume definition-head-only zero-width witnesses before committing draft
/// content. Each range is in the final visible term, not in roff source bytes;
/// a macro's generated dash and executed escapes therefore use one coordinate.
/// Unpaired marks are discarded, never promoted to evidence or public anchors.
pub(super) fn take_head_ranges(
    terms: &mut [Vec<crate::mandoc::inline::DraftInline>],
) -> HeadRanges {
    fn strip(
        nodes: &mut Vec<crate::mandoc::inline::DraftInline>,
        offset: &mut usize,
        option_open: &mut HashMap<usize, usize>,
        operand_open: &mut HashMap<usize, usize>,
        option_ranges: &mut Vec<Range<usize>>,
        operand_ranges: &mut Vec<Range<usize>>,
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
                    option_open.insert(key, *offset);
                    false
                } else if let Some(key) = parse(OPTION_END) {
                    if let Some(start) = option_open.remove(&key)
                        && start < *offset
                    {
                        option_ranges.push(start..*offset);
                    }
                    false
                } else if let Some(key) = parse(OPERAND_START) {
                    operand_open.insert(key, *offset);
                    false
                } else if let Some(key) = parse(OPERAND_END) {
                    if let Some(start) = operand_open.remove(&key)
                        && start < *offset
                    {
                        operand_ranges.push(start..*offset);
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
                strip(
                    children,
                    offset,
                    option_open,
                    operand_open,
                    option_ranges,
                    operand_ranges,
                );
                true
            }
            DraftInline::LineBreak => {
                *offset += 1;
                true
            }
        });
    }

    let (option_ranges, operand_ranges) = terms
        .iter_mut()
        .map(|term| {
            let mut offset = 0;
            let mut option_open = HashMap::new();
            let mut operand_open = HashMap::new();
            let mut option_ranges = Vec::new();
            let mut operand_ranges = Vec::new();
            strip(
                term,
                &mut offset,
                &mut option_open,
                &mut operand_open,
                &mut option_ranges,
                &mut operand_ranges,
            );
            option_ranges.sort_by_key(|range| range.start);
            operand_ranges.sort_by_key(|range| range.start);
            (option_ranges, operand_ranges)
        })
        .unzip();
    HeadRanges {
        option_ranges,
        operand_ranges,
    }
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
