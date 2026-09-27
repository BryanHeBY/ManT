//! Capture declaration-head macros before their literal/argument styling loses
//! the native role. This is only called for an existing definition owner.
use crate::definitions::{NativeHeadComponent, NativeHeadRole};
use libmandoc_rs::Node;
use std::{collections::HashMap, ops::Range};

const OPTION_START: &str = "\0mant-native-option-start:";
const OPTION_END: &str = "\0mant-native-option-end:";
const OPERAND_START: &str = "\0mant-native-operand-start:";
const OPERAND_END: &str = "\0mant-native-operand-end:";
const ARGUMENT_START: &str = "\0mant-native-argument-start:";
const ARGUMENT_END: &str = "\0mant-native-argument-end:";
const LITERAL_START: &str = "\0mant-native-component-literal-start:";
const LITERAL_END: &str = "\0mant-native-component-literal-end:";
const VARIABLE_START: &str = "\0mant-native-component-variable-start:";
const VARIABLE_END: &str = "\0mant-native-component-variable-end:";

pub(super) struct HeadRanges {
    pub(super) option_ranges: Vec<Vec<Range<usize>>>,
    pub(super) operand_ranges: Vec<Vec<Range<usize>>>,
    pub(super) literal_argument_ranges: Vec<Vec<Range<usize>>>,
    pub(super) components: Vec<Vec<NativeHeadComponent>>,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum MarkKind {
    Option,
    Operand,
    Argument,
    Literal,
    Variable,
}

#[derive(Default)]
struct TermRanges {
    open: HashMap<(MarkKind, usize), usize>,
    closed: Vec<(MarkKind, Range<usize>)>,
}

impl TermRanges {
    fn marker(&mut self, id: &str, offset: usize) -> bool {
        let marks = [
            (MarkKind::Option, OPTION_START, OPTION_END),
            (MarkKind::Operand, OPERAND_START, OPERAND_END),
            (MarkKind::Argument, ARGUMENT_START, ARGUMENT_END),
            (MarkKind::Literal, LITERAL_START, LITERAL_END),
            (MarkKind::Variable, VARIABLE_START, VARIABLE_END),
        ];
        for (kind, start, end) in marks {
            if let Some(key) = id
                .strip_prefix(start)
                .and_then(|hex| usize::from_str_radix(hex, 16).ok())
            {
                self.open.insert((kind, key), offset);
                return true;
            }
            if let Some(key) = id
                .strip_prefix(end)
                .and_then(|hex| usize::from_str_radix(hex, 16).ok())
            {
                if let Some(start) = self.open.remove(&(kind, key))
                    && start < offset
                {
                    self.closed.push((kind, start..offset));
                }
                return true;
            }
        }
        false
    }
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
        ranges: &mut TermRanges,
    ) {
        use crate::mandoc::inline::DraftInline;
        nodes.retain_mut(|node| match node {
            DraftInline::Anchor { id, .. } => !ranges.marker(id.as_str(), *offset),
            DraftInline::Text { value } | DraftInline::Code { value } => {
                *offset += value.len();
                true
            }
            DraftInline::Strong { children }
            | DraftInline::Emphasis { children }
            | DraftInline::Link { children, .. } => {
                strip(children, offset, ranges);
                true
            }
            DraftInline::LineBreak => {
                *offset += 1;
                true
            }
        });
    }

    let mut result = HeadRanges {
        option_ranges: Vec::with_capacity(terms.len()),
        operand_ranges: Vec::with_capacity(terms.len()),
        literal_argument_ranges: Vec::with_capacity(terms.len()),
        components: Vec::with_capacity(terms.len()),
    };
    for term in terms {
        let mut offset = 0;
        let mut ranges = TermRanges::default();
        strip(term, &mut offset, &mut ranges);
        ranges.closed.sort_by_key(|(_, range)| range.start);
        let mut option = Vec::new();
        let mut operand = Vec::new();
        let mut argument = Vec::new();
        let mut components = Vec::new();
        for (kind, range) in ranges.closed {
            match kind {
                MarkKind::Option => option.push(range),
                MarkKind::Operand => operand.push(range),
                MarkKind::Argument => argument.push(range),
                MarkKind::Literal => components.push(NativeHeadComponent {
                    role: NativeHeadRole::Literal,
                    range,
                }),
                MarkKind::Variable => components.push(NativeHeadComponent {
                    role: NativeHeadRole::Variable,
                    range,
                }),
            }
        }
        result.option_ranges.push(option);
        result.operand_ranges.push(operand);
        result.literal_argument_ranges.push(argument);
        result.components.push(components);
    }
    result
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
            // mdoc_term.c::termp_under_pre/termp_li_pre choose initial fonts,
            // but term_word() may override them. Preserve the native role;
            // font alone cannot distinguish Va/Dv/Ar/No.
            Some("Va") => return Some(HeadStart::Typed(NativeHeadRole::Variable)),
            Some("Dv") => return Some(HeadStart::Typed(NativeHeadRole::DefinedVariable)),
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
