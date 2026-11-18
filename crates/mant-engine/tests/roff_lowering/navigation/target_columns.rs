//! Distinct native targets in an active column field.

use super::*;

const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd review sample\n.Sh DESCRIPTION\n";

fn lower(body: &str) -> ResolvedContent {
    let document = mant_loader::load_roff_bytes(format!("{HEADER}{body}").as_bytes()).unwrap();
    json_roundtrip::json_string_roundtrip(&document)
}

#[derive(Debug, PartialEq, Eq)]
enum Event<'a> {
    Text(&'a str),
    Newline,
    Target(&'a str, u32, &'a str),
}

fn field_events(blocks: &[Block]) -> Vec<Event<'_>> {
    struct Events<'a>(Vec<Event<'a>>);
    impl<'a> Visit<'a> for Events<'a> {
        fn visit_inline(&mut self, inline: &'a Inline) {
            match inline {
                Inline::Text { value } | Inline::Code { value } => {
                    if !value.is_empty() {
                        self.0.push(Event::Text(value));
                    }
                }
                Inline::LineBreak { .. } => self.0.push(Event::Newline),
                Inline::Anchor {
                    id,
                    owner_source,
                    fragment_aliases,
                } => {
                    self.0.push(Event::Target(
                        id.as_str(),
                        owner_source.unwrap().line,
                        fragment_aliases[0].as_str(),
                    ));
                }
                _ => visit::walk_inline(self, inline),
            }
        }
    }
    let mut events = Events(Vec::new());
    for block in blocks {
        events.visit_block(block);
    }
    events.0
}

fn cells(document: &mant_ir::Document) -> &[mant_ir::TableCell] {
    let Block::Table { rows, .. } = &document.sections[1].blocks[0] else {
        panic!("expected column table");
    };
    &rows[0].cells
}

fn check_markdown(query: &ResolvedContent, markers: &[&str], omitted: &[&str]) {
    let markdown = render_markdown_with_options(query, MarkdownOptions::ADDRESSABLE);
    for marker in markers {
        let anchor = format!("<a id=\"{marker}\"></a>");
        assert_eq!(markdown.matches(&anchor).count(), 1, "{markdown}");
        assert!(markdown.find(&anchor).unwrap() < markdown.find("```").unwrap());
    }
    for marker in omitted {
        assert!(
            !markdown.contains(&format!("<a id=\"{marker}\"></a>")),
            "{markdown}"
        );
    }
}

fn check_duplicate(document: &mant_ir::Document) {
    check_ambiguous(document, "Mixed.Target", ["mixed-target", "mixed-target-2"]);
}

fn check_ambiguous(document: &mant_ir::Document, alias: &str, ids: [&str; 2]) {
    let index = DocumentIndex::build(document);
    for id in ids {
        assert_eq!(index.fragment_target(id).map(NodeId::as_str), Some(id));
    }
    assert!(index.fragment_target(alias).is_none());
    assert!(index.is_fragment_ambiguous(alias));
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_deref() == Some("ir.ambiguous-fragment-alias"))
            .count(),
        1
    );
}

#[test]
fn repeated_normalized_spelling_remains_only_an_ambiguous_alias() {
    // CVS mdoc_d1_pre() and mdoc_pp_pre() emit independent authored
    // destinations, including the exact alias that resembles a suffix.
    for (prefix, suffix) in [
        (".Bl -column a b\n.It FIRST Ta SECOND\n", ".El\n"),
        (".No BEFORE\n", ""),
    ] {
        let first = ".Tg mixed-target\n.D1 TARGETONE\n";
        let query = lower(&format!("{prefix}{first}{suffix}"));
        let document = query.document.as_ref().unwrap();
        let index = DocumentIndex::build(document);
        assert_eq!(
            index.fragment_target("mixed-target").map(NodeId::as_str),
            Some("mixed-target")
        );

        let second = ".Tg mixed-target\n.Dl TARGETTWO\n";
        for already_reserved in ["", ".Tg mixed-target-2\n.Pp\nANOTHER\n"] {
            let query = lower(&format!(
                "{prefix}{first}{second}{already_reserved}{suffix}"
            ));
            let document = query.document.as_ref().unwrap();
            let ids = if already_reserved.is_empty() {
                ["mixed-target-3", "mixed-target-2"]
            } else {
                ["mixed-target-4", "mixed-target-3"]
            };
            check_ambiguous(document, "mixed-target", ids);
            let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
            for id in ids {
                assert_eq!(markdown.matches(&format!("<a id=\"{id}\"></a>")).count(), 1);
            }
            assert!(!markdown.contains("<a id=\"mixed-target\"></a>"));
            if !already_reserved.is_empty() {
                let index = DocumentIndex::build(document);
                assert_eq!(
                    index.fragment_target("mixed-target-2").map(NodeId::as_str),
                    Some("mixed-target-2")
                );
                assert_eq!(markdown.matches("<a id=\"mixed-target-2\"></a>").count(), 1);
            }
        }
    }
}

