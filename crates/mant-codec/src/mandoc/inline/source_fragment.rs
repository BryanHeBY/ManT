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
    Compression, DiagnosticCode, IncludePolicy, InputFormat, MacroSet, NodeKind, ParseOptions,
    Parser,
};
use mant_ir::Inline;

use super::{InlineBuilder, append_inline_node_with_next, lower_man_link};

const MAX_FRAGMENT_SOURCE_BYTES: usize = 64 * 1024;
const MAX_FRAGMENT_REQUESTS: usize = 64;
const MAX_FRAGMENT_INLINES: usize = 4_096;

/// Result of the isolated, source-backed table enhancement.
///
/// Rejection means the source requires facts owned by the original native
/// execution. Exhaustion means the closed enhancement itself hit a `ManT`
/// resource bound. Neither outcome authorizes replacing native cell content.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum FragmentEnhancement {
    Accepted(RecoveredFragment),
    Rejected,
    Exhausted,
}

/// Bounded synthetic input admitted by the closed table-enhancement language.
///
/// Preparing source grants no authority to replace native content.  The
/// staged path must execute this buffer through the native executor and make
/// its own transactional overlay decision; the legacy path may still project
/// the resulting AST until K23 switches the production backend.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum PreparedFragmentSource {
    Ready { source: String, format: InputFormat },
    Rejected,
    Exhausted,
}

/// Admit and wrap one direct native cell source without executing it.
pub(in crate::mandoc) fn prepare_fragment_source(
    source: &str,
    initial_escape: Option<u8>,
    dialect: MacroSet,
    synopsis: bool,
) -> PreparedFragmentSource {
    prepare_fragment_source_with_context(source, initial_escape, dialect, None, synopsis)
}

fn prepare_fragment_source_with_context(
    source: &str,
    initial_escape: Option<u8>,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
) -> PreparedFragmentSource {
    if let Err(failure) = validate_fragment_language(source, initial_escape, dialect) {
        return failure.preparation();
    }
    match synthetic_fragment_source(source, initial_escape, dialect, default_name, synopsis) {
        Ok((source, format)) => PreparedFragmentSource::Ready { source, format },
        Err(failure) => failure.preparation(),
    }
}

#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct RecoveredFragment {
    pub(in crate::mandoc) inlines: Vec<Inline>,
    pub(in crate::mandoc) formatter: crate::mandoc::formatter::FormatterState,
}

#[derive(Clone, Copy)]
enum EnhancementFailure {
    Rejected,
    Exhausted,
}

impl EnhancementFailure {
    const fn preparation(self) -> PreparedFragmentSource {
        match self {
            Self::Rejected => PreparedFragmentSource::Rejected,
            Self::Exhausted => PreparedFragmentSource::Exhausted,
        }
    }
}

fn validate_fragment_language(
    source: &str,
    initial_escape: Option<u8>,
    dialect: MacroSet,
) -> Result<(), EnhancementFailure> {
    if source.len() > MAX_FRAGMENT_SOURCE_BYTES {
        return Err(EnhancementFailure::Exhausted);
    }
    let mut requests = 0usize;
    for line in source.lines() {
        let Some(name) = control_line_request(line) else {
            continue;
        };
        if !inline_request(name, dialect) {
            return Err(EnhancementFailure::Rejected);
        }
        requests = requests
            .checked_add(1)
            .ok_or(EnhancementFailure::Exhausted)?;
        if requests > MAX_FRAGMENT_REQUESTS {
            return Err(EnhancementFailure::Exhausted);
        }
    }
    let has_font_escape = initial_escape != Some(0)
        && super::decode(source).iter().any(|event| {
            matches!(
                event,
                super::RoffInlineEvent::Font(_) | super::RoffInlineEvent::PreviousFont
            )
        });
    if requests == 0 && !has_font_escape {
        return Err(EnhancementFailure::Rejected);
    }
    if requires_native_evaluation(source, initial_escape) {
        return Err(EnhancementFailure::Rejected);
    }
    Ok(())
}

