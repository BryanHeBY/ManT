//! Operand-only requests must never enter generic printable-child fallback.
//!
//! Payload ownership and logical-sibling transparency are independent facts:
//! ft/Sm still execute state, Tg still owns a target, and ce/rj own real text
//! after their count operand. This classification is not a blanket ignore list.

use libmandoc_rs::{
    MacroToken,
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, RoffMacro,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OperandControl {
    Font,
    Spacing,
    Fill,
    Indent,
    ParagraphDistance,
    Delimiters,
    /// Native validation already consumed legacy manual metadata.
    Metadata,
    /// Device/page presentation omitted by the source-neutral reader.
    Presentation,
}

/// Terminal execution boundary owned by a roff request.
///
/// This is independent from whether the request's operands are printable.
/// The native renderer dispatches these requests from block, display, and
/// inline trees alike, so every lowering path must consume the same effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FormatterBoundary {
    None,
    Line,
    NoBreak,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FormatterControl {
    pub(super) boundary: FormatterBoundary,
    /// Specialized consumers additionally own payload, vertical distance, or
    /// mode changes. Generic consumers must execute only non-specialized
    /// boundaries, avoiding a second flush around those handlers.
    pub(super) specialized: bool,
    /// This request calls `term_newln()` before its other effects, and its
    /// specialized Rust handler does not settle the shared execution row.
    pub(super) settle_before_handler: bool,
}

/// Classify formatter control requests using the pinned CVS `roff_term_pre()`
/// contract. Device geometry is intentionally omitted, but its execution
/// boundary remains observable through pending `\z`, `\c`, and `\p` state.
pub(super) fn formatter_control(name: Option<&MacroToken>) -> Option<FormatterControl> {
    let (boundary, specialized, settle_before_handler) = match name? {
        // `mc` uses TERMP_NOBREAK around term_flushln(); `ti` calls the
        // ordinary break handler before installing its temporary indent.
        Roff(RoffMacro::Mc) => (FormatterBoundary::NoBreak, false, false),
        // The shared ti handler now owns pre_br as well. A block-level
        // preparatory hard_break would consume that same row twice.
        // roff_term_pre_ce() calls pre_br() before centering/right-justifying;
        // man_term.c::pre_in() calls term_newln() before changing its offset.
        Roff(RoffMacro::Ce | RoffMacro::Rj) | Man(ManMacro::In) => {
            (FormatterBoundary::Line, true, true)
        }
        // These requests own additional behavior in block/display lowering.
        Roff(RoffMacro::Br | RoffMacro::Fi | RoffMacro::Nf | RoffMacro::Sp | RoffMacro::Ti)
        | Man(ManMacro::Ex | ManMacro::Ee)
        | Mdoc(MdocMacro::Pp) => (FormatterBoundary::Line, true, false),
        // State-only or device/page presentation controls do not flush.
        Roff(
            RoffMacro::Ft
            | RoffMacro::Ad
            | RoffMacro::Na
            | RoffMacro::Hy
            | RoffMacro::Nh
            | RoffMacro::Ne
            | RoffMacro::Nr
            | RoffMacro::Ta
            | RoffMacro::Ll
            | RoffMacro::Po,
        )
        | Man(ManMacro::Pd | ManMacro::Dt)
        | Mdoc(MdocMacro::Sm | MdocMacro::Tg) => (FormatterBoundary::None, true, false),
        _ => return None,
    };
    Some(FormatterControl {
        boundary,
        specialized,
        settle_before_handler,
    })
}

/// Classify controls whose children contain operands only. Each consumer
/// executes its supported effects before applying the no-text fallback.
/// Break/space requests and payload-bearing ce/rj have separate dispatch.
pub(super) fn operand_control(name: Option<&MacroToken>) -> Option<OperandControl> {
    Some(match name? {
        Roff(RoffMacro::Ft) => OperandControl::Font,
        Mdoc(MdocMacro::Sm) => OperandControl::Spacing,
        Roff(RoffMacro::Nf | RoffMacro::Fi) => OperandControl::Fill,
        Man(ManMacro::In) => OperandControl::Indent,
        Man(ManMacro::Pd) => OperandControl::ParagraphDistance,
        // Native validation has already attached these to later En nodes.
        Mdoc(MdocMacro::Es) => OperandControl::Delimiters,
        // man_term_acts marks UC/AT MAN_NOTEXT. They set the OS string in
        // man_validate, not a paragraph, even when retained in the root AST.
        Man(ManMacro::Uc | ManMacro::At) => OperandControl::Metadata,
        Roff(
            RoffMacro::Ad
            | RoffMacro::Na
            | RoffMacro::Hy
            | RoffMacro::Nh
            | RoffMacro::Ne
            | RoffMacro::Nr
            | RoffMacro::Ta
            | RoffMacro::Ti
            | RoffMacro::Ll
            | RoffMacro::Mc
            | RoffMacro::Po,
        )
        | Man(ManMacro::Dt) => OperandControl::Presentation,
        _ => return None,
    })
}