#[test]
fn column_targets_stay_after_the_native_pre_and_before_the_next_cell() {
    // CVS termp_pp_pre tags after term_vspace(). D1/Dl and Bd use a BODY
    // owner after their own native pre. Ta on its own line intentionally
    // exercises the native next-cell transition and its existing warning.
    use Event::{Newline, Target, Text};
    let original = lower(".Bl -column Ds\n.It first\n.Tg Mixed.Target\n.Pp\nMARK\n.El\n");
    assert_eq!(
        field_events(&cells(original.document.as_ref().unwrap())[0].blocks),
        [
            Text("first"),
            Newline,
            Newline,
            Target("mixed-target", 11, "Mixed.Target"),
            Text("MARK")
        ]
    );

    for (payload, empty_breaks, occupied_breaks) in [
        (".Pp\nTARGETONE", 1, 2),
        (".D1 TARGETONE", 0, 1),
        (".Dl TARGETONE", 0, 1),
        (".Bd -literal\nTARGETONE\n.Ed", 0, 2),
    ] {
        for prior in [false, true] {
            let first = if prior { "FIRST" } else { "\\&" };
            for (widths, row, slot) in [
                ("a b", format!(".It {first}"), 0),
                ("a b c", format!(".It OTHER Ta {first}"), 1),
            ] {
                let source = format!(
                    ".Bl -column {widths}\n{row}\n.Tg Mixed.Target\n{payload}\n.Ta THIRD\n.El\n"
                );
                let query = lower(&source);
                let document = query.document.as_ref().unwrap();
                let mut expected = Vec::new();
                if prior {
                    expected.push(Text("FIRST"));
                }
                expected.extend(
                    (0..if prior { occupied_breaks } else { empty_breaks }).map(|_| Newline),
                );
                expected.extend([
                    Target("mixed-target", 11, "Mixed.Target"),
                    Text("TARGETONE"),
                ]);
                let all = cells(document);
                assert_eq!(field_events(&all[slot].blocks), expected, "{source}");
                assert_eq!(
                    field_events(&all[slot + 1].blocks),
                    [Text("THIRD")],
                    "{source}"
                );
                if slot == 1 {
                    assert_eq!(field_events(&all[0].blocks), [Text("OTHER")]);
                }
                anchor_line(document, "Mixed.Target", 11);
                check_markdown(&query, &["mixed-target", "Mixed.Target"], &[]);
            }
        }
    }
}

#[test]
fn separate_column_display_owners_survive_same_spelling_and_macro_coordinates() {
    // Pinned CVS has separate D1 and Dl BODY destinations, even when both
    // label spellings and native source coordinates coincide after XX.
    use Event::{Newline, Target, Text};
    for (slot, row, before, tail) in [
        (0, ".It FIRST", "FIRST", ".Ta THIRD\n"),
        (1, ".It OTHER Ta SECOND", "SECOND", ""),
    ] {
        for same in [false, true] {
            let (alias, id) = if same {
                ("Mixed.Target", "mixed-target-2")
            } else {
                ("Second.Target", "second-target")
            };
            let request = format!(".Tg Mixed.Target\n.D1 TARGETONE\n.Tg {alias}\n.Dl TARGETTWO\n");
            for (print, first_line, second_line) in [
                (request.clone(), 11, 13),
                (format!(".de XX\n{request}..\n.XX\n"), 16, 16),
            ] {
                let query = lower(&format!(".Bl -column a b\n{row}\n{print}{tail}.El\n"));
                let document = query.document.as_ref().unwrap();
                assert_eq!(
                    field_events(&cells(document)[slot].blocks),
                    [
                        Text(before),
                        Newline,
                        Target("mixed-target", first_line, "Mixed.Target"),
                        Text("TARGETONE"),
                        Newline,
                        Target(id, second_line, alias),
                        Text("TARGETTWO")
                    ]
                );
                if same {
                    check_duplicate(document);
                    check_markdown(&query, &["mixed-target", id], &[alias]);
                } else {
                    anchor_line(document, "Mixed.Target", first_line);
                    anchor_line(document, alias, second_line);
                    check_markdown(&query, &["mixed-target", id, "Mixed.Target", alias], &[]);
                }
            }
        }
    }
}

