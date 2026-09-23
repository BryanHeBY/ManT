#![cfg(feature = "annotated")]

use libmandoc_rs::annotated::AnnotatedRenderer;
use libmandoc_rs::{InputFormat, SourceBundle};

fn heading_names(input: &[u8], format: InputFormat) -> Vec<Option<String>> {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", input.to_vec()).unwrap();
    AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle, format)
        .unwrap()
        .marks
        .iter()
        .filter(|mark| mark.kind == 1)
        .map(|mark| mark.name.clone())
        .collect()
}

#[test]
fn authored_heading_phrase_follows_pinned_deroff_leaf_order_and_trim() {
    // Exact input first ran through pinned CVS -Ttree, -Thtml, and -Tutf8.
    // roff.c::deroff() trims the first nonempty leaf, then appends later
    // leaves with %*s (minimum width), retaining their trailing blanks.
    assert_eq!(
        heading_names(
            b".TH T 1\n.SH \" Alpha \" \" Beta  \"\ntext\n",
            InputFormat::Man,
        ),
        vec![Some("Alpha Beta  ".to_owned())]
    );
}

#[test]
fn empty_heading_keeps_absent_authored_phrase() {
    // Exact input first ran through pinned CVS -Ttree. The empty HEAD text
    // is skipped by roff.c::deroff(), not converted to a fabricated name.
    assert_eq!(
        heading_names(b".TH T 1\n.SH \"\"\ntext\n", InputFormat::Man),
        vec![None]
    );
}

#[test]
fn inline_heading_macros_follow_pinned_deroff_tree_order() {
    // Exact input first ran through pinned CVS -Ttree and -Thtml.
    // roff.c::deroff() descends into Em before visiting later siblings.
    assert_eq!(
        heading_names(
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh Alpha Em Beta\ntext\n",
            InputFormat::Mdoc,
        ),
        vec![Some("Alpha Beta".to_owned())]
    );
}

#[test]
fn output_budget_keeps_the_first_failure_and_next_call_recovers() {
    // Exact generated input (80 A bytes in SH) first ran through pinned CVS
    // -Tutf8 -O width=78. man_term.c::print_man_head and term.c::term_field
    // still emit field events after a bounded sink stops accepting bytes.
    // Those events must not replace the sink's original budget failure.
    let title = "A".repeat(80);
    let input = format!(".TH T 1\n.SH {title}\nbody\n");
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", input.into_bytes()).unwrap();
    let limited = AnnotatedRenderer::default()
        .with_max_content_bytes(64)
        .unwrap();
    let error = limited
        .render_bundle("t.1", &bundle, InputFormat::Man)
        .unwrap_err();
    assert_eq!(
        (
            error.status,
            error.stage,
            error.limit_kind,
            error.observed,
            error.allowed
        ),
        (3, 4, 10, 65, 64)
    );
    let recovered = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle, InputFormat::Man)
        .unwrap();
    let heading = recovered.marks.iter().find(|mark| mark.kind == 1).unwrap();
    assert_eq!(heading.name.as_deref(), Some(title.as_str()));
}
