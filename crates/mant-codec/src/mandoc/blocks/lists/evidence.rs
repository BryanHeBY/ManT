//! Capture declaration-head macros before their literal/argument styling loses
//! the native role. This is only called for an existing definition owner.
use crate::definitions::NativeHeadRole;
use libmandoc_rs::Node;
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

pub(super) fn leading_role(nodes: &[Node]) -> Option<NativeHeadRole> {
    enum HeadStart {
        Typed(NativeHeadRole),
        Other,
    }
    fn first(node: &Node) -> Option<HeadStart> {
        match node.macro_token.as_ref() {
            Some(Mdoc(MdocMacro::Fl)) => return Some(HeadStart::Typed(NativeHeadRole::Option)),
            Some(Mdoc(MdocMacro::Ev)) => {
                return Some(HeadStart::Typed(NativeHeadRole::Environment));
            }
            Some(Mdoc(MdocMacro::Ic | MdocMacro::Cm)) => {
                return Some(HeadStart::Typed(NativeHeadRole::Literal));
            }
            Some(Mdoc(MdocMacro::Ar | MdocMacro::Em | MdocMacro::Sy)) => {
                return Some(HeadStart::Other);
            }
            Some(Mdoc(MdocMacro::Tg | MdocMacro::Ns | MdocMacro::Sm)) => return None,
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
