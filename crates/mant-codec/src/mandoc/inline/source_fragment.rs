//! Parse a bounded, self-contained inline man/mdoc fragment from a tbl cell.
//!
//! CVS mandoc deliberately sends high-level macro *operands* to `tbl_read()`,
//! whereas GNU tbl expands the same inline macro language before formatting.
//! The native table snapshot consequently cannot retain the typed structure of
//! an admitted `T{}` fragment.  This module recovers only that closed inline
//! language.  Requests needing document/session state remain the native
//! cell's responsibility; callers must retain the whole native/raw cell when
//! this parser declines or cannot prove completion.

use libmandoc_rs::{
    Compression, IncludePolicy, InputFormat, MacroSet,
    MacroToken::{Man, Mdoc},
    ManMacro, MdocMacro, NodeKind, ParseOptions, Parser,
};
use mant_ir::Inline;
use std::cell::RefCell;

use crate::mandoc::table_recovery_budget::{MAX_FRAGMENT_BYTES, TableRecoveryBudget};

use super::{InlineBuilder, append_inline_node_with_next, append_man_link};

#[must_use]
pub(in crate::mandoc) struct RecoveredFragment {
    pub(in crate::mandoc) inlines: Vec<Inline>,
    pub(in crate::mandoc) complete: bool,
    pub(in crate::mandoc) formatter: crate::mandoc::formatter::FormatterState,
    /// Output traversal receipt; zero when optional work was not budgeted.
    pub(in crate::mandoc) output_text_bytes: usize,
}

/// Recover a cell only when every request belongs to the closed inline
/// language and the source does not require the original roff session.
pub(in crate::mandoc) fn lower_source_fragment_with_formatter_state(
    source: &str,
    initial_escape: Option<u8>,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
    formatter: crate::mandoc::formatter::FormatterState,
    budget: Option<&RefCell<TableRecoveryBudget>>,
) -> Option<RecoveredFragment> {
    if !charge_fragment_input(budget, source.len()) {
        return None;
    }
    let mut requests = 0usize;
    for line in source.lines() {
        let Some(request) = line.trim_start().strip_prefix(['.', '\'']) else {
            continue;
        };
        let name = request.split_whitespace().next().unwrap_or_default();
        if !inline_request(name, dialect) {
            return None;
        }
        requests = requests.saturating_add(1);
    }
    // `.eo` is an executed lexical mode, not an absent table escape. In that
    // mode `\\fI` is authored text, so it must neither admit this recovery
    // merely because it resembles a font escape nor be decoded a second time.
    let has_font_escape = initial_escape != Some(0)
        && super::decode(source).iter().any(|event| {
            matches!(
                event,
                super::RoffInlineEvent::Font(_) | super::RoffInlineEvent::PreviousFont
            )
        });
    if requests == 0 && !has_font_escape {
        return None;
    }

    let fallback = || {
        let mut recovered = incomplete_fragment(source, &formatter);
        recovered.output_text_bytes = admit_fragment_output(&recovered.inlines, budget)?;
        Some(recovered)
    };
    // A separate parser invocation is intentionally small and finite.  More
    // importantly, strings, registers and macro arguments would otherwise be
    // evaluated against the synthetic document rather than the real session.
    if requests > 64 || requires_native_evaluation(source, initial_escape) {
        return fallback();
    }

    // A tbl row always records an executed escape state. Do not invent `.eo`
    // for a synthetic or incomplete caller that lacks this fact.
    let Some(escape) = initial_escape else {
        return fallback();
    };
    let escape_bytes = match escape {
        b'\\' => 0,
        0 => 4,
        escape if escape.is_ascii_graphic() => 6,
        _ => return fallback(),
    };
    if !charge_fragment_input(budget, escape_bytes) {
        return None;
    }
    let escape_prefix = match escape {
        b'\\' => String::new(),
        0 => ".eo\n".to_owned(),
        escape if escape.is_ascii_graphic() => {
            format!(".ec {}\n", char::from(escape))
        }
        // libmandoc stores `.ec` as one byte. A non-ASCII byte cannot be
        // faithfully reconstructed as a Rust source character here.
        _ => return fallback(),
    };

    let (input, format) = fragment_input(
        source,
        &escape_prefix,
        dialect,
        default_name,
        synopsis,
        budget,
    )?;
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .with_input_format(format)
    .parse_bytes("mant-table-fragment.1", input.as_bytes());
    let Ok(mut report) = report else {
        return fallback();
    };
    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code().is_some())
    {
        return fallback();
    }
    clear_synthetic_targets(&mut report.document.root);
    let section = report.document.root.children.iter().rev().find(|node| {
        matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::Sh) | Man(ManMacro::Sh))
        ) && node.kind == NodeKind::Block
    })?;
    let body = section
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)?;
    let mut formatter = formatter;
    let inlines = lower_body(&body.children, default_name, &mut formatter);
    let output_text_bytes = admit_fragment_output(&inlines, budget)?;
    Some(RecoveredFragment {
        inlines,
        complete: true,
        formatter,
        output_text_bytes,
    })
}

