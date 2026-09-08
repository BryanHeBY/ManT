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
    FontState, InlineBuilder, append_inline_node_with_next, lower_man_link,
    parse_roff_text_with_state,
};

pub(in crate::mandoc) struct RecoveredFragment {
    pub(in crate::mandoc) inlines: Vec<Inline>,
    pub(in crate::mandoc) complete: bool,
    pub(in crate::mandoc) font: FontState,
}

pub(in crate::mandoc) fn lower_source_fragment_with_font_state(
    source: &str,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
    font: FontState,
) -> Option<RecoveredFragment> {
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
        let mut font = font;
        RecoveredFragment {
            inlines: parse_roff_text_with_state(source, &mut font, true),
            complete: false,
            font,
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
    let mut font = font;
    Some(RecoveredFragment {
        inlines: lower_body(&body.children, default_name, &mut font),
        complete: true,
        font,
    })
}

#[cfg(test)]
fn lower_source_fragment(
    source: &str,
    dialect: MacroSet,
    default_name: Option<&str>,
    synopsis: bool,
) -> Option<RecoveredFragment> {
    lower_source_fragment_with_font_state(source, dialect, default_name, synopsis, FontState::new())
}

fn lower_body(
    nodes: &[libmandoc_rs::Node],
    default_name: Option<&str>,
    font: &mut FontState,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::new();
    builder.font = *font;
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
    *font = builder.font;
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
    use crate::inline::plain_text;

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
}
