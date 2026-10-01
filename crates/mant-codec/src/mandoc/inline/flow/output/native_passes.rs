//! Project accepted native field passes without executing formatter state.

use super::super::Inline;
use super::split::{advance_boundary, split_text_at_boundaries};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct OwnerAcceptance {
    pub(in crate::mandoc::inline::flow) scalars: usize,
    pub(in crate::mandoc::inline::flow) native_cells: bool,
}

/// Accepted projection range of each stable source word. A delayed glyph
/// can re-enter that owner after a later word marker without becoming its
/// rejected suffix. Native cell intervals, not append order, decide limits.
pub(in crate::mandoc::inline::flow) fn accepted_owner_lengths(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[(usize, String, usize)],
    passes: &[super::super::field_buffer::FillPass],
) -> BTreeMap<String, OwnerAcceptance> {
    let mut lengths = BTreeMap::new();
    let mut pass_index = 0;
    let mut pass_start = 0;
    for (index, (_, owner, content)) in anchors.iter().enumerate() {
        #[cfg(test)]
        OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
        // BACKBEFORE may replace the previous trailing blank before this
        // word's original buffer end. Its landing, not that old end, owns
        // the replacement graph (term.c:901-908).
        let end = anchors
            .get(index + 1)
            .map_or(buffer.cells().len(), |next| next.0.min(next.2));
        if end <= *content {
            // An empty word can be followed by BACKBEFORE replacing its
            // trailing native blank. Its zero-length projection interval
            // cannot consume a pass needed by that replacement's owner.
            lengths.insert(owner.clone(), OwnerAcceptance::default());
            continue;
        }
        while passes
            .get(pass_index)
            .is_some_and(|pass| pass.end <= *content)
        {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            pass_start = next_native_pass_start(buffer, passes[pass_index].end);
            pass_index += 1;
        }
        let mut acceptance = OwnerAcceptance::default();
        while let Some(pass) = passes.get(pass_index) {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            let start = (*content).max(pass_start);
            let accepted_end = end.min(pass.end);
            if start < accepted_end {
                acceptance.scalars += buffer.projection_length(start, accepted_end);
                acceptance.native_cells = true;
            }
            if pass.end >= end {
                break;
            }
            pass_start = next_native_pass_start(buffer, pass.end);
            pass_index += 1;
        }
        lengths.insert(owner.clone(), acceptance);
    }
    lengths
}

fn next_native_pass_start(
    buffer: &super::super::field_buffer::FieldBuffer,
    mut cell: usize,
) -> usize {
    while matches!(
        buffer.cells().get(cell),
        Some(super::super::field_buffer::FieldCell::BreakableBlank)
    ) {
        cell += 1;
    }
    cell
}

/// Consume accepted `term_flushln()` passes at their real retirement point.
/// Word-time marker passes may already have produced a boundary; a deferred
/// scan supplies the remaining ones here. Definition and plain owners share
/// this projection, including an accepted prefix followed by rejection.
pub(in crate::mandoc::inline::flow) fn project_accepted_native_passes(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[(usize, String, usize)],
    passes: &[super::super::field_buffer::FillPass],
    projected_passes: usize,
    output_start: usize,
) -> usize {
    use super::super::field_buffer::FieldCell;

    let first = projected_passes.max(buffer.committed_pass_count());
    let count = passes.len().saturating_sub(1);
    if first >= count {
        return count;
    }
    let start = output_start.min(nodes.len());
    let mut boundaries: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, pass) in passes.iter().take(count).enumerate() {
        let mut cell = pass.end;
        while matches!(buffer.cells().get(cell), Some(FieldCell::BreakableBlank)) {
            cell += 1;
        }
        let represented = buffer.has_projected_pass(pass.end);
        if index < first || represented {
            continue;
        }
        let owner = anchors.partition_point(|(_, _, content)| *content <= cell);
        if let Some((_, marker, content)) =
            owner.checked_sub(1).and_then(|index| anchors.get(index))
        {
            boundaries
                .entry(marker.clone())
                .or_default()
                .push(buffer.projection_length(*content, cell));
        }
    }
    if !boundaries.is_empty() {
        // Earlier flushed fields are immutable output. Only the active
        // suffix is traversed, once per real flush (term.c:233-237).
        let pending = nodes.split_off(start);
        nodes.extend(split_native_field_passes(&pending, &boundaries));
    }
    count
}

