//! Reconstructs source-proven lists expressed with man(7) tagged paragraphs.

use mant_ir::{Block, DefinitionItem, ListItem, ListKind, SourceSpan};

use crate::mandoc::{
    inline::plain_text,
    layout::{layout, layout_with_spacing},
    targets,
};
use mant_ir::geometry::rebase_roots;

/// The handle belongs to this driver's output container, never to a detached subtree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) struct ManListState {
    active: Option<ActiveOrdinal>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveOrdinal {
    block: usize,
    marker: ManOrdinalMarker,
}

impl ManListState {
    pub(in crate::mandoc::blocks) const fn new() -> Self {
        Self { active: None }
    }
    pub(in crate::mandoc::blocks) const fn is_active(self) -> bool {
        self.active.is_some()
    }
    pub(in crate::mandoc::blocks) fn reset(&mut self) {
        self.active = None;
    }
}

impl ActiveOrdinal {
    /// A new ordinal joins only the physical tail, not an earlier list.
    fn last_list(self, output: &mut [Block]) -> Option<(&mut bool, &mut Vec<ListItem>)> {
        if output.len().checked_sub(1) != Some(self.block) {
            return None;
        }
        self.owned_list(output)
    }

    /// Continuation ownership survives separately emitted spacing requests.
    /// The driver resets this handle at structural boundaries; an output-tail
    /// check here would invent a new ownership boundary for `.sp`.
    fn owned_list(self, output: &mut [Block]) -> Option<(&mut bool, &mut Vec<ListItem>)> {
        match output.get_mut(self.block)? {
            Block::List {
                kind: ListKind::Ordered { .. },
                compact,
                items,
                ..
            } => Some((compact, items)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) struct ManOrdinalMarker {
    value: u64,
    style: IpOrdinalStyle,
}

impl ManOrdinalMarker {
    pub(in crate::mandoc::blocks) const fn value(self) -> u64 {
        self.value
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpOrdinalStyle {
    Period,
    ClosingParenthesis,
    Parenthesized,
    IncrementingRegister,
}

/// Parse only source-proven enumerator spellings used by man(7) `.IP`/`.TP`.
///
/// A bare resolved integer is ambiguous: option manuals routinely use the
/// same spelling for a real value domain. It is therefore accepted only when
/// the source line used roff's pre-increment register form. Punctuation is
/// retained as a sequence style so `1.` followed by `2)` cannot accidentally
/// merge across unrelated tagged paragraphs.
pub(in crate::mandoc::blocks) fn ordinal_marker(
    item: &DefinitionItem,
    uses_incrementing_register: bool,
) -> Option<ManOrdinalMarker> {
    if item.description.is_empty() {
        return None;
    }
    let [term] = item.terms.as_slice() else {
        return None;
    };
    let text = plain_text(term);
    let text = text.trim();
    let (digits, style) = if let Some(digits) = text.strip_suffix('.') {
        (digits, IpOrdinalStyle::Period)
    } else if let Some(digits) = text.strip_suffix(')') {
        if let Some(digits) = digits.strip_prefix('(') {
            (digits, IpOrdinalStyle::Parenthesized)
        } else {
            (digits, IpOrdinalStyle::ClosingParenthesis)
        }
    } else if uses_incrementing_register {
        (text, IpOrdinalStyle::IncrementingRegister)
    } else {
        return None;
    };
    let value = digits.parse().ok()?;
    Some(ManOrdinalMarker { value, style })
}

/// Recognize an entire mdoc tag list as one explicit ordinal sequence.
///
/// Unlike man(7) tagged paragraphs, `.Bl -tag` is explicit evidence for a
/// definition list. Override it only when at least two described items form a
/// complete, consecutive sequence with one punctuation style. This leaves
/// singleton numeric terms and value domains untouched.
pub(in crate::mandoc::blocks) fn ordinal_sequence(
    items: &[DefinitionItem],
) -> Option<ManOrdinalMarker> {
    if items.len() < 2 {
        return None;
    }
    let first = ordinal_marker(&items[0], false)?;
    let mut previous = first;
    for item in &items[1..] {
        let marker = ordinal_marker(item, false)?;
        if marker.style != previous.style || previous.value.checked_add(1) != Some(marker.value) {
            return None;
        }
        previous = marker;
    }
    Some(first)
}

/// Convert a source-proven man enumerator into an ordered list, appending it
/// to an adjacent sequence when the spelling and numeric progression agree.
///
/// Punctuated integers and source-level incrementing registers carry enough
/// evidence even when a list contains only one item.  Requiring a second item
/// used to leak singleton footnote labels such as `1.` into the semantic entry
/// index.  Bare literal integers remain excluded by [`ordinal_marker`].
pub(in crate::mandoc::blocks) fn append_ordered(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<SourceSpan>,
    marker: ManOrdinalMarker,
    state: &mut ManListState,
) {
    if let Some(previous) = state.active
        && previous.marker.style == marker.style
        && previous.marker.value.checked_add(1) == Some(marker.value)
        && let Some((compact, items)) = previous.last_list(output)
    {
        *compact = *compact && paragraph_distance == 0;
        items.push(spaced_man_list_item(
            item,
            ordinal_width(marker.value),
            source,
            paragraph_distance,
        ));
        state.active = Some(ActiveOrdinal {
            block: previous.block,
            marker,
        });
        return;
    }
    append_new_ordered(
        output,
        item,
        indent_columns,
        paragraph_distance,
        source,
        marker,
        state,
    );
}

fn append_new_ordered(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<SourceSpan>,
    marker: ManOrdinalMarker,
    state: &mut ManListState,
) {
    let block = output.len();
    output.push(Block::List {
        kind: ListKind::Ordered {
            start: Some(marker.value),
        },
        compact: paragraph_distance == 0,
        items: vec![spaced_man_list_item(
            item,
            ordinal_width(marker.value),
            source,
            paragraph_distance,
        )],
        layout: layout_with_spacing(indent_columns, 0),
        source,
    });
    state.active = Some(ActiveOrdinal { block, marker });
}

/// Attach a transparent relative-indent scope to the current `.IP` item.
///
/// AsciDoc-generated man pages commonly put a reference URI in an `.RS/.RE`
/// scope immediately after each numbered `.IP`.  Libmandoc correctly exposes
/// that scope as a sibling of the `.IP`, but it remains content of the same
/// visible item and must not break ordinal sequence recognition.  Proven list
/// items use coordinates relative to the list container.
pub(in crate::mandoc::blocks) fn append_relative_continuation(
    output: &mut [Block],
    nested: &mut Vec<Block>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    state: ManListState,
) -> bool {
    let Some(active) = state.active else {
        return false;
    };
    let origin = indent_columns
        .relative_columns()
        .saturating_add(ordinal_width(active.marker.value));
    let Some((_, items)) = active.owned_list(output) else {
        return false;
    };
    let Some(item) = items.last_mut() else {
        return false;
    };
    rebase_roots(nested, 0, origin);
    item.blocks.append(nested);
    true
}

fn ordinal_width(value: u64) -> i32 {
    mant_ir::geometry::coordinate(mant_ir::geometry::text_width(&format!("{value}. ")))
}

/// The tagged paragraph's leading boundary precedes its marker, not its
/// first body block. Store it once on the item, regardless of list grouping.
pub(in crate::mandoc::blocks) fn spaced_man_list_item(
    item: DefinitionItem,
    marker_width: i32,
    source: Option<SourceSpan>,
    paragraph_distance: u16,
) -> ListItem {
    let mut item = list_item_from_definition(item, marker_width, source);
    item.layout.spacing_before_lines = Some(paragraph_distance);
    item
}

/// Remove an `.IP`/`.TP` mark from visible content while conserving any target
/// it owned and making item indentation relative to the new list container.
pub(in crate::mandoc::blocks) fn list_item_from_definition(
    item: DefinitionItem,
    marker_width: i32,
    source: Option<SourceSpan>,
) -> ListItem {
    let DefinitionItem {
        source: item_source,
        terms,
        mut description,
        layout: definition_layout,
        ..
    } = item;
    // The source body column survives semantic conversion. Only these roots
    // move from the definition body origin to the list marker's body origin.
    rebase_roots(
        &mut description,
        definition_layout.body_indent_columns,
        marker_width,
    );
    let mut anchors = Vec::new();
    for term in &terms {
        targets::inline_anchor_ids(term, &mut anchors);
    }
    targets::attach_targets(
        &mut description,
        anchors,
        layout(crate::mandoc::layout::SourceIndent::default()),
        source,
    );
    ListItem {
        layout: mant_ir::ListItemLayout::default(),
        source: item_source,
        entry: None,
        blocks: description,
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint, ListKind};

    fn definition(term: &str, description: &str) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: vec![vec![Inline::Text {
                value: term.to_owned(),
            }]],
            description: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: description.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        }
    }

    #[test]
    fn recognizes_only_unambiguous_ordinal_spellings() {
        for marker in ["1.", "2)", "(3)"] {
            assert!(super::ordinal_marker(&definition(marker, "item"), false).is_some());
        }
        assert!(super::ordinal_marker(&definition("1", "item"), true).is_some());
        for value in ["1", "[4]", "2.2", "v1.", "1.2."] {
            assert!(super::ordinal_marker(&definition(value, "value"), false).is_none());
        }
        let mut empty = definition("1.", "");
        empty.description.clear();
        assert!(super::ordinal_marker(&empty, false).is_none());
    }

    #[test]
    fn recognizes_only_complete_punctuated_mdoc_sequences() {
        let sequence = [
            definition("1.", "one"),
            definition("2.", "two"),
            definition("3.", "three"),
        ];
        assert_eq!(
            super::ordinal_sequence(&sequence).map(super::ManOrdinalMarker::value),
            Some(1)
        );

        assert!(super::ordinal_sequence(&sequence[..1]).is_none());
        assert!(
            super::ordinal_sequence(&[definition("1.", "one"), definition("3.", "three")])
                .is_none()
        );
        assert!(
            super::ordinal_sequence(&[definition("1.", "one"), definition("2)", "two")]).is_none()
        );
        assert!(
            super::ordinal_sequence(&[definition("0", "off"), definition("1", "on")]).is_none()
        );
    }

    #[test]
    fn ordered_conversion_conserves_a_native_term_target() {
        let mut item = definition("1.", "item");
        item.terms[0].insert(0, Inline::anchor("native-target"));
        let marker = super::ordinal_marker(&item, false).expect("punctuated ordinal");
        let mut output = Vec::new();
        let mut state = super::ManListState::new();

        super::append_ordered(
            &mut output,
            item,
            crate::mandoc::layout::SourceIndent::default(),
            1,
            None,
            marker,
            &mut state,
        );

        let [
            Block::List {
                kind: ListKind::Ordered { .. },
                items,
                ..
            },
        ] = output.as_slice()
        else {
            panic!("ordered list");
        };
        assert!(matches!(
            items[0].blocks[0],
            Block::Paragraph { ref children, .. }
                if matches!(children.first(), Some(Inline::Anchor { id, .. }) if id == "native-target")
        ));
    }

    fn append(output: &mut Vec<Block>, state: &mut super::ManListState, term: &str, distance: u16) {
        let item = definition(term, term);
        let marker = super::ordinal_marker(&item, false).unwrap();
        super::append_ordered(output, item, 0.into(), distance, None, marker, state);
    }

    #[test]
    fn ordinal_state_rejects_removed_or_wrong_role_owner_without_consuming_continuation() {
        for removed in [false, true] {
            let mut output = Vec::new();
            let mut state = super::ManListState::new();
            append(&mut output, &mut state, "1.", 0);
            let paragraph = definition("plain", "paragraph").description.pop().unwrap();
            if removed {
                output.clear();
            } else {
                output[0] = paragraph;
            }
            let before = output.clone();
            let mut continuation = definition("continuation", "tail").description;
            let expected = continuation.clone();
            assert!(!super::append_relative_continuation(
                &mut output,
                &mut continuation,
                0.into(),
                state
            ));
            assert_eq!(output, before);
            assert_eq!(continuation, expected);
            append(&mut output, &mut state, "2.", 0);
            assert!(
                matches!(output.last(), Some(Block::List { kind: ListKind::Ordered { start: Some(2) }, items, .. }) if items.len() == 1)
            );
        }
    }

    #[test]
    fn ordinal_continuation_keeps_its_owner_across_separate_spacing_output() {
        let mut output = Vec::new();
        let mut state = super::ManListState::new();
        append(&mut output, &mut state, "1.", 0);
        let spacing = Block::VerticalSpace {
            lines: 2,
            source: None,
        };
        output.push(spacing.clone());
        let mut continuation = definition("continuation", "tail").description;
        assert!(super::append_relative_continuation(
            &mut output,
            &mut continuation,
            0.into(),
            state
        ));
        assert!(continuation.is_empty());
        assert_eq!(output[1], spacing);
        assert!(matches!(&output[0], Block::List { items, .. } if items[0].blocks.len() == 2));
        // In contrast, the next ordinal cannot join across that physical gap.
        append(&mut output, &mut state, "2.", 0);
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn native_spacing_between_relative_scopes_does_not_detach_item_contents() {
        #[derive(Default)]
        struct Text(String);
        impl<'ir> mant_ir::visit::Visit<'ir> for Text {
            fn visit_inline(&mut self, inline: &'ir Inline) {
                if let Inline::Text { value } | Inline::Code { value } = inline {
                    self.0.push_str(value);
                }
                mant_ir::visit::walk_inline(self, inline);
            }
        }
        for spacing in [".sp 1", ".PD 0", ".sp 2\n.PD 2"] {
            let source = format!(
                ".TH STATE 1\n.SH DESCRIPTION\n.IP 1.\nFIRST\n{spacing}\n.RS 4\nLEFT\n.RE\n{spacing}\n.RS 4\nRIGHT\n.RE\n.IP 2.\nSECOND\n"
            );
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("ordinal-spacing.1"),
                source.as_bytes(),
            )
            .unwrap();
            let first = document.sections[0]
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::List { items, .. } => items.first(),
                    _ => None,
                })
                .unwrap();
            let mut text = Text::default();
            for block in &first.blocks {
                mant_ir::visit::Visit::visit_block(&mut text, block);
            }
            let text = text.0;
            assert!(
                text.contains("FIRST") && text.contains("LEFT") && text.contains("RIGHT"),
                "{source}\n{document:?}"
            );
            assert!(!text.contains("SECOND"), "{source}\n{document:?}");
        }
    }

    #[test]
    fn ordinal_state_reset_and_empty_output_cannot_reuse_an_old_item() {
        let mut output = Vec::new();
        let mut state = super::ManListState::new();
        append(&mut output, &mut state, "1.", 0);
        let mut continuation = definition("continuation", "tail").description;
        assert!(!super::append_relative_continuation(
            &mut [],
            &mut continuation,
            0.into(),
            state
        ));
        state.reset();
        assert!(!state.is_active());
        assert!(!super::append_relative_continuation(
            &mut output,
            &mut continuation,
            0.into(),
            state
        ));
        append(&mut output, &mut state, "2.", 0);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn paragraph_distance_does_not_reset_ordered_progression() {
        let mut output = Vec::new();
        let mut state = super::ManListState::new();
        append(&mut output, &mut state, "1.", 0);
        append(&mut output, &mut state, "2.", 2);
        let [Block::List { items, compact, .. }] = output.as_slice() else {
            panic!("one sequence: {output:?}");
        };
        assert!(!compact);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].layout.spacing_before_lines, Some(0));
        assert_eq!(items[1].layout.spacing_before_lines, Some(2));
    }