fn synthetic_fragment_source(
    source: &str,
    initial_escape: Option<u8>,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
) -> Result<(String, InputFormat), EnhancementFailure> {
    let escape = initial_escape.ok_or(EnhancementFailure::Rejected)?;
    let escape_prefix = match escape {
        b'\\' => String::new(),
        0 => ".eo\n".to_owned(),
        escape if escape.is_ascii_graphic() => format!(".ec {}\n", char::from(escape)),
        _ => return Err(EnhancementFailure::Rejected),
    };
    let section = if synopsis { "SYNOPSIS" } else { "DESCRIPTION" };
    let (prefix, format) = match dialect {
        MacroSet::Mdoc => {
            let raw_name = default_name.unwrap_or("table-fragment");
            let name_size = escaped_name_size(raw_name).ok_or(EnhancementFailure::Exhausted)?;
            if name_size > MAX_FRAGMENT_SOURCE_BYTES {
                return Err(EnhancementFailure::Exhausted);
            }
            let name = raw_name
                .replace('\\', "\\e")
                .replace('"', "\\(dq")
                .replace(['\n', '\r'], " ");
            (
                format!(
                    ".Dd January 1, 2000\n.Dt MANT-TABLE 1\n.Os\n.Sh NAME\n.Nm \"{name}\"\n.Nd table fragment\n.Sh {section}\n"
                ),
                InputFormat::Mdoc,
            )
        }
        MacroSet::Man => (
            format!(".TH MANT-TABLE 1 \"January 1, 2000\"\n.SH {section}\n"),
            InputFormat::Man,
        ),
        MacroSet::None => return Err(EnhancementFailure::Rejected),
    };
    let total_source = prefix
        .len()
        .checked_add(escape_prefix.len())
        .and_then(|length| length.checked_add(source.len()))
        .and_then(|length| length.checked_add(1))
        .ok_or(EnhancementFailure::Exhausted)?;
    if total_source > MAX_FRAGMENT_SOURCE_BYTES {
        return Err(EnhancementFailure::Exhausted);
    }
    Ok((format!("{prefix}{escape_prefix}{source}\n"), format))
}

/// Recover a cell only when every request belongs to the closed inline
/// language and the source does not require the original roff session.
pub(in crate::mandoc) fn enhance_source_fragment(
    source: &str,
    initial_escape: Option<u8>,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
    formatter: crate::mandoc::formatter::FormatterState,
) -> FragmentEnhancement {
    let prepared = prepare_fragment_source_with_context(
        source,
        initial_escape,
        dialect,
        default_name,
        synopsis,
    );
    let (synthetic_source, format) = match prepared {
        PreparedFragmentSource::Ready { source, format } => (source, format),
        PreparedFragmentSource::Rejected => return FragmentEnhancement::Rejected,
        PreparedFragmentSource::Exhausted => return FragmentEnhancement::Exhausted,
    };
    let parser = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .with_input_format(format);
    let Ok(parser) = parser.with_mdoc_operating_system("ManT") else {
        return FragmentEnhancement::Rejected;
    };
    let report = parser.parse_bytes("mant-table-fragment.1", synthetic_source.as_bytes());
    let Ok(mut report) = report else {
        return FragmentEnhancement::Rejected;
    };
    if !report.diagnostics.is_empty() {
        return if report.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code(),
                Some(DiagnosticCode::SyntaxTreeDepthLimit | DiagnosticCode::EquationTreeDepthLimit)
            )
        }) {
            FragmentEnhancement::Exhausted
        } else {
            FragmentEnhancement::Rejected
        };
    }
    clear_synthetic_targets(&mut report.document.root);
    let Some(section) = report.document.root.children.iter().rev().find(|node| {
        matches!(node.macro_name.as_deref(), Some("Sh" | "SH")) && node.kind == NodeKind::Block
    }) else {
        return FragmentEnhancement::Rejected;
    };
    let Some(body) = section
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)
    else {
        return FragmentEnhancement::Rejected;
    };
    let mut formatter = formatter;
    let inlines = lower_body(&body.children, default_name, &mut formatter);
    if inline_tree_size(&inlines) > MAX_FRAGMENT_INLINES {
        FragmentEnhancement::Exhausted
    } else {
        FragmentEnhancement::Accepted(RecoveredFragment { inlines, formatter })
    }
}

