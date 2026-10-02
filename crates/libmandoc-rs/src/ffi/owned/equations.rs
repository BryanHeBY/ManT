//! Bounded ownership transfer of the native eqn child/sibling forest.

use super::super::raw::{self, CDocument, CEquationBox, CEquationBoxView};
#[cfg(test)]
use super::super::{raw::CNodeView, session::DocumentHandle};
use super::{
    budget::{
        EquationBudget, MAX_OWNED_EQUATION_BYTES, MAX_OWNED_EQUATION_DEPTH,
        MAX_OWNED_EQUATION_NODES,
    },
    strings::checked_string,
};
use crate::{EquationBox, EquationFont, EquationKind, EquationPosition};
#[cfg(test)]
use std::ptr::NonNull;

pub(super) unsafe fn copy_equation(
    document: *const CDocument,
    pointer: *const CEquationBox,
    depth: usize,
    budget: &mut EquationBudget,
) -> Result<Option<EquationBox>, String> {
    // CVS eqn.h and eqn.c::eqn_box_alloc represent a child/sibling forest.
    // Preserve its depth-first prefix and transfer limits on the heap: the
    // supported 256-box path must also fit a Windows main thread's stack.
    struct Frame {
        equation: EquationBox,
        child: *const CEquationBox,
        depth: usize,
    }

    let Some((equation, child, _)) =
        (unsafe { copy_equation_shallow(document, pointer, depth, budget) })?
    else {
        return Ok(None);
    };
    let mut stack = vec![Frame {
        equation,
        child,
        depth,
    }];
    while let Some(mut frame) = stack.pop() {
        if !frame.child.is_null()
            && !budget.truncated
            && let Some((equation, child, next)) =
                unsafe { copy_equation_shallow(document, frame.child, frame.depth + 1, budget) }?
        {
            frame.child = next;
            let child_depth = frame.depth + 1;
            stack.push(frame);
            stack.push(Frame {
                equation,
                child,
                depth: child_depth,
            });
            continue;
        }
        if let Some(parent) = stack.last_mut() {
            parent.equation.children.push(frame.equation);
        } else {
            return Ok(Some(frame.equation));
        }
    }
    Err("libmandoc returned an empty equation traversal".to_owned())
}

unsafe fn copy_equation_shallow(
    document: *const CDocument,
    pointer: *const CEquationBox,
    depth: usize,
    budget: &mut EquationBudget,
) -> Result<Option<(EquationBox, *const CEquationBox, *const CEquationBox)>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    if depth >= MAX_OWNED_EQUATION_DEPTH || budget.nodes >= MAX_OWNED_EQUATION_NODES {
        budget.truncated = true;
        return Ok(None);
    }
    let mut view = std::mem::MaybeUninit::<CEquationBoxView>::uninit();
    if unsafe { raw::mant_mandoc_eqn_box_snapshot(document, pointer, view.as_mut_ptr()) } == 0 {
        return Err("libmandoc returned an invalid borrowed equation box".to_owned());
    }
    let view = unsafe { view.assume_init() };
    let kind = match view.kind {
        0 => EquationKind::Text,
        1 => EquationKind::Subexpression,
        2 => EquationKind::List,
        3 => EquationKind::Pile,
        4 => EquationKind::Matrix,
        _ => return Err("libmandoc returned an unknown equation box kind".to_owned()),
    };
    let font = match view.font {
        0 => EquationFont::None,
        1 => EquationFont::Roman,
        2 => EquationFont::Bold,
        3 => EquationFont::Fat,
        4 => EquationFont::Italic,
        _ => return Err("libmandoc returned an unknown equation font".to_owned()),
    };
    let position = match view.position {
        0 => EquationPosition::None,
        1 => EquationPosition::Superscript,
        2 => EquationPosition::SubscriptSuperscript,
        3 => EquationPosition::Subscript,
        4 => EquationPosition::To,
        5 => EquationPosition::From,
        6 => EquationPosition::FromTo,
        7 => EquationPosition::Over,
        8 => EquationPosition::Sqrt,
        _ => return Err("libmandoc returned an unknown equation position".to_owned()),
    };
    let text = unsafe { checked_string(view.text) }?;
    let left = unsafe { checked_string(view.left) }?;
    let right = unsafe { checked_string(view.right) }?;
    let top = unsafe { checked_string(view.top) }?;
    let bottom = unsafe { checked_string(view.bottom) }?;
    let required_bytes = std::mem::size_of::<EquationBox>()
        + [&text, &left, &right, &top, &bottom]
            .into_iter()
            .filter_map(|part| part.as_ref())
            .map(String::len)
            .sum::<usize>();
    budget.nodes += 1;
    budget.bytes = budget.bytes.saturating_add(required_bytes);
    if budget.bytes > MAX_OWNED_EQUATION_BYTES {
        budget.truncated = true;
        return Ok(None);
    }
    Ok(Some((
        EquationBox {
            kind,
            font,
            position,
            size: view.size,
            expected_args: view.expected_args,
            actual_args: view.actual_args,
            text,
            gnu_ldots: match view.gnu_ldots {
                0 => false,
                1 => true,
                _ => return Err("libmandoc returned an invalid GNU ldots flag".to_owned()),
            },
            left,
            right,
            top,
            bottom,
            children: Vec::new(),
        },
        view.first,
        view.next,
    )))
}