fn admit_fragment_output(
    inlines: &[Inline],
    budget: Option<&RefCell<TableRecoveryBudget>>,
) -> Option<usize> {
    budget.map_or(Some(0), |budget| {
        budget.borrow_mut().charge_output_with_text_bytes(inlines)
    })
}

fn fragment_input(
    source: &str,
    escape_prefix: &str,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
    budget: Option<&RefCell<TableRecoveryBudget>>,
) -> Option<(String, InputFormat)> {
    let section = if synopsis { "SYNOPSIS" } else { "DESCRIPTION" };
    let (prefix, format) = match dialect {
        MacroSet::Mdoc => {
            let name = default_name.unwrap_or("table-fragment");
            let before = ".Dd January 1, 2000\n.Dt MANT-TABLE 1\n.Os\n.Sh NAME\n.Nm \"";
            let after = "\"\n.Nd table fragment\n.Sh ";
            let minimum_bytes = before
                .len()
                .saturating_add(name.len())
                .saturating_add(after.len())
                .saturating_add(section.len())
                .saturating_add(1);
            // Metadata can be much larger than a cell. Establish the byte
            // bound before walking its scalars or escaping/copying its name.
            if !charge_fragment_input(budget, minimum_bytes) {
                return None;
            }
            let escaped_bytes = name.chars().fold(0usize, |bytes, character| {
                bytes.saturating_add(match character {
                    '\\' => 2,
                    '"' => 4,
                    '\n' | '\r' => 1,
                    character => character.len_utf8(),
                })
            });
            let bytes = before
                .len()
                .saturating_add(escaped_bytes)
                .saturating_add(after.len())
                .saturating_add(section.len())
                .saturating_add(1);
            if !charge_fragment_input(budget, bytes) {
                return None;
            }
            let name = name
                .replace('\\', "\\e")
                .replace('"', "\\(dq")
                .replace(['\n', '\r'], " ");
            (
                format!("{before}{name}{after}{section}\n"),
                InputFormat::Mdoc,
            )
        }
        MacroSet::Man => {
            let before = ".TH MANT-TABLE 1\n.SH ";
            if !charge_fragment_input(
                budget,
                before.len().saturating_add(section.len()).saturating_add(1),
            ) {
                return None;
            }
            (format!("{before}{section}\n"), InputFormat::Man)
        }
        MacroSet::None => return None,
    };
    let input_bytes = prefix
        .len()
        .saturating_add(escape_prefix.len())
        .saturating_add(source.len())
        .saturating_add(1);
    if !charge_fragment_input(budget, input_bytes) {
        return None;
    }
    Some((format!("{prefix}{escape_prefix}{source}\n"), format))
}

fn charge_fragment_input(budget: Option<&RefCell<TableRecoveryBudget>>, bytes: usize) -> bool {
    budget.is_none_or(|budget| {
        let mut budget = budget.borrow_mut();
        if bytes > MAX_FRAGMENT_BYTES {
            budget.reject_fragment();
            return false;
        }
        budget.charge_input(bytes)
    })
}

/// A declined recovery retains its scan receipt only in the speculative cell.
/// The caller must explicitly accept the cell before publishing that fact.
fn incomplete_fragment(
    source: &str,
    formatter: &crate::mandoc::formatter::FormatterState,
) -> RecoveredFragment {
    let mut font = formatter.font.clone();
    let candidate = formatter.clone();
    let (inlines, escape_scan) =
        super::font::parse_roff_text_with_scan_status(source, &mut font, true);
    candidate.execution.escape_coverage.record(escape_scan);
    RecoveredFragment {
        inlines,
        complete: false,
        formatter: candidate,
        output_text_bytes: 0,
    }
}

/// True when the visible value depends on surrounding roff execution state.
fn requires_native_evaluation(source: &str, escape: Option<u8>) -> bool {
    let Some(escape) = escape.map(char::from) else {
        return false;
    };
    let mut characters = source.chars();
    while let Some(character) = characters.next() {
        if character != escape {
            continue;
        }
        let Some(escape) = characters.next() else {
            break;
        };
        // `\\E` quotes its successor in copy mode.
        if escape == 'E' {
            continue;
        }
        if matches!(escape, '*' | 'n' | '$') {
            return true;
        }
    }
    false
}

fn lower_body(
    nodes: &[libmandoc_rs::Node],
    default_name: Option<&str>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(formatter.spacing_enabled());
    builder.escape_coverage = formatter.execution.escape_coverage.clone();
    builder.font = formatter.font.clone();
    builder.inherit_vertical_space_debt(formatter.vertical_space_debt);
    builder.inherit_zero_advance_armed(formatter.take_zero_advance_armed());
    for (index, node) in nodes.iter().enumerate() {
        if matches!(
            node.macro_token.as_ref(),
            Some(Man(ManMacro::Ur | ManMacro::Mt))
        ) {
            append_man_link(&mut builder, node, default_name, node.flags.no_fill);
        } else {
            append_inline_node_with_next(&mut builder, node, nodes.get(index + 1), default_name);
        }
    }
    // This admitted fragment owns one independent tbl cell. Its closing
    // term_newln consumes the same receipt as a paragraph drain, retaining
    // completed empty rows separately from the ordinary closing delimiter.
    let (mut inlines, _, completed_rows) = builder.take_paragraph_segment(true);
    if completed_rows > 0 {
        // A cell's split_terminator consumes the printed row's closing
        // delimiter. Completed empty rows therefore need their own ones.
        let delimiters = usize::from(completed_rows)
            + usize::from(!mant_ir::inline_plain_text(&inlines).is_empty());
        inlines.extend((0..delimiters).map(|_| Inline::line_break()));
    }
    formatter.font = builder.font.clone();
    formatter.set_spacing_enabled(builder.spacing_enabled());
    formatter.vertical_space_debt = builder.vertical_space_debt();
    formatter.inherit_zero_advance_armed(builder.take_zero_advance_armed());
    inlines
}