#[test]
fn column_display_body_target_follows_punctuation_outside_the_body() {
    // CVS mdoc_d1_pre() opens the tagged div at BODY; opening and closing
    // delimiters can be direct BLOCK children outside the div.
    use Event::{Newline, Target, Text};
    for display in ["D1", "Dl"] {
        let query = lower(&format!(
            ".Bl -column a b\n.It FIRST Ta SECOND\n.Tg Mixed.Target\n.{display} ( TARGETONE )\n.El\n"
        ));
        let document = query.document.as_ref().unwrap();
        assert_eq!(
            field_events(&cells(document)[1].blocks),
            [
                Text("SECOND"),
                Newline,
                Text("("),
                Target("mixed-target", 11, "Mixed.Target"),
                Text("TARGETONE"),
                Text(")")
            ],
            "{display}"
        );
    }
}

#[test]
fn native_column_row_and_cell_targets_keep_separate_owners() {
    // post_tg() can directly tag the column It, and tag_move_id() can
    // relocate an inline tag onto the same TR. Its first-cell position is
    // independent of a genuine Pp or display target in a cell.
    use Event::{Newline, Target, Text};
    let query = lower(
        ".Bl -column a b\n.Tg Outer.Target\n.It FIRST Ta SECOND\n.Tg Mixed.Target\n.Pp\nTARGETONE\n.El\n",
    );
    let document = query.document.as_ref().unwrap();
    let fields = cells(document);
    assert_eq!(
        field_events(&fields[0].blocks),
        [Target("outer-target", 10, "Outer.Target"), Text("FIRST")]
    );
    assert_eq!(
        field_events(&fields[1].blocks),
        [
            Text("SECOND"),
            Newline,
            Newline,
            Target("mixed-target", 12, "Mixed.Target"),
            Text("TARGETONE")
        ]
    );

    let query = lower(".Bl -column a b\n.It FIRST Ta SECOND\n.Tg Outer.Target\n.Ox 7.6\n.El\n");
    let document = query.document.as_ref().unwrap();
    let fields = cells(document);
    assert_eq!(
        field_events(&fields[0].blocks),
        [Target("outer-target", 9, "Outer.Target"), Text("FIRST")]
    );
    assert!(
        field_events(&fields[1].blocks)
            .iter()
            .all(|event| !matches!(event, Target(..)))
    );

    let query =
        lower(".Bl -column a\n.Tg Mixed.Target\n.It FIRST\n.Tg Mixed.Target\n.D1 TARGETONE\n.El\n");
    let document = query.document.as_ref().unwrap();
    assert_eq!(
        field_events(&cells(document)[0].blocks),
        [
            Target("mixed-target", 10, "Mixed.Target"),
            Text("FIRST"),
            Newline,
            Target("mixed-target-2", 12, "Mixed.Target"),
            Text("TARGETONE")
        ]
    );
    check_duplicate(document);

    let query = lower(
        ".Bl -column a b\n.It FIRST Ta SECOND\n.Tg Mixed.Target\n.Pp\nTARGETONE\n.Tg Mixed.Target\n.Bd -literal\nTARGETTWO\n.Ed\n.El\n",
    );
    let document = query.document.as_ref().unwrap();
    assert_eq!(
        field_events(&cells(document)[1].blocks),
        [
            Text("SECOND"),
            Newline,
            Newline,
            Target("mixed-target", 11, "Mixed.Target"),
            Text("TARGETONE"),
            Newline,
            Newline,
            Target("mixed-target-2", 14, "Mixed.Target"),
            Text("TARGETTWO")
        ]
    );
    check_duplicate(document);
}

#[test]
fn column_anchor_rules_leave_other_output_destinations_independent() {
    for body in [
        ".No BEFORE\n.Tg Mixed.Target\n.D1 TARGETONE\n.Tg Mixed.Target\n.Dl TARGETTWO\n",
        ".Bl -bullet\n.It\nBEFORE\n.Tg Mixed.Target\n.D1 TARGETONE\n.Tg Mixed.Target\n.Dl TARGETTWO\n.El\n",
        ".Bd -literal\nBEFORE\n.Tg Mixed.Target\n.D1 TARGETONE\n.Tg Mixed.Target\n.Dl TARGETTWO\n.Ed\n",
        ".Bl -tag -width Ds\n.It Xo\nBEFORE\n.Tg Mixed.Target\n.D1 TARGETONE\n.Tg Mixed.Target\n.Dl TARGETTWO\n.Xc\nBODY.\n.El\n",
    ] {
        let query = lower(body);
        let document = query.document.as_ref().unwrap();
        check_duplicate(document);
        let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
        for label in ["BEFORE", "TARGETONE", "TARGETTWO"] {
            assert!(markdown.contains(label), "{body}: {markdown}");
        }
    }
}
