//! Project accepted native field passes without executing formatter state.

use super::super::Inline;
use super::split::{advance_boundary, split_text_at_boundaries};
use std::collections::BTreeMap;

#[cfg(test)]
std::thread_local! {
    static OWNER_NODES_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
        positions.dedup();
    }
    let mut cursor = NativePassCursor {
        boundaries: &[],
        scalar: 0,
        next_boundary: 0,
    };
    let mut output = project_native_pass_nodes(nodes, &ordered, &mut cursor);
    trim_native_pass_rows(&mut output);
    output
}

struct NativePassCursor<'a> {
    boundaries: &'a [usize],
    scalar: usize,
    next_boundary: usize,
}

/// Existing authored hard breaks belong to stable word owners even when a
/// semantic style or link wrapper encloses them. Width-pass projection must
/// not reinterpret such a break as an automatic split at another scalar.
pub(in crate::mandoc::inline::flow) fn authored_owner_breaks(
    nodes: &[Inline],
) -> BTreeMap<String, usize> {
    fn visit(nodes: &[Inline], owner: &mut Option<String>, breaks: &mut BTreeMap<String, usize>) {
        for node in nodes {
            #[cfg(test)]
            OWNER_NODES_VISITED.with(|work| work.set(work.get().saturating_add(1)));
            if let Some(marker) = native_word_marker(node) {
                *owner = Some(marker.to_owned());
            } else {
                match node {
                    Inline::LineBreak { .. } => {
                        if let Some(owner) = owner.as_ref() {
                            *breaks.entry(owner.clone()).or_default() += 1;
                        }
                    }
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => visit(children, owner, breaks),
                    _ => {}
                }
            }
        }
    }
    let mut breaks = BTreeMap::new();
    visit(nodes, &mut None, &mut breaks);
    breaks
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
        if let Some(marker) = native_word_marker(node) {
            cursor.boundaries = boundaries.get(marker).map_or(&[], Vec::as_slice);
            cursor.scalar = 0;
            cursor.next_boundary = 0;
            output.push(node.clone());
            if cursor.boundaries.first() == Some(&0) {
                output.push(Inline::line_break());
                cursor.next_boundary = 1;
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
                // An already projected hard boundary satisfies the same
                // pass boundary. It remains an existing scalar, so later
                // positions in this owner still count it exactly once.
                advance_boundary(
                    &mut cursor.scalar,
                    &mut cursor.next_boundary,
                    cursor.boundaries,
                );
                output.push(node.clone());
                cursor.scalar += 1;
                advance_boundary(
                    &mut cursor.scalar,
                    &mut cursor.next_boundary,
                    cursor.boundaries,
                );
            }
            Inline::Strong { children } => output.push(Inline::Strong {
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            Inline::Emphasis { children } => output.push(Inline::Emphasis {
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
            | Inline::Link { children, .. } => {
                let consumed = trim_native_breakable_tail(children);
                if consumed && !children.is_empty() {
                    metadata.push(node);
                    continue;
                }
                if consumed && matches!(node, Inline::Link { .. }) {
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
        assert_eq!(plain_text(&output), "𝔸β\nγD E");
        let Inline::Strong { children } = &output[2] else {
            panic!("code's style wrapper must survive: {output:?}");
        };
        assert!(matches!(children[1], Inline::Code { .. }));
        assert!(matches!(children[3], Inline::Code { .. }));
    }

    #[test]
    fn existing_hard_boundaries_keep_their_scalars_and_empty_rows() {
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
            split_native_field_passes(&input, &BTreeMap::from([(marker("word"), vec![4, 1])]));
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