    #[test]
    fn native_pd_keeps_sequence_but_plain_paragraph_stops_it() {
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("ordinal-state.1"),
            b".TH STATE 1\n.SH DESCRIPTION\n.PD 0\n.IP 1.\nFIRST\n.PD 2\n.IP 2.\nSECOND\n.PP\nBOUNDARY\n.IP 3.\nTHIRD\n",
        ).unwrap();
        let lists: Vec<_> = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::List {
                    kind: ListKind::Ordered { start },
                    items,
                    ..
                } => Some((*start, items.len())),
                _ => None,
            })
            .collect();
        assert_eq!(lists, [(Some(1), 2), (Some(3), 1)]);
    }

    #[test]
    fn bracketed_ip_indexes_remain_exact_definition_labels() {
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("bracketed-ip-index.5"),
            b".TH BRACKETED-IP-INDEX 5\n.SH DESCRIPTION\n.IP [0] 5\nZERO\n.IP [1]\nONE\n",
        )
        .expect("parse bracketed indices");
        let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
            panic!("bracketed IP labels must not be rewritten as decimal list markers");
        };
        assert_eq!(crate::mandoc::inline::plain_text(&items[0].terms[0]), "[0]");
        assert_eq!(crate::mandoc::inline::plain_text(&items[1].terms[0]), "[1]");
        assert!(items.iter().all(|item| item.entry.is_none()));
    }
}
