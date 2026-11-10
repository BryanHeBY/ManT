use super::{Block, MAN, inline_text, lower};

#[test]
fn a_retained_reading_space_does_not_publish_a_completed_row_twice() {
    // Each full source ran all five pinned profiles. term.c:205–220
    // closes each accepted loop; term_field (389–427) defers the HORIZ
    // advance, so the preserved reading space is not another endline.
    // When a final invisible HEAD shares the last accepted row, the
    // retained literal represents it. A rejected HEAD has its own row.
    for (pd, old, section, title, body, heading, gap) in [
        (0, r"\h'1n'", "SS", "NEXT", "ALPHA\n\n ", " NEXT", 0),
        (0, r"\h'1n'", "SH", "NEXT", "ALPHA\n\n ", "NEXT", 0),
        (1, r"\p\& \h'1n'", "SS", "NEXT", "ALPHA\n\n\n ", "NEXT", 1),
        (0, r"\h'1n'", "SS", r"\&", "ALPHA\n\n ", "", 0),
        (
            0,
            r"\h'1n' BETA",
            "SS",
            r"\p NEXT",
            "ALPHA\n\n  BETA",
            "",
            1,
        ),
    ] {
        let source = format!(
            "{}.nf\nALPHA\\p \\p\\& {old}\n.{section} {title}\n\
             AFTERMARK\n.SH ENDTEST\nFINISH\n",
            MAN.replace(".PD 0", &format!(".PD {pd}")),
        );
        let doc = lower(&source);
        let first = &doc.sections[1];
        assert!(
            matches!(first.blocks.as_slice(), [Block::Preformatted { children, .. }]
            if inline_text(children) == body),
            "{source}: {:?}",
            first.blocks
        );
        let next = first.children.first().unwrap_or(&doc.sections[2]);
        assert_eq!(inline_text(&next.heading.content), heading, "{source}");
        assert_eq!(next.spacing_before_lines, gap, "{source}");
        assert!(
            matches!(next.blocks.as_slice(), [Block::Paragraph { children, .. }]
            if inline_text(children) == "AFTERMARK"),
            "{source}"
        );
    }
}

#[test]
fn a_deferred_tab_keeps_its_spelling_only_in_the_projected_literal() {
    // term_field() (term.c:389–427) does not print a deferred trailing
    // TAB on the device. The no-fill source projection preserves the
    // authored scalar on that committed row. A later scan normalizing
    // other cells must not cause the old projected TAB to be retired.
    let source =
        format!("{MAN}.nf\nALPHA\t \\p\\& BETA\n.SH NEXT\nAFTERMARK\n.SH ENDTEST\nFINISH\n");
    let doc = lower(&source);
    assert!(matches!(doc.sections[1].blocks.as_slice(),
        [Block::Preformatted { children, .. }] if inline_text(children) == "ALPHA\t\nBETA"));

    // In filled text the accepted blank pass never prints its deferred TAB.
    let source = format!("{MAN}ALPHA\\p \\p\\&\t BETA\n.SS NEXT\nAFTERMARK\n.SH ENDTEST\nFINISH\n");
    let doc = lower(&source);
    assert!(matches!(doc.sections[1].blocks.as_slice(),
        [Block::Paragraph { children, .. }] if inline_text(children) == "ALPHA\n\nBETA"));
}
