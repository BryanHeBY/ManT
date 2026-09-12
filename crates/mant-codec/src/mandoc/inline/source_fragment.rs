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
    Compression, IncludePolicy, InputFormat, MacroSet, NodeKind, ParseOptions, Parser,
};
use mant_ir::Inline;

use super::{
    InlineBuilder, append_inline_node_with_next, lower_man_link, parse_roff_text_with_state,
};

#[must_use]
pub(in crate::mandoc) struct RecoveredFragment {
    pub(in crate::mandoc) inlines: Vec<Inline>,
    pub(in crate::mandoc) complete: bool,
    pub(in crate::mandoc) formatter: crate::mandoc::formatter::FormatterState,
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
) -> Option<RecoveredFragment> {
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
        let mut font = formatter.font;
        RecoveredFragment {
            inlines: parse_roff_text_with_state(source, &mut font, true),
            complete: false,
            formatter,
        }
    };
    // A separate parser invocation is intentionally small and finite.  More
    // importantly, strings, registers and macro arguments would otherwise be
    // evaluated against the synthetic document rather than the real session.
    if requests > 64 || requires_native_evaluation(source, initial_escape) {
        return Some(fallback());
    }

    // A tbl row always records an executed escape state. Do not invent `.eo`
    // for a synthetic or incomplete caller that lacks this fact.
    let Some(escape) = initial_escape else {
        return Some(fallback());
    };
    let escape_prefix = match escape {
        b'\\' => String::new(),
        0 => ".eo\n".to_owned(),
        escape if escape.is_ascii_graphic() => {
            format!(".ec {}\n", char::from(escape))
        }
        // libmandoc stores `.ec` as one byte. A non-ASCII byte cannot be
        // faithfully reconstructed as a Rust source character here.
        _ => return Some(fallback()),
    };

    let section = if synopsis { "SYNOPSIS" } else { "DESCRIPTION" };
    let (prefix, format) = match dialect {
        MacroSet::Mdoc => {
            let name = default_name
                .unwrap_or("table-fragment")
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
            format!(".TH MANT-TABLE 1\n.SH {section}\n"),
            InputFormat::Man,
        ),
        MacroSet::None => return None,
    };
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .with_input_format(format)
    .parse_bytes(
        "mant-table-fragment.1",
        format!("{prefix}{escape_prefix}{source}\n").as_bytes(),
    );
    let Ok(mut report) = report else {
        return Some(fallback());
    };
    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code().is_some())
    {
        return Some(fallback());
    }
    clear_synthetic_targets(&mut report.document.root);
    let section = report.document.root.children.iter().rev().find(|node| {
        matches!(node.macro_name.as_deref(), Some("Sh" | "SH")) && node.kind == NodeKind::Block
    })?;
    let body = section
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)?;
    let mut formatter = formatter;
    let inlines = lower_body(&body.children, default_name, &mut formatter);
    Some(RecoveredFragment {
        inlines,
        complete: true,
        formatter,
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
    builder.finish()
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
            let recovered = lower_source_fragment_with_formatter_state(
                source,
                Some(b'\\'),
                MacroSet::Mdoc,
                None,
                false,
                crate::mandoc::formatter::FormatterState::default(),
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
        )
        .expect("admitted custom-escape fragment");
        assert!(recovered.complete);
        assert_eq!(plain_text(&recovered.inlines), "leftright");
    }
}