#[cfg(test)]
std::thread_local! {
    static OWNER_NODES_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNER_PASS_INTERSECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Project the final native pass receipt once, in output-owner order. The
/// receipt supplies scalar offsets inside each formatter word; semantic
/// wrappers do not begin another word or reset its cursor. No formatter
/// register changes here: term.c:205-220 already chose the accepted ranges.
pub(in crate::mandoc::inline::flow) fn split_native_field_passes(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> Vec<Inline> {
    let mut ordered = boundaries.clone();
    for positions in ordered.values_mut() {
        positions.sort_unstable();
        // Different native passes can end at the same semantic scalar:
        // NBRZW sets graph but contributes no glyph (term.c:340-349).
        // Keep one event per pass; scalar equality is not event identity.
    }
    let mut cursor = NativePassCursor {
        boundaries: &[],
        scalar: 0,
        next_boundary: 0,
        owner: None,
        positions: BTreeMap::new(),
    };
    let mut output = project_native_pass_nodes(nodes, &ordered, &mut cursor);
    trim_native_pass_rows(&mut output);
    output
}

struct NativePassCursor<'a> {
    boundaries: &'a [usize],
    scalar: usize,
    next_boundary: usize,
    owner: Option<String>,
    positions: BTreeMap<String, (usize, usize)>,
}

fn native_word_marker(node: &Inline) -> Option<&str> {
    let marker = match node {
        Inline::Anchor { id, .. } => id.as_str(),
        Inline::Text { value } => value,
        _ => return None,
    };
    marker
        .starts_with(super::INTERNAL_FIELD_WORD)
        .then_some(marker)
}

fn project_native_pass_nodes<'a>(
    nodes: &[Inline],
    boundaries: &'a BTreeMap<String, Vec<usize>>,
    cursor: &mut NativePassCursor<'a>,
) -> Vec<Inline> {
    let mut output = Vec::with_capacity(nodes.len());
    for node in nodes {
        #[cfg(test)]
        OWNER_NODES_VISITED.with(|work| work.set(work.get().saturating_add(1)));
        if let Some(marker) = native_word_marker(node) {
            if let Some(owner) = cursor.owner.take() {
                cursor
                    .positions
                    .insert(owner, (cursor.scalar, cursor.next_boundary));
            }
            cursor.owner = Some(marker.to_owned());
            cursor.boundaries = boundaries.get(marker).map_or(&[], Vec::as_slice);
            (cursor.scalar, cursor.next_boundary) =
                cursor.positions.get(marker).copied().unwrap_or_default();
            output.push(node.clone());
            while cursor.boundaries.get(cursor.next_boundary) == Some(&cursor.scalar) {
                output.push(Inline::line_break());
                cursor.next_boundary += 1;
            }
            continue;
        }
        match node {
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                split_text_at_boundaries(
                    value,
                    node,
                    &mut cursor.scalar,
                    &mut cursor.next_boundary,
                    cursor.boundaries,
                    &mut output,
                );
            }
            Inline::LineBreak { .. } => {
                // A device endline is a receipt event, never a native cell
                // or a scalar from the authored word (term.c:220). If a
                // receipt boundary reaches this same native position, this
                // already executed event satisfies it exactly once.
                advance_boundary(
                    &mut cursor.scalar,
                    &mut cursor.next_boundary,
                    cursor.boundaries,
                );
                output.push(node.clone());
            }
            Inline::Strong { children } => output.push(Inline::Strong {
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            Inline::Emphasis { children } => output.push(Inline::Emphasis {
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            Inline::PortableDisplay { display, children } => output.push(Inline::PortableDisplay {
                display: display.clone(),
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            Inline::Link {
                target,
                title,
                children,
            } => output.push(Inline::Link {
                target: target.clone(),
                title: title.clone(),
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            other => output.push(other.clone()),
        }
    }
    output
}

/// A break at offset zero of a new owner consumes the previous owner's
/// breakable padding too. Do that after projection, including across style
/// and link wrappers, without dropping existing hard boundaries or identity.
fn trim_native_pass_rows(nodes: &mut Vec<Inline>) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        let starts_row = match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => {
                trim_native_pass_rows(children);
                starts_with_native_break(children)
            }
            Inline::LineBreak { .. } => true,
            _ => false,
        };
        if starts_row {
            trim_native_breakable_tail(&mut output);
        }
        output.push(node);
    }
    *nodes = output;
}

fn starts_with_native_break(nodes: &[Inline]) -> bool {
    first_native_row_event(nodes) == Some(true)
}

fn first_native_row_event(nodes: &[Inline]) -> Option<bool> {
    for node in nodes {
        if native_word_marker(node).is_some() || matches!(node, Inline::Anchor { .. }) {
            continue;
        }
        match node {
            Inline::LineBreak { .. } => return Some(true),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => {
                if let Some(event) = first_native_row_event(children) {
                    return Some(event);
                }
            }
            _ => return Some(false),
        }
    }
    None
}

fn trim_native_breakable_tail(nodes: &mut Vec<Inline>) -> bool {
    let mut metadata = Vec::new();
    let mut exhausted = true;
    while let Some(mut node) = nodes.pop() {
        if native_word_marker(&node).is_some() || matches!(node, Inline::Anchor { .. }) {
            metadata.push(node);
            continue;
        }
        let identity = crate::mandoc::inline::links::presentation::retains_authored_identity(&node);
        let consumed = match &mut node {
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                while value
                    .chars()
                    .next_back()
                    .is_some_and(super::super::super::is_formatter_word_blank)
                {
                    value.pop();
                }
                value.is_empty()
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => {
                let consumed = trim_native_breakable_tail(children);
                if consumed && !children.is_empty() {
                    metadata.push(node);
                    continue;
                }
                if consumed && (identity || matches!(node, Inline::Link { .. })) {
                    metadata.push(node);
                    continue;
                }
                consumed
            }
            _ => false,
        };
        if !consumed {
            nodes.push(node);
            exhausted = false;
            break;
        }
    }
    nodes.extend(metadata.into_iter().rev());
    exhausted
}

#[cfg(test)]
mod native_pass_tests {
    use super::{BTreeMap, Inline, split_native_field_passes};
    use crate::mandoc::inline::plain_text;
    use mant_ir::LinkTarget;

    fn marker(name: &str) -> String {
        format!("{}{name}", super::super::INTERNAL_FIELD_WORD)
    }

    #[test]
    fn accepted_owner_receipt_intersects_each_ordered_range_once() {
        use super::super::super::field_buffer::{
            FieldBuffer, FieldCell, FieldWrite, FillTargets, FlushReceipt,
        };

        // The exact 1024/2048/4096/8192-word HANG sources ran pristine CVS
        // ASCII/UTF-8/lint before this test. term_flushln() consumes accepted
        // passes forward (term.c:123-231); ownership may not rescan all old
        // passes for each word. Count the actual production intersections.
        for words in [1_024, 2_048, 4_096, 8_192] {
            let mut buffer = FieldBuffer::default();
            let mut anchors = Vec::new();
            for index in 0..=words {
                let start = buffer.cells().len();
                buffer.begin_word(false, true, false);
                let mut writes = if index == words {
                    vec![
                        FieldWrite::Cell(FieldCell::BreakMarker),
                        FieldWrite::UnprojectedBlank,
                        FieldWrite::Cell(FieldCell::Graph {
                            text: 'D',
                            width: 1,
                        }),
                    ]
                } else {
                    FieldWrite::literal("aa")
                };
                if index < words {
                    writes.push(FieldWrite::Cell(FieldCell::BreakMarker));
                }
                buffer.apply_writes(&writes);
                anchors.push((start, marker(&index.to_string()), start));
            }
            let targets = FillTargets {
                first: usize::MAX / 2,
                rest: usize::MAX / 2,
                unbounded: true,
            };
            let FlushReceipt::Rejected { passes, .. } = buffer.flush_receipt(targets, false) else {
                panic!("the graphless final marker must reject");
            };
            assert_eq!(passes.len(), words);
            super::OWNER_PASS_INTERSECTIONS.with(|work| work.set(0));
            let accepted = super::accepted_owner_lengths(&buffer, &anchors, &passes);
            assert_eq!(accepted.len(), words + 1);
            assert_eq!(accepted[&marker("0")].scalars, 2);
            assert!(!accepted[&marker(&words.to_string())].native_cells);
            let intersections = super::OWNER_PASS_INTERSECTIONS.with(std::cell::Cell::get);
            assert!(
                intersections <= 4 * (words + 1),
                "{words} owners revisited {intersections} pass intervals"
            );
        }
    }

    #[test]
    fn native_word_receipt_preserves_ordinary_kept_and_tight_boundaries() {
        use super::super::super::field_buffer::{FieldBuffer, FieldCell, FieldWrite};

        // Exact No A No B / Bk -words / No A Ns No B sources ran pristine
        // CVS ASCII/UTF-8/tree/lint first. term_word() writes its incoming
        // NOSPACE/KEEP separator before promoting PREKEEP (term.c:573-586).
        for (tight, kept, cell) in [
            (false, false, Some(FieldCell::BreakableBlank)),
            (false, true, Some(FieldCell::NonBreakingBlank)),
            (true, false, None),
        ] {
            let mut buffer = FieldBuffer::default();
            assert_eq!(buffer.begin_word(true, true, false), 0);
            buffer.apply_writes(&FieldWrite::literal("A"));
            assert_eq!(
                buffer.begin_word(tight, true, kept),
                usize::from(cell.is_some())
            );
            let receipt = buffer.apply_writes(&FieldWrite::literal("B"));
            assert_eq!(receipt.first_content_cell, if tight { 1 } else { 2 });
            assert_eq!(
                buffer.cells().get(1),
                cell.as_ref().or(Some(&FieldCell::Graph {
                    text: 'B',
                    width: 1
                }))
            );
        }
    }

    #[test]
    fn repeated_physical_flushes_never_revisit_the_committed_head_prefix() {
        // Both exact sources ran on pristine CVS -Tascii/-Tutf8/-Tlint
        // before this test. NODE_LINE executes term_newln() for each native
        // source row (mdoc_term.c:314-318); term.c:233-237 consumes that
        // field. Its earlier output must not be scanned at the next flush.
        // Count actual production visits instead of using wall-clock timing.
        for words in [128, 512] {
            let source = format!(
                ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Bl -hang -width 4n\n.It Xo\n{}.Xc\n.No BodyWord\n.El\n",
                ".No aa\n".repeat(words)
            );
            super::OWNER_NODES_VISITED.with(|work| work.set(0));
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("native-owner-scan-scale.1"),
                source.as_bytes(),
            )
            .unwrap();
            assert!(!document.sections.is_empty());
            let inspected = super::OWNER_NODES_VISITED.with(std::cell::Cell::get);
            assert!(
                inspected <= words * 8,
                "{words} rows revisited {inspected} owner nodes"
            );
        }
    }

    #[test]
    fn an_accepted_hang_mode_flush_retires_the_field_with_its_row_still_open() {
        use crate::mandoc::formatter::AuthorFlow;
        use crate::mandoc::inline::flow::native_field::FieldFlags;
        use crate::mandoc::inline::{AuthorBreakEffect, InlineBuilder};

        // Exact X/nf/Y/fi HANG input ran on pristine CVS first. fi/nf share
        // pre_br()->term_newln() (roff_term.c:45-58,69-78), and the accepted
        // field reset clears lastcol even when HANG retains its device row
        // (term.c:233-253). Check the buffer at that execution checkpoint,
        // not only the final spelling, which could hide a repeated scan.
        let mut builder = InlineBuilder::new();
        builder.inherit_author_execution_with_effect(
            AuthorFlow::default(),
            false,
            AuthorBreakEffect::Field {
                gap_cells: 1,
                body_width_columns: 6,
                field_width_columns: 6,
                flags: FieldFlags::hang(),
            },
        );
        builder.append_text("X");
        assert!(
            !builder
                .execution
                .definition
                .as_ref()
                .unwrap()
                .field_buffer
                .is_empty()
        );
        builder.fill_mode_boundary();
        assert!(
            builder
                .execution
                .definition
                .as_ref()
                .unwrap()
                .field_buffer
                .is_empty()
        );
        assert_eq!(plain_text(&builder.nodes), "X");
        builder.append_text("Y");
        let field = &builder.execution.definition.as_ref().unwrap().field_buffer;
        assert!(field.cells().iter().all(|cell| !matches!(
            cell,
            crate::mandoc::inline::flow::field_buffer::FieldCell::Graph { text: 'X', .. }
        )));
    }

    #[test]
    fn empty_native_flush_closes_only_an_already_occupied_overrun_row() {
        use crate::mandoc::inline::InlineBuilder;

        // Twelve complete short/near-full column Bd sources ran pristine
        // CVS first, lint=0. term_newln(lastcol || viscol) still reaches
        // term_flushln's vbr=0 tail with an empty buffer (term.c:475-480,
        // 143-146,250-253). No cell and no device row remains a no-op.
        for (word, expected_open) in [("LEFT", true), ("LEFT1234567", false)] {
            let mut builder = InlineBuilder::new();
            builder.begin_column_body(12, 0, false);
            builder.append_text(word);
            builder.execute_native_newline();
            assert!(builder.execution.has_open_native_device_row());
            assert!(builder.definition.as_ref().unwrap().field_buffer.is_empty());
            builder.execute_native_newline();
            assert_eq!(
                builder.execution.has_open_native_device_row(),
                expected_open
            );
            let committed = builder.nodes.clone();
            if !expected_open {
                builder.execute_native_newline();
                assert_eq!(builder.nodes, committed, "the closed row cannot end twice");
            }
        }
        let mut empty = InlineBuilder::new();
        empty.begin_column_body(12, 0, false);
        empty.execute_native_newline();
        assert!(empty.nodes.is_empty());
    }

    #[test]
    fn display_post_clears_native_no_fill_without_ending_the_retained_device_row() {
        use crate::mandoc::inline::InlineBuilder;

        // The exact compact literal-column Marker/AFTER sources ran CVS
        // first. termp_bd_post temporarily sets BRNEVER, calls term_newln,
        // then clears BRNEVER irrespective of a NOBREAK open device row
        // (mdoc_term.c:1474-1483).
        let mut builder = InlineBuilder::new();
        builder.begin_column_body(20, 0, false);
        builder.execution.no_fill_word_active = true;
        builder.append_text("Marker");
        builder.finish_display_body(Some(libmandoc_rs::DisplayKind::Literal));
        assert!(!builder.execution.no_fill_word_active);
        assert!(builder.execution.has_open_native_device_row());
        assert!(builder.definition.as_ref().unwrap().field_buffer.is_empty());
    }

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.to_owned(),
        }
    }

    #[test]
    fn owner_start_break_consumes_prior_padding_through_semantic_wrappers() {
        // The existing accepted-run segmentation contract consumes ordinary
        // separators at a pass boundary. Owner and style changes do not
        // alter that projection or the retained typed link identity.
        let target = LinkTarget::External {
            uri: "https://example.org".to_owned(),
        };
        let input = [
            Inline::anchor(marker("a")),
            Inline::Strong {
                children: vec![text("A\u{a0}  ")],
            },
            Inline::anchor("authored-target"),
            Inline::anchor(marker("b")),
            Inline::Link {
                target: target.clone(),
                title: Some("label".to_owned()),
                children: vec![Inline::Emphasis {
                    children: vec![text("B C")],
                }],
            },
        ];
        let output = split_native_field_passes(&input, &BTreeMap::from([(marker("b"), vec![0])]));
        assert_eq!(plain_text(&output), "A\u{a0}\nB C");
        assert!(matches!(
            output.last(),
            Some(Inline::Link { target: retained, title, .. })
                if retained == &target && title.as_deref() == Some("label")
        ));
        assert!(matches!(
            output.get(2),
            Some(Inline::Anchor { id, .. }) if id.as_str() == "authored-target"
        ));
    }

    #[test]
    fn scalar_cursor_crosses_styles_and_resets_at_each_native_owner() {
        let input = [
            Inline::anchor(marker("unicode")),
            text("𝔸"),
            Inline::Strong {
                children: vec![
                    Inline::anchor("authored-target"),
                    Inline::Code {
                        value: "β  γ".to_owned(),
                    },
                ],
            },
            Inline::anchor(marker("tail")),
            text("D E"),
        ];
        let output =
            split_native_field_passes(&input, &BTreeMap::from([(marker("unicode"), vec![4, 4])]));
        assert_eq!(plain_text(&output), "𝔸β\n\nγD E");
        let Inline::Strong { children } = &output[2] else {
            panic!("code's style wrapper must survive: {output:?}");
        };
        assert!(matches!(children[1], Inline::Code { .. }));
        assert!(matches!(children[4], Inline::Code { .. }));
    }

    #[test]
    fn existing_hard_boundaries_represent_events_without_native_scalars() {
        let input = [
            Inline::anchor(marker("word")),
            text("A"),
            Inline::line_break_indented(3),
            text("B C"),
            Inline::line_break(),
            Inline::line_break(),
            text("D"),
        ];
        let output =
            split_native_field_passes(&input, &BTreeMap::from([(marker("word"), vec![3, 1])]));
        assert_eq!(plain_text(&output), "A\nB\nC\n\nD");
        assert!(matches!(output[2], Inline::LineBreak { indent_columns: 3 }));
    }

    #[test]
    fn private_text_marker_does_not_advance_the_scalar_cursor() {
        let input = [text(&marker("text")), text("A B")];
        let output =
            split_native_field_passes(&input, &BTreeMap::from([(marker("text"), vec![2])]));
        assert_eq!(output[0], input[0]);
        assert_eq!(plain_text(&output[1..]), "A\nB");
    }

    #[test]
    fn zero_offset_boundary_does_not_require_a_visible_owner_glyph() {
        let input = [
            Inline::anchor(marker("prefix")),
            text("A  "),
            Inline::anchor(marker("empty")),
            text(""),
            Inline::anchor(marker("suffix")),
            text("B"),
        ];
        let output =
            split_native_field_passes(&input, &BTreeMap::from([(marker("empty"), vec![0])]));
        assert_eq!(plain_text(&output), "A\nB");
        assert!(
            output.contains(&text("")),
            "keep its row witness: {output:?}"
        );
    }

    #[test]
    fn many_owner_passes_keep_every_accepted_piece_in_source_order() {
        let mut input = Vec::new();
        let mut boundaries = BTreeMap::new();
        for owner in 0..1_024 {
            let owner = marker(&owner.to_string());
            input.push(Inline::anchor(owner.clone()));
            input.push(text("x  y"));
            boundaries.insert(owner, vec![3]);
        }
        let output = split_native_field_passes(&input, &boundaries);
        assert_eq!(plain_text(&output), "x\ny".repeat(1_024));
    }
}
