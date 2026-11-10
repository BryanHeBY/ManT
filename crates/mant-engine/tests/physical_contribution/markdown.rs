//! Compare each Markdown readback to the input obligations, not just itself.
use super::*;
use mant_codec::encode::{
    MarkdownArtifact, MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
};

#[derive(Default)]
struct BodyFacts {
    /// Authored phrasing payload; block framing is tested separately.
    text: Vec<String>,
    literal_rows: Vec<String>,
    hard_rows: usize,
    gap_rows: usize,
}

impl<'ir> visit::Visit<'ir> for BodyFacts {
    fn visit_block(&mut self, block: &'ir Block) {
        self.gap_rows += geometry::block_layout(block)
            .map_or(0, |layout| usize::from(layout.spacing_before_lines));
        match block {
            Block::Paragraph { children, .. } => {
                let text = inline_plain_text(children);
                // A final delimiter opens a provisional row, while an
                // interior empty row has already been completed.
                self.hard_rows += text
                    .split_terminator('\n')
                    .filter(|row| row.is_empty())
                    .count()
                    * usize::from(text.contains('\n'));
                self.text.extend(
                    text.split('\n')
                        .filter(|row| !row.is_empty())
                        .map(str::to_owned),
                );
            }
            Block::Preformatted { children, .. } => {
                self.literal_rows.push(inline_plain_text(children));
            }
            Block::VerticalSpace { lines, .. } => self.gap_rows += usize::from(*lines),
            _ => visit::walk_block(self, block),
        }
    }
}

fn body_facts(document: &Document) -> BodyFacts {
    let mut facts = BodyFacts::default();
    visit::Visit::visit_document(&mut facts, document);
    facts
}

fn expected_blank_rows(body: Body, owner: Owner, neighbors: Neighbors) -> (usize, usize) {
    let explicit_neighbor_gap = usize::from(matches!(
        neighbors,
        Neighbors::GapBefore | Neighbors::GapAfter
    ));
    match body {
        Body::HardRow => (1, explicit_neighbor_gap),
        Body::EmptyLiteralRow => (0, explicit_neighbor_gap),
        Body::Space | Body::LinkedText => (
            0,
            if neighbors == Neighbors::Both {
                2
            } else {
                explicit_neighbor_gap
            },
        ),
        _ => match neighbors {
            Neighbors::None => (usize::from(!body.rows().is_empty()), 0),
            // An independent paragraph requires one syntax blank; an
            // authored gap at this same boundary does not add another.
            Neighbors::Both => (0, 1),
            // Plain lists and headless definitions acquire a structural
            // CommonMark marker. Its otherwise empty row is not a second
            // source blank. A definition uses the separator after its marker
            // as the gap, whereas a plain list writes a hard row inside it.
            Neighbors::GapAfter if owner.definition() => (1, 1),
            Neighbors::GapAfter if matches!(owner, Owner::PlainRoot | Owner::PlainSection) => {
                (2, 0)
            }
            Neighbors::GapBefore | Neighbors::GapAfter => (1, 0),
        },
    }
}

/// Raw-HTML navigation is an addressable-export obligation, not input prose.
/// Discard only the exact mapped tags and their standalone syntax carrier.
fn source_prose(artifact: &MarkdownArtifact<'_>) -> String {
    let mut removed = vec![false; artifact.text().len()];
    for range in artifact.anchor_ranges() {
        removed[range.clone()].fill(true);
    }
    let mut output = Vec::new();
    let mut offset = 0;
    let mut lines = artifact.text().split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let end = offset + line.len();
        let remaining = line
            .bytes()
            .zip(&removed[offset..end])
            .filter_map(|(byte, remove)| (!remove).then_some(byte))
            .collect::<Vec<_>>();
        let had_markers = removed[offset..end].contains(&true);
        offset = end;
        if had_markers && remaining.iter().all(|byte| matches!(byte, b' ' | b'\n')) {
            // A standalone addressable heading prefix has a separating
            // blank syntax line. No authored paragraph is in the run.
            if lines.peek() == Some(&"\n") && lines.next().is_some() {
                offset += 1;
            }
        } else {
            output.extend(remaining);
        }
    }
    String::from_utf8(output).unwrap()
}

