//! Original regressions reduced from the full-corpus vimtutor spacing finding.
use mant_ir::{Block, ResolvedContent};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn load(body: &str) -> ResolvedContent {
    let query = load_roff_bytes(format!(".TH PROBE 1\n.SH TEST\n{body}\n").as_bytes()).unwrap();
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    query
}

fn gap(query: &ResolvedContent) -> usize {
    let text = render_query_text(query);
    let lines: Vec<_> = text.lines().collect();
    let before = lines
        .iter()
        .position(|line| line.trim() == "ALPHA")
        .unwrap();
    let after = lines.iter().position(|line| line.trim() == "BETA").unwrap();
    assert!(
        lines[before + 1..after]
            .iter()
            .all(|line| line.trim().is_empty()),
        "{text:?}"
    );
    after - before - 1
}

#[test]
fn state_only_paragraph_bodies_preserve_executed_distance_and_source() {
    // Fixed CVS pre_PP/print_bvspace and groff 1.24.1 agree on all 27 cases.
    // Native PP contains only the control; RS is its following sibling.
    for paragraph in ["PP", "P", "LP"] {
        for control in ["nf", "fi", "ft B"] {
            for distance in 0..=2 {
                let query = load(&format!(
                    ".PD {distance}\nALPHA\n.{paragraph}\n.{control}\n.RS\nBETA\n.RE\n.fi"
                ));
                assert_eq!(gap(&query), distance, "{paragraph}/{control}/{distance}");
                let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
                if distance > 0 {
                    assert!(
                        blocks.iter().any(|block| matches!(block,
                            Block::VerticalSpace { lines, source: Some(source) }
                                if usize::from(*lines) == distance && source.line == 5
                        )),
                        "{blocks:#?}"
                    );
                }
            }
        }
    }
}

#[test]
fn skipped_or_initial_paragraphs_do_not_invent_distance() {
    let skipped = load("ALPHA\n.if 0 .PP\n.nf\n.RS\nBETA\n.RE\n.fi");
    assert_eq!(gap(&skipped), 0);
    for paragraph in ["PP", "P", "LP"] {
        let initial = load(&format!(".{paragraph}\n.nf\n.RS\nBETA\n.RE\n.fi"));
        let blocks = &initial.document.as_ref().unwrap().sections[0].blocks;
        assert!(
            !blocks
                .iter()
                .any(|block| matches!(block, Block::VerticalSpace { .. }))
        );
        assert!(blocks.iter().all(|block| {
            mant_ir::geometry::block_layout(block)
                .is_none_or(|layout| layout.spacing_before_lines == 0)
        }));
    }
}

#[test]
fn nested_relative_scopes_keep_the_pending_paragraph_gap() {
    let query = load(".RS 3\nALPHA\n.PP\n.nf\n.RS 4\nBETA\n.RE\n.fi\n.RE\nAFTER");
    assert_eq!(gap(&query), 1);
    let text = render_query_text(&query);
    assert!(text.lines().any(|line| line == "       BETA"), "{text:?}");
    assert!(text.lines().any(|line| line == "AFTER"), "{text:?}");
}