/// Parse one fixed-CVS control line without accepting indentation before the
/// control character. Request names terminate at either SP or TAB, exactly as
/// `roff_getcontrol()` followed by `roff_getname()` does upstream.
pub(in crate::mandoc) fn control_line_request(line: &str) -> Option<&str> {
    control_line_parts(line).map(|(name, _)| name)
}

/// Return the fixed-CVS request name and its operand text.
///
/// Keeping both slices behind one parser is important for the escaped `\.`
/// control form: `roff_getcontrol()` consumes two bytes there, while `.` and
/// `'` consume one. Consumers must not reconstruct that boundary from the
/// returned request name.
pub(in crate::mandoc) fn control_line_parts(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut start = match bytes {
        [b'.' | b'\'', ..] => 1,
        [b'\\', b'.', ..] => 2,
        _ => return None,
    };
    while matches!(bytes.get(start), Some(b' ' | b'\t')) {
        start += 1;
    }
    let end = bytes[start..]
        .iter()
        .position(|byte| matches!(byte, b' ' | b'\t'))
        .map_or(bytes.len(), |offset| start + offset);
    (start < end).then(|| {
        let operands = line[end..].trim_start_matches([' ', '\t']);
        (&line[start..end], operands)
    })
}

fn escaped_name_size(source: &str) -> Option<usize> {
    source.chars().try_fold(0usize, |length, character| {
        length.checked_add(match character {
            '\\' => 2,
            '"' => 4,
            '\n' | '\r' => 1,
            _ => character.len_utf8(),
        })
    })
}

fn inline_tree_size(nodes: &[Inline]) -> usize {
    nodes.iter().fold(0usize, |count, node| {
        let children = match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => inline_tree_size(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => 0,
        };
        count.saturating_add(1).saturating_add(children)
    })
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
    let mut builder = InlineBuilder::with_spacing(formatter.spacing);
    builder.font = formatter.font;
    builder.inherit_vertical_space_debt(formatter.vertical_space_debt);
    builder.inherit_zero_advance_armed(std::mem::take(&mut formatter.zero_advance_armed));
    for (index, node) in nodes.iter().enumerate() {
        if matches!(node.macro_name.as_deref(), Some("UR" | "MT")) {
            builder.append(lower_man_link(
                node,
                default_name,
                builder.spacing_enabled(),
            ));
        } else {
            append_inline_node_with_next(&mut builder, node, nodes.get(index + 1), default_name);
        }
    }
    formatter.font = builder.font;
    formatter.spacing = builder.spacing_enabled();
    formatter.vertical_space_debt = builder.vertical_space_debt();
    formatter.zero_advance_armed = builder.take_zero_advance_armed();
    builder.finish()
}

fn clear_synthetic_targets(node: &mut libmandoc_rs::Node) {
    // Ownership belongs to the original table cell, never this private parse.
    node.flags.deep_link_target = false;
    for child in &mut node.children {
        clear_synthetic_targets(child);
    }
}

