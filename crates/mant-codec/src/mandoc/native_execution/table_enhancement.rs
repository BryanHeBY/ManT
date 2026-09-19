//! Transactional, cell-local semantic enrichment of native tbl content.
//!
//! The fixed-CVS parser and renderer remain authoritative for ownership,
//! content, and formatter state. This module executes an admitted fragment in
//! a fresh, bounded native session and may replace only the semantic IR payload
//! of one already completed cell. It has no resolver, filesystem, mutable
//! document session, or legacy Rust formatter access.

use std::collections::BTreeSet;

use libmandoc_rs::{
    AtomDisposition, AtomKind, AtomRole, Compression, DiagnosticCode, ExecutionAtom,
    ExecutionErrorKind, ExecutionFont, ExecutionLimits, ExecutionNodeKey, ExecutionReferenceKind,
    ExecutionTableCellFlags, ExecutionTableCellKey, IncludePolicy, MacroSet, Node, NodeKind,
    ParseOptions, Parser,
};
use mant_ir::{Inline, LinkTarget};

use crate::mandoc::inline::{PreparedFragmentSource, inline_request, prepare_fragment_source};

const MAX_OVERLAY_INLINES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum RejectReason {
    NotTextCell,
    NotTextBlock,
    UnsafeExecutionSource,
    MissingSource,
    MissingEscapeState,
    SessionDependent,
    EmptyOverlay,
    OwnerMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum LimitKind {
    SourceOrSyntax,
    NativeExecution,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct CellOverlay {
    pub(super) cell: ExecutionTableCellKey,
    pub(super) content: Vec<Inline>,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum EnhancementDecision {
    Accepted(CellOverlay),
    Rejected(RejectReason),
    BudgetExhausted(LimitKind),
}

pub(super) struct CellEnhancementInput<'a> {
    pub(super) cell: ExecutionTableCellKey,
    pub(super) kind: mant_ir::TableCellKind,
    pub(super) flags: ExecutionTableCellFlags,
    pub(super) native_operand: Option<&'a str>,
    pub(super) native_content: &'a [Inline],
    pub(super) source: Option<&'a str>,
    pub(super) escape: Option<u8>,
    pub(super) macro_set: MacroSet,
    pub(super) synopsis: bool,
}

pub(super) fn analyze(input: &CellEnhancementInput<'_>) -> EnhancementDecision {
    if input.kind != mant_ir::TableCellKind::Text {
        return EnhancementDecision::Rejected(RejectReason::NotTextCell);
    }
    if !input.flags.contains(ExecutionTableCellFlags::TEXT_BLOCK) {
        return EnhancementDecision::Rejected(RejectReason::NotTextBlock);
    }
    if !input
        .flags
        .contains(ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE)
        || input
            .flags
            .contains(ExecutionTableCellFlags::VERTICAL_CONTINUATION)
    {
        return EnhancementDecision::Rejected(RejectReason::UnsafeExecutionSource);
    }
    let Some(source) = input.source else {
        return EnhancementDecision::Rejected(RejectReason::MissingSource);
    };
    if input.escape.is_none() {
        return EnhancementDecision::Rejected(RejectReason::MissingEscapeState);
    }

    let prepared = prepare_fragment_source(source, input.escape, input.macro_set, input.synopsis);
    let (source, format) = match prepared {
        PreparedFragmentSource::Ready { source, format } => (source, format),
        PreparedFragmentSource::Rejected => {
            return EnhancementDecision::Rejected(RejectReason::SessionDependent);
        }
        PreparedFragmentSource::Exhausted => {
            return EnhancementDecision::BudgetExhausted(LimitKind::SourceOrSyntax);
        }
    };
    let parser = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .with_input_format(format);
    let Ok(parser) = parser.with_mdoc_operating_system("ManT") else {
        return EnhancementDecision::Rejected(RejectReason::SessionDependent);
    };
    let report = match parser.execute_bytes(
        "mant-table-fragment.1",
        source.as_bytes(),
        fragment_execution_limits(),
    ) {
        Ok(report) => report,
        Err(error)
            if matches!(
                error.kind,
                ExecutionErrorKind::Budget
                    | ExecutionErrorKind::Allocation
                    | ExecutionErrorKind::Cancelled
            ) =>
        {
            return EnhancementDecision::BudgetExhausted(LimitKind::NativeExecution);
        }
        Err(_) => return EnhancementDecision::Rejected(RejectReason::SessionDependent),
    };
    if report.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code(),
            Some(DiagnosticCode::SyntaxTreeDepthLimit | DiagnosticCode::EquationTreeDepthLimit)
        )
    }) {
        return EnhancementDecision::BudgetExhausted(LimitKind::NativeExecution);
    }
    if !report.diagnostics.is_empty() {
        return EnhancementDecision::Rejected(RejectReason::SessionDependent);
    }

    let Some(body) = synthetic_section_body(&report.document.root) else {
        return EnhancementDecision::Rejected(RejectReason::SessionDependent);
    };
    let mut nodes = BTreeSet::new();
    if let Some(key) = body.execution_node_key {
        nodes.insert(ExecutionNodeKey(key));
    }
    if !body
        .children
        .iter()
        .all(|node| collect_closed_nodes(node, input.macro_set, &mut nodes))
    {
        return EnhancementDecision::Rejected(RejectReason::SessionDependent);
    }
    let Some(content) = project_body(&report.execution, &nodes) else {
        return EnhancementDecision::Rejected(RejectReason::SessionDependent);
    };
    if inline_tree_size(&content) > MAX_OVERLAY_INLINES {
        return EnhancementDecision::BudgetExhausted(LimitKind::SourceOrSyntax);
    }
    if content.is_empty()
        && (input.native_operand.is_some_and(|value| !value.is_empty())
            || !input.native_content.is_empty())
    {
        EnhancementDecision::Rejected(RejectReason::EmptyOverlay)
    } else {
        EnhancementDecision::Accepted(CellOverlay {
            cell: input.cell,
            content,
        })
    }
}

