//! Operand-only requests must never enter generic printable-child fallback.
//!
//! Payload ownership and logical-sibling transparency are independent facts:
//! ft/Sm still execute state, Tg still owns a target, and ce/rj own real text
//! after their count operand. This classification is not a blanket ignore list.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OperandControl {
    Font,
    Spacing,
    Fill,
    Indent,
    ParagraphDistance,
    Delimiters,
    /// Device/page presentation omitted by the source-neutral reader.
    Presentation,
}

/// Classify controls whose children contain operands only. Each consumer
/// executes its supported effects before applying the no-text fallback.
/// Break/space requests and payload-bearing ce/rj have separate dispatch.
pub(super) fn operand_control(name: Option<&str>) -> Option<OperandControl> {
    Some(match name? {
        "ft" => OperandControl::Font,
        "Sm" => OperandControl::Spacing,
        "nf" | "fi" => OperandControl::Fill,
        "in" => OperandControl::Indent,
        "PD" => OperandControl::ParagraphDistance,
        // Native validation has already attached these to later En nodes.
        "Es" => OperandControl::Delimiters,
        "ad" | "na" | "hy" | "nh" | "ne" | "nr" | "ta" | "ti" | "ll" | "mc" | "po" => {
            OperandControl::Presentation
        }
        _ => return None,
    })
}
