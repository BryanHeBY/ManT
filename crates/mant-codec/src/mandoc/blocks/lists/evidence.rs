//! Capture declaration-head macros before their literal/argument styling loses
//! the native role. This is only called for an existing definition owner.
use crate::definitions::NativeHeadRole;
use libmandoc_rs::Node;

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
