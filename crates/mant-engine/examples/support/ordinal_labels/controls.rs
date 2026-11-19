//! Which native control children execute as output words.

use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, RoffMacro,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceControl {
    Skip,
    State,
    SpacingMode,
    Margin,
    LineBoundary,
    VerticalSpace,
    AlignedRows,
}

pub(crate) fn source_control(node: &Node) -> Option<SourceControl> {
    use SourceControl::{
        AlignedRows, LineBoundary, Margin, Skip, SpacingMode, State, VerticalSpace,
    };
    Some(match node.macro_token.as_ref()? {
        // mdoc_term.c: skip pre-handlers and state-only spacing handler.
        Mdoc(MdocMacro::Db | MdocMacro::Es | MdocMacro::Tg) => Skip,
        Mdoc(MdocMacro::Sm) => SpacingMode,
        // roff_term.c consumes request arguments itself. ce/rj additionally
        // execute the row content that follows the parser's first count child.
        Roff(RoffMacro::Ce | RoffMacro::Rj) => AlignedRows,
        Roff(RoffMacro::Mc) => Margin,
        Roff(RoffMacro::Br | RoffMacro::Fi | RoffMacro::Nf | RoffMacro::Ti) => LineBoundary,
        Roff(RoffMacro::Sp) | Mdoc(MdocMacro::Pp) => VerticalSpace,
        Roff(RoffMacro::Ft | RoffMacro::Ll | RoffMacro::Po | RoffMacro::Ta)
        | Man(ManMacro::In | ManMacro::Dt | ManMacro::Pd) => State,
        _ => return None,
    })
}