fn fragment_execution_limits() -> ExecutionLimits {
    ExecutionLimits {
        max_nodes: 8_192,
        max_depth: 128,
        max_work: 1_000_000,
        max_records: 250_000,
        max_pool_bytes: 512 * 1024,
        max_buffer_cells: 64 * 1024,
        max_report_bytes: 32 * 1024 * 1024,
    }
}

fn synthetic_section_body(root: &Node) -> Option<&Node> {
    root.children
        .iter()
        .rev()
        .find(|node| {
            node.kind == NodeKind::Block && matches!(node.macro_name.as_deref(), Some("Sh" | "SH"))
        })?
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)
}

fn collect_closed_nodes(
    node: &Node,
    macro_set: MacroSet,
    output: &mut BTreeSet<ExecutionNodeKey>,
) -> bool {
    if let Some(name) = node.macro_name.as_deref()
        && (matches!(name, "Nm" | "Sx") || !inline_request(name, macro_set))
    {
        return false;
    }
    if let Some(key) = node.execution_node_key {
        output.insert(ExecutionNodeKey(key));
    }
    node.children
        .iter()
        .all(|child| collect_closed_nodes(child, macro_set, output))
}

fn flush_overlay_run(
    output: &mut Vec<Inline>,
    linked: &mut Vec<Inline>,
    active_reference: Option<u32>,
    buffer: &mut String,
    font: &mut Option<ExecutionFont>,
) {
    if let Some(font) = font.take()
        && !buffer.is_empty()
    {
        let styled = super::styled_table_text(std::mem::take(buffer), font);
        push_overlay_inline(output, linked, active_reference, styled);
    }
}

fn push_overlay_inline(
    output: &mut Vec<Inline>,
    linked: &mut Vec<Inline>,
    active_reference: Option<u32>,
    inline: Inline,
) {
    if active_reference.is_some() {
        linked.push(inline);
    } else {
        output.push(inline);
    }
}

fn overlay_character(atom: &ExecutionAtom) -> Option<char> {
    match atom.kind {
        AtomKind::Glyph if atom.disposition == AtomDisposition::Emitted => {
            char::from_u32(atom.display_scalar)
        }
        AtomKind::BreakableSpace
            if matches!(
                atom.disposition,
                AtomDisposition::Emitted | AtomDisposition::Consumed
            ) =>
        {
            Some(' ')
        }
        AtomKind::NonBreakingSpace
            if matches!(
                atom.disposition,
                AtomDisposition::Emitted | AtomDisposition::Consumed
            ) =>
        {
            Some('\u{a0}')
        }
        AtomKind::Tab
            if matches!(
                atom.disposition,
                AtomDisposition::Emitted | AtomDisposition::Consumed
            ) =>
        {
            Some('\t')
        }
        AtomKind::Glyph
        | AtomKind::BreakableSpace
        | AtomKind::NonBreakingSpace
        | AtomKind::BreakableHyphen
        | AtomKind::ZeroWidth
        | AtomKind::Tab
        | AtomKind::TabReference
        | AtomKind::Backspace
        | AtomKind::BreakPoint => None,
        AtomKind::WordEndBreak => unreachable!("handled before glyph projection"),
    }
}

