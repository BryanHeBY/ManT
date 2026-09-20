//! Source-neutral presentation classification for executed man(7) fields.
//!
//! Fixed CVS `man_term.c::pre_IP()` and `pre_TP()` execute every tagged
//! paragraph through the same formatter field machinery. Bullet and ordinal
//! recovery consumes the exact authored word facts together with the already
//! materialized field; it never searches a finished IR tree by coordinates.

use libmandoc_rs::{AtomRole, ExecutionManBlockKind, NativeExecutionReport};
use mant_ir::Inline;

use super::super::NativeManBlock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OrdinalStyle {
    Period,
    ClosingParenthesis,
    Parenthesized,
    IncrementingRegister,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Ordinal {
    pub(super) value: u64,
    pub(super) style: OrdinalStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Presentation {
    Definition,
    Bullet,
    Ordered(Ordinal),
}

pub(super) fn classify(
    terms: &[Vec<Inline>],
    description_is_empty: bool,
    native: &NativeManBlock,
    report: &NativeExecutionReport,
) -> Presentation {
    if description_is_empty
        || !matches!(
            native.kind,
            ExecutionManBlockKind::IndentedParagraph | ExecutionManBlockKind::TaggedParagraph
        )
    {
        return Presentation::Definition;
    }
    let operands = report
        .words()
        .iter()
        .filter(|word| {
            word.role == AtomRole::Authored
                && native.head.atoms.start <= word.atoms.start
                && word.atoms.end <= native.head.atoms.end
        })
        .filter_map(|word| report.pool_bytes(word.operand))
        .collect::<Vec<_>>();
    let term = terms
        .first()
        .map_or_else(String::new, |term| mant_ir::inline_plain_text(term));
    if term.trim() == "•"
        && operands.iter().any(|operand| {
            let source = String::from_utf8_lossy(operand);
            source.contains(r"\(bu") || source.contains(r"\[bu]")
        })
    {
        return Presentation::Bullet;
    }
    ordinal(
        term.trim(),
        operands
            .iter()
            .any(|operand| String::from_utf8_lossy(operand).contains(r"\n+")),
    )
    .map_or(Presentation::Definition, Presentation::Ordered)
}

fn ordinal(text: &str, incrementing_register: bool) -> Option<Ordinal> {
    let (digits, style) = if let Some(digits) = text.strip_suffix('.') {
        (digits, OrdinalStyle::Period)
    } else if let Some(digits) = text.strip_suffix(')') {
        if let Some(digits) = digits.strip_prefix('(') {
            (digits, OrdinalStyle::Parenthesized)
        } else {
            (digits, OrdinalStyle::ClosingParenthesis)
        }
    } else if incrementing_register {
        (text, OrdinalStyle::IncrementingRegister)
    } else {
        return None;
    };
    Some(Ordinal {
        value: digits.parse().ok()?,
        style,
    })
}