#[derive(Clone, Copy)]
struct SourceCase {
    body: Body,
    owner: Owner,
    tail: usize,
    neighbors: Neighbors,
}

fn assert_source_obligations(
    artifact: &MarkdownArtifact<'_>,
    readback: &ResolvedContent,
    SourceCase {
        body,
        owner,
        tail,
        neighbors,
    }: SourceCase,
    options: MarkdownOptions,
    cycle: usize,
) {
    let document = readback.document.as_ref().unwrap();
    assert_eq!(validate_document(document), []);
    assert_targets(readback, body, neighbors, false);
    assert_eq!(
        inline_plain_text(&document.heading.as_ref().unwrap().content),
        format!("{LABEL}{}", "\n".repeat(tail)),
        "Setext document heading retains authored hard rows"
    );
    if owner.section() {
        let subject = if matches!(owner, Owner::NestedSection) {
            &document.sections[0].children[0]
        } else {
            &document.sections[0]
        };
        // CommonMark cannot contain hard rows in a deeper ATX heading. Its
        // words are retained; this is the documented projection, not a blank.
        let retained = if matches!(owner, Owner::NestedSection) {
            0
        } else {
            tail
        };
        assert_eq!(
            inline_plain_text(&subject.heading.content),
            format!("Subject{}", "\n".repeat(retained))
        );
    }
    let actual = body_facts(document);
    let expected = neighbors
        .rows(body)
        .into_iter()
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(actual.text, expected, "portable phrasing rows");
    assert_eq!(
        actual.literal_rows,
        if body == Body::EmptyLiteralRow {
            vec![String::new()]
        } else {
            vec![]
        },
        "authored empty literal owns a fence"
    );
    assert_eq!(
        (actual.hard_rows, actual.gap_rows),
        expected_blank_rows(body, owner, neighbors),
        "exact authored hard rows and resolved spacing after Markdown framing"
    );
    if body.rows().is_empty() {
        let mut control = base(tail);
        owner.install(&mut control, tail, neighbors.blocks(Body::None));
        let control = render_addressable_markdown_with_options(&control, options);
        let control_readback =
            mant_loader::load_markdown_text(&source_prose(&control), None).unwrap();
        let expected = body_facts(control_readback.document.as_ref().unwrap());
        assert_eq!(
            actual.gap_rows, expected.gap_rows,
            "transparent payload must not add a paragraph gap"
        );
        assert_eq!(
            actual.hard_rows, expected.hard_rows,
            "transparent payload must not add a hard row"
        );
        assert_eq!(
            render_query_text(readback),
            render_query_text(&control_readback),
            "transparent payload must not add a physical row"
        );
    }
    for owner in artifact.nodes() {
        let bytes = &artifact.text()[owner.range()];
        if matches!(owner.node(), MarkdownNode::DocumentRoot) && !bytes.is_empty() {
            assert!(
                !bytes.contains("Subject"),
                "root range excludes section heading"
            );
        }
    }
    if options.preserve_anchors && body.anchor() && cycle == 1 {
        assert_eq!(
            artifact
                .anchor_ranges()
                .iter()
                .filter(|range| artifact.text()[(*range).clone()] == *"<a id=\"Source.Nav\"></a>")
                .count(),
            1
        );
    }
}