pub(in crate::mandoc) fn inline_request(name: &str, dialect: MacroSet) -> bool {
    use crate::mandoc::controls::{OperandControl, operand_control};
    match operand_control(Some(name)) {
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
            let recovered = enhance_source_fragment(
                source,
                Some(b'\\'),
                MacroSet::Mdoc,
                None,
                false,
                crate::mandoc::formatter::FormatterState::default(),
            );
            assert_eq!(recovered, FragmentEnhancement::Rejected, "{source}");
        }
        for dialect in [MacroSet::Man, MacroSet::Mdoc] {
            assert_eq!(
                enhance_source_fragment(
                    ".PP\nTOKEN",
                    Some(b'\\'),
                    dialect,
                    None,
                    false,
                    crate::mandoc::formatter::FormatterState::default(),
                ),
                FragmentEnhancement::Rejected,
            );
        }
    }

    #[test]
    fn admitted_fragments_preserve_mdoc_joining_and_generated_closers() {
        let FragmentEnhancement::Accepted(recovered) = enhance_source_fragment(
            ".No a Oo b Ns Oc c",
            Some(b'\\'),
            MacroSet::Mdoc,
            None,
            false,
            crate::mandoc::formatter::FormatterState::default(),
        ) else {
            panic!("admitted fragment must be accepted");
        };
        assert_eq!(plain_text(&recovered.inlines), "a [b] c");
    }

    #[test]
    fn custom_escape_state_is_recreated_before_the_fragment() {
        let FragmentEnhancement::Accepted(recovered) = enhance_source_fragment(
            ".No left@|right",
            Some(b'@'),
            MacroSet::Mdoc,
            None,
            false,
            crate::mandoc::formatter::FormatterState::default(),
        ) else {
            panic!("admitted custom-escape fragment must be accepted");
        };
        assert_eq!(plain_text(&recovered.inlines), "leftright");
    }

    #[test]
    fn enhancement_budgets_are_distinct_from_semantic_rejection() {
        // These are ManT resource-policy assertions, not claims about CVS
        // rendering. Their purpose is to keep speculative enhancement
        // exhaustion distinguishable from a source that requires the native
        // document session.
        assert_eq!(
            enhance_source_fragment(
                &"x".repeat(MAX_FRAGMENT_SOURCE_BYTES + 1),
                Some(b'\\'),
                MacroSet::Mdoc,
                None,
                false,
                crate::mandoc::formatter::FormatterState::default(),
            ),
            FragmentEnhancement::Exhausted,
        );
        let requests = ".No x\n".repeat(MAX_FRAGMENT_REQUESTS + 1);
        assert_eq!(
            enhance_source_fragment(
                &requests,
                Some(b'\\'),
                MacroSet::Mdoc,
                None,
                false,
                crate::mandoc::formatter::FormatterState::default(),
            ),
            FragmentEnhancement::Exhausted,
        );
    }

    #[test]
    fn control_line_lexer_matches_fixed_cvs_name_boundaries() {
        // `roff_getcontrol()` requires the control character at the current
        // input position; `roff_getname()` terminates on either SP or TAB.
        assert_eq!(control_line_request(".Fl\thelp"), Some("Fl"));
        assert_eq!(control_line_request("'  No value"), Some("No"));
        assert_eq!(control_line_request("\\.br"), Some("br"));
        assert_eq!(control_line_request(" .Fl help"), None);
        assert_eq!(control_line_request("plain"), None);
    }

    #[test]
    fn escaped_control_line_operands_start_after_both_prefix_bytes() {
        // Fixed CVS `roff_getcontrol()` consumes both bytes of `\.` before
        // `roff_getname()` and the request operand parser run.
        assert_eq!(
            control_line_parts(r"\.BR linked (3)"),
            Some(("BR", "linked (3)"))
        );
    }

    #[test]
    fn direct_man_alternating_font_macro_is_a_complete_isolated_overlay() {
        // The fixed CVS reference renders this direct T{} operand as
        // `linked (3)` because tbl owns the executed text. Isolated parsing
        // is permitted only to restore `.BR` presentation, yielding the
        // adjacent `linked(3)` overlay without changing native ownership.
        let PreparedFragmentSource::Ready { source, format } =
            prepare_fragment_source(".BR linked (3)", Some(b'\\'), MacroSet::Man, false)
        else {
            panic!("direct .BR fragment must be eligible for enrichment");
        };
        assert_eq!(format, InputFormat::Man);
        assert!(source.ends_with(".BR linked (3)\n"));
    }

    #[test]
    fn inline_budget_counts_nested_children_not_only_roots() {
        let nested = vec![Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "value".to_owned(),
                }],
            }],
        }];
        assert_eq!(inline_tree_size(&nested), 3);
    }
}
