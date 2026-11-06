//! Native source events outlive output owners, but not their actual scopes.

#[path = "execution_scopes/fixture.rs"]
mod fixture;
#[path = "execution_scopes/tables.rs"]
mod tables;

use mant_ir::ResolvedContent;
use mant_ir::visit::Visit;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

fn load(case: &fixture::Case) -> ResolvedContent {
    if case.expected.require_name_body {
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("name-table-body.1", case.source.as_bytes())
            .unwrap();
        assert!(
            original_name_body(&parsed.document.root, false),
            "{}: execute Nm BLOCK in original It BODY, not an elem",
            case.id
        );
    }
    let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    assert_eq!(
        mant_ir::validate_document(original.document.as_ref().unwrap()),
        []
    );
    let restored = json_string_roundtrip(&original);
    assert_eq!(
        original, restored,
        "{}: actual JSON text roundtrip",
        case.id
    );
    restored
}

fn original_name_body(node: &libmandoc_rs::Node, in_item_body: bool) -> bool {
    use libmandoc_rs::{MacroToken::Mdoc, MdocMacro, NodeKind};
    let in_item_body = in_item_body
        || (node.kind == NodeKind::Body && node.macro_token == Some(Mdoc(MdocMacro::It)));
    (in_item_body && node.kind == NodeKind::Block && node.macro_token == Some(Mdoc(MdocMacro::Nm)))
        || node
            .children
            .iter()
            .any(|child| original_name_body(child, in_item_body))
}

fn matches_word(row: &str, token: &str) -> Vec<usize> {
    row.match_indices(token)
        .filter_map(|(start, _)| {
            let end = start + token.len();
            let part = |character: char| character.is_alphanumeric() || character == '_';
            (!row[..start].chars().next_back().is_some_and(part)
                && !row[end..].chars().next().is_some_and(part))
            .then_some(start)
        })
        .collect()
}

fn prose_rows(rendered: &str, tokens: &[String]) -> Vec<fixture::ProseRow> {
    rendered
        .lines()
        .filter_map(|row| {
            let words = tokens
                .iter()
                .flat_map(|token| {
                    matches_word(row, token)
                        .into_iter()
                        .map(move |start| (start, start + token.len()))
                })
                .collect::<Vec<_>>();
            let first = words.iter().map(|(start, _)| *start).min()?;
            let last = words.iter().map(|(_, end)| *end).max().unwrap();
            Some(fixture::ProseRow {
                text: row[first..last].into(),
                column: i32::try_from(mant_ir::geometry::text_width(&row[..first])).unwrap(),
            })
        })
        .collect()
}

#[test]
fn ended_name_fields_do_not_control_later_prose_width() {
    // Exact complete sources ran the five pinned profiles before recording.
    // man_term.c::post_SY(BODY) consumes the live input and restores offset;
    // its former HEAD width must not be reused by the following PP/br/RS.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| !case.expected.prose_tokens.is_empty()),
        |case| &case.id,
        |case| {
            let content = load(case);
            let rendered = mant_render::render_query_man(&content);
            let heading = format!("{}\n", case.expected.heading);
            let body = rendered.split_once(&heading).unwrap().1;
            let body = body.split_once("NEXT\n").map_or(body, |(body, _)| body);
            let actual = prose_rows(body, &case.expected.prose_tokens);
            if case.expected.responsive_soft_wrap {
                // The explicit ll device limit wraps these six ordinary
                // words. Responsive reading may reflow only those soft
                // boundaries; no source word, separator or origin is lost.
                assert_eq!(
                    actual
                        .iter()
                        .map(|row| row.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                    case.expected
                        .prose_rows
                        .iter()
                        .map(|row| row.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                    "{}: complete responsive source words",
                    case.id
                );
                assert!(
                    actual
                        .iter()
                        .all(|row| row.column == case.expected.prose_rows[0].column)
                );
            } else {
                assert_eq!(
                    actual, case.expected.prose_rows,
                    "{}: exact surviving words, native hard rows and origins",
                    case.id
                );
            }
            for token in &case.expected.prose_tokens {
                assert_eq!(
                    body.lines()
                        .map(|row| matches_word(row, token).len())
                        .sum::<usize>(),
                    1,
                    "{}: {token} has one original owner",
                    case.id
                );
            }
        },
    );
}

#[test]
fn nested_plain_items_execute_source_origins_before_display_owners() {
    // termp_it_pre HEAD/BODY establishes each source offset before D1's +6
    // or Bd's signed local offset. Generic node return restores geometry
    // after post, never by subtracting an unexecuted IR parent's distance.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| !case.expected.glyphs.is_empty()),
        |case| &case.id,
        |case| {
            let content = load(case);
            let rendered = mant_render::render_query_man(&content);
            for (label, expected) in &case.expected.glyphs {
                let matches = rendered
                    .lines()
                    .flat_map(|row| {
                        matches_word(row, label)
                            .into_iter()
                            .map(move |start| mant_ir::geometry::text_width(&row[..start]))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    matches,
                    [usize::try_from(*expected).unwrap()],
                    "{}: actual {label} glyph cell, not marker indentation",
                    case.id
                );
            }
        },
    );
}

#[test]
fn one_native_table_keeps_its_spans_and_authored_owner() {
    // Both native dispatchers call term_newln only for the first tbl span.
    // tbl_term.c consumes its own columns thereafter; an old parent word
    // must not become an invisible row between spans or another Table.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| !case.expected.table_owners.is_empty()),
        |case| &case.id,
        |case| tables::assert_tables(&load(case), case),
    );
}

