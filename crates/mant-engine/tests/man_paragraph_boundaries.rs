//! All man paragraph forms resolve native predecessor evidence before IR emission.
use mant_engine::{query_roff_bytes, render_excerpt_text, render_query_text, select_excerpt};
use mant_ir::Block;

const PARAGRAPHS: &[&str] = &[
    ".PP\nAFTER\n",
    ".HP 4\nAFTER\n",
    ".TP 4\nx\nAFTER\n",
    ".IP x 4\nAFTER\n",
    ".IP 1. 4\nAFTER\n",
    ".IP \\(bu 4\nAFTER\n",
];

fn blank_rows_before(text: &str, token: &str) -> usize {
    let rows: Vec<_> = text.lines().collect();
    let index = rows.iter().position(|line| line.contains(token)).unwrap();
    rows[..index]
        .iter()
        .rev()
        .take_while(|line| line.is_empty())
        .count()
}

#[test]
fn first_and_preceded_paragraphs_use_one_boundary_rule_for_every_man_form() {
    for pd in [0, 2] {
        for prefix in ["", "BEFORE\n"] {
            for body in PARAGRAPHS {
                for (open, close) in [("", ""), (".RS 4\n.RS 2\n", ".RE\n.RE\n")] {
                    let source = format!(
                        ".TH BOUNDARY 1\n.SH DESCRIPTION\n.PD {pd}\n{prefix}{open}{body}{close}"
                    );
                    let query = query_roff_bytes(source.as_bytes()).unwrap();
                    let text = render_query_text(&query);
                    // A first paragraph has no PD boundary; the section
                    // facade must not invent one after its heading either.
                    let gap = if prefix.is_empty() { 0 } else { pd };
                    assert_eq!(blank_rows_before(&text, "AFTER"), gap, "{source}\n{text}");
                }
            }
        }
    }
}

#[test]
fn detached_ordered_continuations_keep_pd_for_every_first_paragraph_form() {
    for pd in [0, 2] {
        for body in PARAGRAPHS {
            for (open, close) in [(".RS 4\n", ".RE\n"), (".RS 4\n.RS 2\n", ".RE\n.RE\n")] {
                let source = format!(
                    ".TH BOUNDARY 1\n.SH DESCRIPTION\n.PD {pd}\n.IP 1. 4\nBEFORE\n{open}{body}{close}.PP\nOUTSIDE\n"
                );
                let query = query_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&query);
                assert_eq!(blank_rows_before(&text, "AFTER"), pd, "{source}\n{text}");
                let document = query.document.as_ref().unwrap();
                let Some(Block::List { items, .. }) = document.sections[0].blocks.first() else {
                    panic!("missing ordered owner: {document:#?}");
                };
                assert_eq!(items.len(), 1);
                // Inspect the retained original item subtree through the same
                // renderer; moving the boundary must not detach its content.
                let mut owned = query.clone();
                owned.document.as_mut().unwrap().sections[0].blocks = items[0].blocks.clone();
                let owned_text = render_query_text(&owned);
                assert!(owned_text.contains("AFTER"), "{owned_text}");
                assert!(!owned_text.contains("OUTSIDE"), "{owned_text}");
            }
        }
    }
}

#[test]
fn tq_stays_zero_distance_and_nested_entry_targets_remain_addressable() {
    let query = query_roff_bytes(b".TH BOUNDARY 1\n.SH OPTIONS\n.PD 2\n.IP 1. 4\nBEFORE\n.RS 4\n.TP 20\n.B --first\n.TQ\n.B --second\nPAYLOAD\n.RE\n.PP\nOUTSIDE\n").unwrap();
    let text = render_query_text(&query);
    assert_eq!(blank_rows_before(&text, "--first"), 2, "{text}");
    assert_eq!(blank_rows_before(&text, "--second"), 0, "{text}");
    for selector in ["--first", "--second"] {
        let excerpt = select_excerpt(&query, &[selector]).unwrap();
        let excerpt_text = render_excerpt_text(&excerpt);
        assert!(excerpt_text.contains("PAYLOAD"), "{excerpt_text}");
        assert!(!excerpt_text.contains("OUTSIDE"), "{excerpt_text}");
    }
}

#[test]
fn merged_man_lists_keep_each_resolved_gap_without_a_container_copy() {
    for markers in [["1.", "2.", "3."], ["\\(bu", "\\(bu", "\\(bu"]] {
        let source = format!(
            ".TH BOUNDARY 1\n.SH DESCRIPTION\n.PD 2\n.IP {} 4\nFIRST\n.PD 0\n.IP {} 4\nSECOND\n.PD 2\n.IP {} 4\nTHIRD\n",
            markers[0], markers[1], markers[2]
        );
        let query = query_roff_bytes(source.as_bytes()).unwrap();
        let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
        let [Block::List { items, layout, .. }] = blocks.as_slice() else {
            panic!("list grouping changed: {blocks:#?}");
        };
        assert_eq!(layout.spacing_before_lines, 0);
        assert_eq!(items.len(), 3);
        for (item, gap) in items.iter().zip([0, 0, 2]) {
            assert_eq!(item.layout.spacing_before_lines, Some(gap));
        }
        let text = render_query_text(&query);
        assert_eq!(blank_rows_before(&text, "FIRST"), 0, "{text}");
        assert_eq!(blank_rows_before(&text, "SECOND"), 0, "{text}");
        assert_eq!(blank_rows_before(&text, "THIRD"), 2, "{text}");
    }
}

#[test]
fn headless_continuations_keep_pd_and_body_space_as_independent_requests() {
    for pd in [0, 1] {
        for space in [0, 2] {
            for head in [".IP \"\" 4", ".IP", ".TP 4\n\\&", ".TP 4\n.B \"\""] {
                let request = if space == 0 {
                    String::new()
                } else {
                    ".sp 2\n".into()
                };
                let source = format!(
                    ".TH BOUNDARY 1\n.SH OPTIONS\n.PD {pd}\n.IP --owner 12\nFIRST\n{head}\n{request}SECOND\n"
                );
                let query = query_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&query);
                // An invisible tag is not an extra content row. Preserve
                // the authored PD and sp, not a formatter's empty-head flush.
                assert_eq!(
                    blank_rows_before(&text, "SECOND"),
                    pd + space,
                    "{source}\n{text}"
                );
                if head.starts_with(".IP") {
                    let excerpt =
                        render_excerpt_text(&select_excerpt(&query, &["--owner"]).unwrap());
                    assert!(
                        excerpt.contains("FIRST") && excerpt.contains("SECOND"),
                        "{excerpt}"
                    );
                }
            }
        }
    }
}