#[cfg(test)]
mod equation_transfer_tests {
    use super::*;

    #[test]
    fn equation_transfer_limits_keep_the_same_depth_first_prefix() {
        // Exact pristine -Ttree source: eqn.c::eqn_box_alloc appends the
        // siblings a, {b c}, d. Exhaustion inside the group must not resume
        // at d, and a rejected box's budget charge remains observable.
        let source = b".TH EQN 1\n.SH BODY\n.EQ\na { b c } d\n.EN\n";
        let pointer = unsafe {
            raw::mant_mandoc_parse_buffer(
                c"budget.1".as_ptr(),
                source.as_ptr(),
                source.len(),
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
            )
        };
        let document = DocumentHandle(NonNull::new(pointer).expect("native document"));
        let mut pending = vec![unsafe { raw::mant_mandoc_document_root(pointer) }];
        let equation = loop {
            let node = pending.pop().expect("native equation node");
            let mut view = std::mem::MaybeUninit::<CNodeView>::uninit();
            assert_ne!(
                unsafe { raw::mant_mandoc_node_snapshot(pointer, node, view.as_mut_ptr()) },
                0
            );
            let view = unsafe { view.assume_init() };
            if !view.equation.is_null() {
                break view.equation;
            }
            if !view.next.is_null() {
                pending.push(view.next);
            }
            if !view.child.is_null() {
                pending.push(view.child);
            }
        };
        for (depth, mut budget, expected_children) in [
            (
                0,
                EquationBudget {
                    nodes: MAX_OWNED_EQUATION_NODES - 3,
                    ..EquationBudget::default()
                },
                2,
            ),
            (MAX_OWNED_EQUATION_DEPTH - 2, EquationBudget::default(), 2),
            (
                0,
                EquationBudget {
                    bytes: MAX_OWNED_EQUATION_BYTES - 2 * std::mem::size_of::<EquationBox>() - 1,
                    ..EquationBudget::default()
                },
                1,
            ),
        ] {
            let initial_nodes = budget.nodes;
            let initial_bytes = budget.bytes;
            let owned = unsafe { copy_equation(pointer, equation, depth, &mut budget) }
                .unwrap()
                .unwrap();
            assert!(budget.truncated);
            assert_eq!(budget.nodes - initial_nodes, 3);
            assert_eq!(
                budget.bytes - initial_bytes,
                3 * std::mem::size_of::<EquationBox>() + 1
            );
            assert_eq!(owned.children.len(), expected_children);
            assert_eq!(owned.children[0].text.as_deref(), Some("a"));
            assert_eq!(owned.children.last().unwrap().children.len(), 0);
            assert!(!owned.readable_text().contains('d'));
        }
        let owned = unsafe { copy_equation(pointer, equation, 0, &mut EquationBudget::default()) }
            .unwrap()
            .unwrap();
        assert_eq!(
            owned
                .children
                .iter()
                .map(|child| child.text.as_deref())
                .collect::<Vec<_>>(),
            [Some("a"), None, Some("d")]
        );
        assert_eq!(
            owned.children[1]
                .children
                .iter()
                .map(|child| child.text.as_deref())
                .collect::<Vec<_>>(),
            [Some("b"), Some("c")]
        );
        // The returned EquationBoxes are independent of the borrowed forest.
        drop(document);
        assert!(owned.readable_text().contains('d'));
    }
}