fn clear_synthetic_targets(node: &mut libmandoc_rs::Node) {
    // Ownership belongs to the original table cell, never this private parse.
    node.flags.deep_link_target = false;
    for child in &mut node.children {
        clear_synthetic_targets(child);
    }
}

fn inline_request(name: &str, dialect: MacroSet) -> bool {
    use crate::mandoc::controls::{OperandControl, operand_control};
    // This entry reads a source fragment rather than an owned node. Resolve
    // its spelling once before sharing the typed AST control classification.
    let token = libmandoc_rs::MacroToken::from_name(name);
    match operand_control(Some(&token)) {
        Some(OperandControl::Font | OperandControl::Presentation) => {
            return dialect != MacroSet::None;
        }
        Some(OperandControl::Spacing | OperandControl::Delimiters) => {
            return dialect == MacroSet::Mdoc;
        }
        _ => {}
    }
    match dialect {
        MacroSet::Man => matches!(
            name,
            "B" | "I"
                | "SB"
                | "SM"
                | "BI"
                | "BR"
                | "IB"
                | "IR"
                | "RB"
                | "RI"
                | "MR"
                | "OP"
                | "UR"
                | "UE"
                | "MT"
                | "ME"
        ),
        MacroSet::Mdoc => matches!(
            name,
            "Ad" | "Ap"
                | "Aq"
                | "Ar"
                | "Bo"
                | "Bc"
                | "Bq"
                | "Bro"
                | "Brc"
                | "Brq"
                | "Cd"
                | "Cm"
                | "Do"
                | "Dc"
                | "Dq"
                | "Dv"
                | "Em"
                | "Er"
                | "Ev"
                | "Fa"
                | "Fl"
                | "Fn"
                | "Ft"
                | "Ic"
                | "In"
                | "Li"
                | "Lk"
                | "Ms"
                | "Mt"
                | "Nm"
                | "No"
                | "Ns"
                | "Oo"
                | "Oc"
                | "Op"
                | "Pa"
                | "Pf"
                | "Po"
                | "Pc"
                | "Pq"
                | "Ql"
                | "Qo"
                | "Qc"
                | "Qq"
                | "Sm"
                | "So"
                | "Sc"
                | "Sq"
                | "Sx"
                | "Sy"
                | "Tn"
                | "Va"
                | "Vt"
                | "Xr"
        ),
        MacroSet::None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::inline_plain_text as plain_text;

    #[test]
    fn rejects_session_dependent_or_structural_fragments_without_dropping_their_text() {
        for source in [r".No There\*(Aqs", r".No step\n+[counter]", r".No \$1"] {
            let recovered = lower_source_fragment_with_formatter_state(
                source,
                Some(b'\\'),
                MacroSet::Mdoc,
                None,
                false,
                crate::mandoc::formatter::FormatterState::default(),
                None,
            )
            .expect("recognized bounded source");
            assert!(!recovered.complete, "{source}");
        }
        for dialect in [MacroSet::Man, MacroSet::Mdoc] {
            assert!(
                lower_source_fragment_with_formatter_state(
                    ".PP\nTOKEN",
                    Some(b'\\'),
                    dialect,
                    None,
                    false,
                    crate::mandoc::formatter::FormatterState::default(),
                    None,
                )
                .is_none()
            );
        }
    }

    #[test]
    fn admitted_fragments_preserve_mdoc_joining_and_generated_closers() {
        let recovered = lower_source_fragment_with_formatter_state(
            ".No a Oo b Ns Oc c",
            Some(b'\\'),
            MacroSet::Mdoc,
            None,
            false,
            crate::mandoc::formatter::FormatterState::default(),
            None,
        )
        .expect("admitted fragment");
        assert!(recovered.complete);
        assert_eq!(plain_text(&recovered.inlines), "a [b] c");
    }

    #[test]
    fn custom_escape_state_is_recreated_before_the_fragment() {
        let recovered = lower_source_fragment_with_formatter_state(
            ".No left@|right",
            Some(b'@'),
            MacroSet::Mdoc,
            None,
            false,
            crate::mandoc::formatter::FormatterState::default(),
            None,
        )
        .expect("admitted custom-escape fragment");
        assert!(recovered.complete);
        assert_eq!(plain_text(&recovered.inlines), "leftright");
    }
}