#[derive(Default)]
struct PrefixOwners(Vec<(mant_ir::ListKind, String)>);

impl<'a> Visit<'a> for PrefixOwners {
    fn visit_block(&mut self, block: &'a mant_ir::Block) {
        if let mant_ir::Block::List { kind, items, .. } = block {
            for item in items {
                for block in &item.blocks {
                    if let mant_ir::Block::Paragraph { children, .. } = block {
                        let text = mant_ir::inline_plain_text(children);
                        if text.contains("BEFORE") {
                            self.0.push((*kind, text));
                        }
                    }
                }
            }
        }
        mant_ir::visit::walk_block(self, block);
    }
}

#[test]
fn direct_prefix_keeps_its_owner_and_relative_display_origin() {
    // Fresh pristine UTF-8 receipts are sealed with the exact source. Native
    // termp_it_pre places DELIMO before its generated marker; semantic ListKind
    // retains marker-first presentation. This narrow allowed difference cannot
    // change authored prefix ownership, display +6, or AFTER/TAIL origins.
    for case in fixture::cases()
        .into_iter()
        .filter(|case| !case.expected.authored_prefix.is_empty())
    {
        let mut content = load(&case);
        let original = mant_render::render_query_man(&content);
        for label in ["DISPLAY", "AFTER", "TAIL"] {
            let (row, start) = original
                .lines()
                .find_map(|row| matches_word(row, label).first().map(|start| (row, *start)))
                .unwrap();
            assert_eq!(
                i32::try_from(mant_ir::geometry::text_width(&row[..start])).unwrap(),
                case.expected.review_glyphs[label],
                "{}: {label}",
                case.id
            );
        }
        for round in 0..=2 {
            let rendered = mant_render::render_query_man(&content);
            assert_eq!(
                rendered.matches(&case.expected.authored_prefix).count(),
                1,
                "{} round {round}",
                case.id
            );
            let mut owners = PrefixOwners::default();
            owners.visit_document(content.document.as_ref().unwrap());
            assert_eq!(owners.0.len(), 1, "{}: one original item owner", case.id);
            assert_eq!(owners.0[0].1, case.expected.authored_prefix);
            assert_eq!(
                matches!(owners.0[0].0, mant_ir::ListKind::Ordered { .. }),
                case.id.contains("enum")
            );
            let positions =
                ["BEFORE", "DISPLAY", "AFTER", "TAIL"].map(|label| rendered.find(label).unwrap());
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
}

#[derive(Default)]
struct Links(Vec<(mant_ir::LinkTarget, String)>);

impl Visit<'_> for Links {
    fn visit_inline(&mut self, inline: &mant_ir::Inline) {
        if let mant_ir::Inline::Link {
            target, children, ..
        } = inline
        {
            self.0
                .push((target.clone(), mant_ir::inline_plain_text(children)));
        }
        mant_ir::visit::walk_inline(self, inline);
    }
}

#[test]
fn active_man_link_labels_keep_their_original_target_across_structures() {
    // Exact UR/MT + PP + tbl/EX sources ran pristine profiles first.
    // man_term.c::pre_UR/post_UR execute the BODY normally; semantic link
    // ownership must not borrow a later structural owner's glyphs.
    for case in fixture::cases()
        .into_iter()
        .filter(|case| !case.expected.links.is_empty())
    {
        let content = load(&case);
        let mut links = Links::default();
        links.visit_document(content.document.as_ref().unwrap());
        for expected in &case.expected.links {
            let actual = links
                .0
                .iter()
                .filter(|(target, label)| {
                    let destination = match target {
                        mant_ir::LinkTarget::External { uri } if expected.kind == "external" => {
                            Some(uri)
                        }
                        mant_ir::LinkTarget::Email { address } if expected.kind == "email" => {
                            Some(address)
                        }
                        _ => None,
                    };
                    destination == Some(&expected.destination) && label == &expected.label
                })
                .count();
            assert_eq!(actual, 1, "{}: original label and typed target", case.id);
        }
    }
}