fn project_body(
    report: &libmandoc_rs::NativeExecutionReport,
    body_nodes: &BTreeSet<ExecutionNodeKey>,
) -> Option<Vec<Inline>> {
    let atom_references = super::atom_reference_owners(report);
    let mut output = Vec::new();
    let mut active_reference = None;
    let mut linked = Vec::new();
    let mut buffer = String::new();
    let mut font = None;
    let mut consume_break_space = false;

    for (index, atom) in report.atoms().iter().enumerate() {
        let Some(node) = atom.node else {
            continue;
        };
        if !body_nodes.contains(&node)
            || matches!(
                atom.role,
                AtomRole::FontDecoration | AtomRole::DeviceGenerated | AtomRole::EquationContent
            )
        {
            continue;
        }

        let reference = atom_references[index];
        if active_reference != reference {
            flush_overlay_run(
                &mut output,
                &mut linked,
                active_reference,
                &mut buffer,
                &mut font,
            );
            flush_link(report, &mut output, &mut linked, &mut active_reference)?;
            active_reference = reference;
        }
        if atom.kind == AtomKind::WordEndBreak {
            flush_overlay_run(
                &mut output,
                &mut linked,
                active_reference,
                &mut buffer,
                &mut font,
            );
            push_overlay_inline(
                &mut output,
                &mut linked,
                active_reference,
                Inline::LineBreak,
            );
            consume_break_space = true;
            continue;
        }
        if consume_break_space
            && atom.kind == AtomKind::BreakableSpace
            && atom.disposition == AtomDisposition::Consumed
        {
            consume_break_space = false;
            continue;
        }
        consume_break_space = false;
        let character = overlay_character(atom);
        let Some(character) = character else {
            continue;
        };
        if font != Some(atom.font) {
            flush_overlay_run(
                &mut output,
                &mut linked,
                active_reference,
                &mut buffer,
                &mut font,
            );
            font = Some(atom.font);
        }
        buffer.push(character);
    }
    flush_overlay_run(
        &mut output,
        &mut linked,
        active_reference,
        &mut buffer,
        &mut font,
    );
    flush_link(report, &mut output, &mut linked, &mut active_reference)?;
    Some(output)
}