fn assert_raw_addressable_readback(readback: &ResolvedContent, case: SourceCase) {
    let document = readback.document.as_ref().unwrap();
    assert_eq!(validate_document(document), []);
    assert_targets(readback, case.body, case.neighbors, false);
    assert_eq!(
        inline_plain_text(&document.heading.as_ref().unwrap().content),
        format!("{LABEL}{}", "\n".repeat(case.tail)),
    );
    let facts = body_facts(document);
    let body = facts.text.join("\n");
    let mut cursor = 0;
    for row in case.neighbors.rows(case.body) {
        match row {
            "" => {}
            " " => assert!(facts.text.iter().any(|row| row == " ")),
            _ => {
                assert_eq!(
                    body.matches(row).count(),
                    1,
                    "authored row {row:?} in {body:?}"
                );
                cursor += body[cursor..].find(row).expect("source row order") + row.len();
            }
        }
    }
    if case.body == Body::EmptyLiteralRow {
        assert_eq!(facts.literal_rows, [String::new()]);
    }
}

#[test]
fn first_markdown_export_and_second_cycle_keep_source_contribution() {
    let mut cases = 0;
    for body in BODIES {
        for owner in OWNERS {
            for tail in 0..=2 {
                for neighbors in NEIGHBORS {
                    let mut value = base(tail);
                    owner.install(&mut value, tail, neighbors.blocks(body));
                    for wire in [false, true] {
                        let restored = if wire {
                            query_wire(&value)
                        } else {
                            value.clone()
                        };
                        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                            let case = SourceCase {
                                body,
                                owner,
                                tail,
                                neighbors,
                            };
                            let mut input = restored.clone();
                            let mut raw_input = restored.clone();
                            for cycle in 1..=2 {
                                let artifact =
                                    render_addressable_markdown_with_options(&input, options);
                                let readback =
                                    mant_loader::load_markdown_text(&source_prose(&artifact), None)
                                        .unwrap();
                                let result = std::panic::catch_unwind(|| {
                                    assert_source_obligations(
                                        &artifact, &readback, case, options, cycle,
                                    );
                                });
                                if let Err(failure) = result {
                                    eprintln!(
                                        "{body:?}/{owner:?}/{tail}/{neighbors:?}/{wire}/{options:?} cycle {cycle}: {:?}",
                                        artifact.text()
                                    );
                                    std::panic::resume_unwind(failure);
                                }
                                let route = if options.preserve_anchors {
                                    "addressable"
                                } else {
                                    "default"
                                };
                                let heading = if matches!(owner, Owner::NestedSection) {
                                    "atx-folding"
                                } else {
                                    "setext-hard-rows"
                                };
                                audit(
                                    "CG-D",
                                    &format!(
                                        "{body:?}/{owner:?}/{tail}/{neighbors:?}/{wire}/{route}/{cycle}"
                                    ),
                                    &format!("{heading}/exact-body-layout/targets/ranges"),
                                );
                                if options.preserve_anchors {
                                    // Preserve a separate, unmodified consumer chain. The
                                    // Markdown reader retains generated raw HTML as fallback
                                    // text, so those rows are outside the addressable import
                                    // equality policy. This still parses the real export, checks
                                    // the original authored payload, and reexports that IR.
                                    let raw = render_addressable_markdown_with_options(
                                        &raw_input, options,
                                    );
                                    let readback =
                                        mant_loader::load_markdown_text(raw.text(), None).unwrap();
                                    let result = std::panic::catch_unwind(|| {
                                        assert_raw_addressable_readback(&readback, case);
                                    });
                                    if let Err(failure) = result {
                                        eprintln!(
                                            "{body:?}/{owner:?}/{tail}/{neighbors:?}/{wire}/raw cycle {cycle}: {:?}",
                                            raw.text()
                                        );
                                        std::panic::resume_unwind(failure);
                                    }
                                    audit(
                                        "CG-D-raw",
                                        &format!(
                                            "{body:?}/{owner:?}/{tail}/{neighbors:?}/{wire}/{cycle}"
                                        ),
                                        "exact-authored-payload-and-links/raw-html-row-fallback-outside-equality",
                                    );
                                    raw_input = readback;
                                }
                                input = readback;
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 5_376);
}
