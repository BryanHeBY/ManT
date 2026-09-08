//! A transparent RS keeps paragraph predecessor evidence across IR ownership.

use mant_engine::{query_roff_bytes, render_query_text};
use mant_ir::{Block, Inline, LayoutHint};

fn paragraph_layout<'a>(blocks: &'a [Block], text: &str) -> Option<&'a LayoutHint> {
    for block in blocks {
        match block {
            Block::Paragraph {
                children, layout, ..
            } if children
                .iter()
                .any(|inline| matches!(inline, Inline::Text { value } if value.contains(text))) =>
            {
                return Some(layout);
            }
            Block::List { items, .. } => {
                for item in items {
                    if let Some(layout) = paragraph_layout(&item.blocks, text) {
                        return Some(layout);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn check(source: &str, expected: &[(&str, u16)]) {
    let query = query_roff_bytes(source.as_bytes()).unwrap();
    let document = query.document.as_ref().unwrap();
    let text = render_query_text(&query);
    if source.contains(".IP 1.") {
        let Some(Block::List { items, .. }) = document.sections[0].blocks.first() else {
            panic!("expected an ordered owner: {document:#?}")
        };
        assert_eq!(items.len(), 1);
        for &(token, _) in expected {
            assert!(
                paragraph_layout(&items[0].blocks, token).is_some(),
                "continuation escaped its item: {document:#?}"
            );
        }
    }
    for &(token, gap) in expected {
        let layout = paragraph_layout(&document.sections[0].blocks, token)
            .unwrap_or_else(|| panic!("missing {token}: {document:#?}"));
        assert_eq!(layout.spacing_before_lines, gap, "{source}\n{text}");
        let rows: Vec<_> = text.lines().collect();
        let index = rows.iter().position(|line| line.trim() == token).unwrap();
        let blanks = rows[..index]
            .iter()
            .rev()
            .take_while(|line| line.is_empty())
            .count();
        assert_eq!(blanks, usize::from(gap), "{source}\n{text}");
    }
}

#[test]
fn ordered_item_relative_scope_applies_pd_only_for_an_explicit_paragraph() {
    // mandoc print_bvspace climbs transparent RS ancestors; it does not
    // require the predecessor to share the temporary IR output buffer.
    for pd in [0, 2] {
        for paragraph in ["", ".PP\n"] {
            let source = format!(
                ".TH PROBE 1\n.SH DESCRIPTION\n.PD {pd}\n.IP 1. 4\nFIRST\n.RS 4\n{paragraph}SECOND\n.RE\n.RS 4\n{paragraph}THIRD\n.RE\n"
            );
            let gap = if paragraph.is_empty() { 0 } else { pd };
            check(&source, &[("SECOND", gap), ("THIRD", gap)]);
        }
    }
}

#[test]
fn nested_relative_wrappers_preserve_the_ordered_item_predecessor() {
    for pd in [0, 2] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.PD {pd}\n.IP 1. 4\nFIRST\n.RS 4\n.RS 2\n.PP\nSECOND\n.RE\n.RE\n"
        );
        check(&source, &[("SECOND", pd)]);
    }
}

#[test]
fn ordinary_relative_scopes_retain_the_same_paragraph_boundary_contract() {
    for pd in [0, 2] {
        for paragraph in ["", ".PP\n"] {
            let source = format!(
                ".TH PROBE 1\n.SH DESCRIPTION\n.PD {pd}\nFIRST\n.RS 4\n{paragraph}SECOND\n.RE\n.RS 4\n{paragraph}THIRD\n.RE\n"
            );
            let gap = if paragraph.is_empty() { 0 } else { pd };
            check(&source, &[("SECOND", gap), ("THIRD", gap)]);
        }
    }
}

#[test]
fn a_first_paragraph_in_a_first_relative_scope_does_not_invent_a_predecessor() {
    let source = ".TH PROBE 1\n.SH DESCRIPTION\n.PD 2\n.RS 4\n.RS 2\n.PP\nSECOND\n.RE\n.RE\n";
    let query = query_roff_bytes(source.as_bytes()).unwrap();
    let document = query.document.as_ref().unwrap();
    assert_eq!(
        paragraph_layout(&document.sections[0].blocks, "SECOND")
            .unwrap()
            .spacing_before_lines,
        0
    );
    // The heading owns its own single blank row; this first paragraph must
    // not introduce a PD=2 boundary on top of that heading presentation.
    assert!(render_query_text(&query).contains("DESCRIPTION\n\n      SECOND"));
}
