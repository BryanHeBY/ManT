//! Reuse native macro parsing for the inline-only language embedded in tbl.
//!
//! A cell is one parsing unit: callable nesting, explicit enclosure closers,
//! punctuation and Sm state must survive line boundaries. Only inline requests
//! are admitted; this is not a second document/include or table frontend.

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
    /// Whether a complete fragment still needs the original parser session
    /// for its visible text.  `roff_expand()` resolves strings, number
    /// registers, and macro arguments before tbl records a cell; a synthetic
    /// parser cannot reproduce that document-local state.
    pub(in crate::mandoc) content_authority: FragmentContentAuthority,
    pub(in crate::mandoc) formatter: crate::mandoc::formatter::FormatterState,
}

/// Select the source of visible cell text after bounded source recovery.
///
/// A synthetic parse is authoritative for self-contained inline syntax such
/// as `.Fl`, `.Ns`, and enclosure macros: libmandoc's flattened tbl payload
/// has already lost that structure.  Dynamic roff interpolation is resolved
/// by the original parse session, so the native cell text remains authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum FragmentContentAuthority {
    RecoveredSyntax,
    NativeEvaluation,
}

pub(in crate::mandoc) fn lower_source_fragment_with_formatter_state(
    source: &str,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
    formatter: crate::mandoc::formatter::FormatterState,
) -> Option<RecoveredFragment> {
    let content_authority = if requires_native_evaluation(source) {
        FragmentContentAuthority::NativeEvaluation
    } else {
        FragmentContentAuthority::RecoveredSyntax
    };
    let mut requests = 0;
    for line in source.lines() {
        if let Some(request) = line.trim_start().strip_prefix(['.', '\'']) {
            let name = request.split_whitespace().next().unwrap_or_default();
            if !inline_request(name, dialect) {
                return None;
            }
            requests += 1;
        }
    }
    if requests == 0
        && !super::decode(source).iter().any(|event| {
            matches!(
                event,
                super::RoffInlineEvent::Font(_) | super::RoffInlineEvent::PreviousFont
            )
        })
    {
        return None;
    }
    // Bound extra parsing work and nesting before entering the native parser.
    // On exhaustion retain the entire source spelling, including tail tokens.
    let fallback = || {
        let mut font = formatter.font;
        RecoveredFragment {
            inlines: parse_roff_text_with_state(source, &mut font, true),
            complete: false,
            content_authority,
            formatter,
        }
    };
    if requests > 64
        || source
            .split_whitespace()
            .filter(|word| inline_request(word, dialect))
            .count()
            > 64
    {
        return Some(fallback());
    }
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
        format!("{prefix}{source}\n").as_bytes(),
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
        content_authority,
        formatter,
    })
}

/// Return whether the source contains an interpolation whose visible value is
/// defined by the surrounding roff execution session.
///
/// The lexer deliberately recognizes only the expansion families handled by
/// CVS mandoc's `roff_expand()`: strings (`\\*`), numeric registers (`\\n`),
/// and macro arguments (`\\$`).  Those branches read the document's string
/// table, register table, or active macro invocation respectively.  Fixed
/// glyphs, font changes, and zero-width hints remain self-contained and
/// therefore keep structured source recovery.  In particular, `\\g` is not
/// expanded by mandoc and `\\V` is retained as unsupported syntax, so neither
/// makes the original parser session content authority.
fn requires_native_evaluation(source: &str) -> bool {
    let mut characters = source.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            continue;
        }
        let Some(escape) = characters.next() else {
            break;
        };
        // `\\E` is a copy-mode literal escape.  It deliberately prevents the
        // following trigger from being interpreted as an interpolation here.
        if escape == 'E' {
            continue;
        }
        if matches!(escape, '*' | 'n' | '$') {
            return true;
        }
    }
    false
}

#[cfg(test)]
fn lower_source_fragment(
    source: &str,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
) -> Option<RecoveredFragment> {
    lower_source_fragment_with_formatter_state(
        source,
        dialect,
        default_name,
        synopsis,
        crate::mandoc::formatter::FormatterState::default(),
    )
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
    // The cell's target ownership belongs to the original document, not this
    // private parse with synthetic source coordinates.
    node.flags.deep_link_target = false;
    for child in &mut node.children {
        clear_synthetic_targets(child);
    }
}

fn inline_request(name: &str, dialect: MacroSet) -> bool {
    // These controls neither add document structure nor own text. Reuse the
    // ordinary control dispatcher rather than recovering their operands as
    // prose. Fill, indentation and paragraph requests stay out of the bounded
    // inline-only language, as do payload-bearing ce/rj.
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
                | "br"
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
                | "br"
        ),
        MacroSet::None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::inline_plain_text as plain_text;

    #[test]
    fn bounded_recovery_retains_unparsed_tail_and_never_accepts_include_requests() {
        let source = format!(".Op {}tail-marker", "Op ".repeat(2_000));
        let recovered = lower_source_fragment(&source, MacroSet::Mdoc, None, false).unwrap();
        assert!(!recovered.complete);
        assert!(plain_text(&recovered.inlines).contains("tail-marker"));
        for dialect in [MacroSet::Man, MacroSet::Mdoc] {
            assert!(lower_source_fragment(".so external.1", dialect, None, false).is_none());
            assert!(lower_source_fragment(".TS\nl.\ntext\n.TE", dialect, None, false).is_none());
        }
    }

    #[test]
    fn content_authority_distinguishes_inline_syntax_from_session_expansion() {
        let recovered = lower_source_fragment(".Fl Fl help", MacroSet::Mdoc, None, false)
            .expect("recover self-contained mdoc syntax");
        assert_eq!(
            recovered.content_authority,
            FragmentContentAuthority::RecoveredSyntax
        );
        for source in [r".No There\*(Aqs", r".No step\n+[counter]", r".No \$1"] {
            let recovered = lower_source_fragment(source, MacroSet::Mdoc, None, false)
                .expect("recover bounded source fragment");
            assert_eq!(
                recovered.content_authority,
                FragmentContentAuthority::NativeEvaluation,
                "{source}"
            );
        }
        let recovered = lower_source_fragment(r".No fixed\(aqglyph", MacroSet::Mdoc, None, false)
            .expect("recover fixed glyph");
        assert_eq!(
            recovered.content_authority,
            FragmentContentAuthority::RecoveredSyntax
        );
    }
}