fn flush_link(
    report: &libmandoc_rs::NativeExecutionReport,
    output: &mut Vec<Inline>,
    children: &mut Vec<Inline>,
    active: &mut Option<u32>,
) -> Option<()> {
    let Some(key) = active.take() else {
        return Some(());
    };
    let reference = report.references().get(usize::try_from(key).ok()?)?;
    if reference.key != key || children.is_empty() {
        return None;
    }
    let primary = String::from_utf8_lossy(report.pool_bytes(reference.primary)?).into_owned();
    let target = match reference.kind {
        ExecutionReferenceKind::ExternalUri => LinkTarget::External { uri: primary },
        ExecutionReferenceKind::Email => LinkTarget::Email { address: primary },
        ExecutionReferenceKind::Manual => LinkTarget::Manual {
            name: primary,
            manual_section: match reference.secondary {
                Some(secondary) => {
                    Some(String::from_utf8_lossy(report.pool_bytes(secondary)?).into_owned())
                }
                None => None,
            },
        },
        ExecutionReferenceKind::SameDocumentSection => return None,
    };
    output.push(Inline::Link {
        target,
        title: None,
        children: std::mem::take(children),
    });
    Some(())
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

pub(super) fn commit(
    owner: ExecutionTableCellKey,
    native_content: Vec<Inline>,
    decision: &mut EnhancementDecision,
) -> Vec<Inline> {
    match decision {
        EnhancementDecision::Accepted(overlay) if overlay.cell == owner => overlay.content.clone(),
        EnhancementDecision::Accepted(_) => {
            *decision = EnhancementDecision::Rejected(RejectReason::OwnerMismatch);
            native_content
        }
        EnhancementDecision::Rejected(_) | EnhancementDecision::BudgetExhausted(_) => {
            native_content
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::inline_plain_text as plain_text;

    fn input(key: u32, source: Option<&str>, flags: u32) -> CellEnhancementInput<'_> {
        CellEnhancementInput {
            cell: ExecutionTableCellKey(key),
            kind: mant_ir::TableCellKind::Text,
            flags: ExecutionTableCellFlags(flags),
            native_operand: Some("Fl help"),
            native_content: &[],
            source,
            escape: Some(b'\\'),
            macro_set: MacroSet::Mdoc,
            synopsis: false,
        }
    }

    #[test]
    fn accepted_rejected_and_exhausted_are_transactional() {
        // Fixed CVS sends the direct `.Fl` operand `Fl help` to tbl. The
        // bounded native overlay may add only its typed mdoc presentation.
        let flags =
            ExecutionTableCellFlags::TEXT_BLOCK | ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE;
        let accepted = analyze(&input(3, Some(".Fl Fl help"), flags));
        assert!(
            matches!(
                accepted,
                EnhancementDecision::Accepted(CellOverlay {
                    cell: ExecutionTableCellKey(3),
                    ..
                })
            ),
            "{accepted:?}"
        );
        assert_eq!(
            analyze(&input(
                3,
                Some(".Fl Fl help"),
                ExecutionTableCellFlags::TEXT_BLOCK
            )),
            EnhancementDecision::Rejected(RejectReason::UnsafeExecutionSource)
        );
        assert_eq!(
            analyze(&input(3, Some(&"x".repeat(65_537)), flags)),
            EnhancementDecision::BudgetExhausted(LimitKind::SourceOrSyntax)
        );
    }

    #[test]
    fn owner_mismatch_preserves_native_payload_and_accepted_audit_data() {
        let native = vec![Inline::Text {
            value: "native".to_owned(),
        }];
        let mut accepted = EnhancementDecision::Accepted(CellOverlay {
            cell: ExecutionTableCellKey(8),
            content: vec![Inline::Text {
                value: "overlay".to_owned(),
            }],
        });
        assert_eq!(
            commit(ExecutionTableCellKey(8), native.clone(), &mut accepted),
            vec![Inline::Text {
                value: "overlay".to_owned(),
            }]
        );
        assert!(matches!(
            accepted,
            EnhancementDecision::Accepted(CellOverlay { content, .. }) if !content.is_empty()
        ));

        let mut mismatch = EnhancementDecision::Accepted(CellOverlay {
            cell: ExecutionTableCellKey(8),
            content: vec![Inline::Text {
                value: "overlay".to_owned(),
            }],
        });
        assert_eq!(
            commit(ExecutionTableCellKey(7), native.clone(), &mut mismatch),
            native
        );
        assert_eq!(
            mismatch,
            EnhancementDecision::Rejected(RejectReason::OwnerMismatch)
        );
    }

    #[test]
    fn structural_and_document_state_inputs_never_replace_native_cells() {
        let flags =
            ExecutionTableCellFlags::TEXT_BLOCK | ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE;
        for source in [".PP\nBODY", ".Nm", ".Sx OTHER", ".No Nm", ".No Sx NAME"] {
            assert!(matches!(
                analyze(&input(1, Some(source), flags)),
                EnhancementDecision::Rejected(RejectReason::SessionDependent)
            ));
        }
        let mut rule = input(1, Some(".Fl Fl help"), flags);
        rule.kind = mant_ir::TableCellKind::HorizontalRule;
        assert_eq!(
            analyze(&rule),
            EnhancementDecision::Rejected(RejectReason::NotTextCell)
        );

        let mut vertical = input(1, Some(".Fl Fl help"), flags);
        vertical.flags.0 |= ExecutionTableCellFlags::VERTICAL_CONTINUATION;
        assert_eq!(
            analyze(&vertical),
            EnhancementDecision::Rejected(RejectReason::UnsafeExecutionSource)
        );
    }

    #[test]
    fn direct_man_font_overlay_uses_native_execution_not_legacy_formatter() {
        let flags =
            ExecutionTableCellFlags::TEXT_BLOCK | ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE;
        let mut request = input(5, Some(".BR linked (3)"), flags);
        request.macro_set = MacroSet::Man;
        let decision = analyze(&request);
        let EnhancementDecision::Accepted(overlay) = decision else {
            panic!("direct .BR must be an admitted isolated overlay: {decision:?}");
        };
        assert_eq!(plain_text(&overlay.content), "linked(3)");

        // Fixed CVS `roff_getcontrol()` consumes the complete escaped `\.`
        // prefix before dispatching BR, so the staged end-to-end path must
        // neither reject the request nor retain a leading dot in its operand.
        request.source = Some(r"\.BR linked (3)");
        let EnhancementDecision::Accepted(escaped) = analyze(&request) else {
            panic!("escaped direct .BR must remain an admitted overlay");
        };
        assert_eq!(plain_text(&escaped.content), "linked(3)");
    }

    #[test]
    fn synopsis_flag_selects_the_fixed_cvs_include_execution() {
        // Verified first with `target/mandoc-migration/reference/mandoc`:
        // `mdoc_term.c::termp_in_pre()` emits `#include <stdio.h>` only when
        // NODE_SYNPRETTY and NODE_LINE are both active; ordinary prose emits
        // `<stdio.h>`. The fact is supplied by the owning native table node.
        let flags =
            ExecutionTableCellFlags::TEXT_BLOCK | ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE;
        let mut prose = input(6, Some(".In stdio.h"), flags);
        prose.native_operand = Some("stdio.h");
        let EnhancementDecision::Accepted(prose_overlay) = analyze(&prose) else {
            panic!("ordinary .In overlay must be accepted");
        };
        assert_eq!(plain_text(&prose_overlay.content), "<stdio.h>");

        let mut synopsis = prose;
        synopsis.synopsis = true;
        let EnhancementDecision::Accepted(synopsis_overlay) = analyze(&synopsis) else {
            panic!("synopsis .In overlay must be accepted");
        };
        assert_eq!(plain_text(&synopsis_overlay.content), "#include <stdio.h>");
    }

    #[test]
    fn automatic_device_wraps_remain_soft_but_word_end_breaks_remain_explicit() {
        // Both fragments were run with the pinned CVS reference first.
        // `term_fill()` wraps the nine long words onto three device rows, but
        // those rows are width-dependent. In contrast, ESCAPE_BREAK stores a
        // WordEndBreak atom and breaks before `AFTER` at the formatter-word
        // boundary regardless of device width.
        let flags =
            ExecutionTableCellFlags::TEXT_BLOCK | ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE;
        let long = "0123456789 1123456789 2123456789 3123456789 4123456789 5123456789 6123456789 7123456789 8123456789";
        let wrapped_source = format!(".Em {long}");
        let mut wrapped = input(7, Some(&wrapped_source), flags);
        wrapped.native_operand = Some(long);
        let EnhancementDecision::Accepted(wrapped) = analyze(&wrapped) else {
            panic!("long direct inline macro must remain eligible");
        };
        assert_eq!(plain_text(&wrapped.content), long);
        assert!(
            !wrapped
                .content
                .iter()
                .any(|inline| matches!(inline, Inline::LineBreak)),
            "device-width wrapping must not become semantic line breaks"
        );

        let mut explicit = input(8, Some(r".Em BEFORE\p AFTER"), flags);
        explicit.native_operand = Some(r"BEFORE\p AFTER");
        let EnhancementDecision::Accepted(explicit) = analyze(&explicit) else {
            panic!("explicit word-end break must remain eligible");
        };
        assert!(
            explicit
                .content
                .iter()
                .any(|inline| matches!(inline, Inline::LineBreak)),
            "fixed-CVS word-end breaks remain explicit IR boundaries"
        );

        // Fixed CVS `termp_lk_pre()` opens one reference around the complete
        // description before terminal width can split it across device rows.
        // A semantic overlay must preserve that single typed link.
        let link_source = format!(".Lk https://example.org {long}");
        let mut link = input(9, Some(&link_source), flags);
        link.native_operand = Some(&link_source[4..]);
        let EnhancementDecision::Accepted(link) = analyze(&link) else {
            panic!("long direct link must remain eligible");
        };
        assert_eq!(
            link.content
                .iter()
                .filter(|inline| matches!(inline, Inline::Link { .. }))
                .count(),
            1,
            "automatic device wrapping cannot split one native reference"
        );
        assert!(
            !link
                .content
                .iter()
                .any(|inline| matches!(inline, Inline::LineBreak)),
            "a wrapped link label remains responsive"
        );
    }
}
