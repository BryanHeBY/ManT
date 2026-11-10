//! Original regressions reduced from the full-corpus vimtutor spacing finding.
use mant_ir::{Block, ResolvedContent};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

const ROW_HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";
const ROW_END: &str = ".SH END\nENDMARK\n";

fn load(body: &str) -> ResolvedContent {
    let query = load_roff_bytes(format!(".TH PROBE 1\n.SH TEST\n{body}\n").as_bytes()).unwrap();
    assert_eq!(
        mant_ir::validate_document(query.document.as_ref().unwrap()).len(),
        0
    );
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

fn hanging_bodies(node: &libmandoc_rs::Node) -> impl Iterator<Item = &libmandoc_rs::Node> {
    use libmandoc_rs::{MacroToken, ManMacro, NodeKind};

    let mut nodes = vec![node];
    std::iter::from_fn(move || {
        while let Some(node) = nodes.pop() {
            nodes.extend(node.children.iter().rev());
            if node.kind == NodeKind::Body
                && node.macro_token == Some(MacroToken::Man(ManMacro::Hp))
            {
                return Some(node);
            }
        }
        None
    })
}

#[test]
fn empty_hanging_paragraph_posts_consume_spacing_debt_one_row_at_a_time() {
    // Every exact source ran pinned CVS ASCII, UTF-8, HTML, tree and lint.
    // post_HP executes term_vspace() only when the BODY child is NULL;
    // term.c:489-497 consumes skipvsp before an actual blank endline.
    for (open, close) in [("", ""), (".nf\n", ".fi\n")] {
        for debt in 0_usize..=3 {
            let skip = if debt == 0 {
                String::new()
            } else {
                format!(".sp -{debt}\n")
            };
            for count in 1_usize..=4 {
                let source = format!(
                    "{ROW_HEADER}.PD 0\n{open}BEFOREMARK\n{skip}{}.PP\nAFTERMARK\n{close}{ROW_END}",
                    ".HP 16\n".repeat(count)
                );
                let query = load_roff_bytes(source.as_bytes()).unwrap();
                let native = libmandoc_rs::Parser::default()
                    .parse_bytes("check.1", source.as_bytes())
                    .unwrap();
                let bodies: Vec<_> = hanging_bodies(&native.document.root).collect();
                assert_eq!(bodies.len(), count, "{source}");
                assert!(
                    bodies.iter().all(|body| body.children.is_empty()),
                    "{source}"
                );
                let plain = mant_render::render_query_man(&query);
                assert_eq!(
                    plain.split_once("\nDESCRIPTION\n").unwrap().1,
                    format!(
                        "BEFOREMARK{}AFTERMARK\nEND\nENDMARK",
                        "\n".repeat(1 + count.saturating_sub(debt))
                    ),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn invisible_hanging_paragraph_children_are_not_empty_bodies() {
    // Pinned CVS was run before defining each expectation. \& buffers an
    // invisible cell, bare \z survives an empty term_newln(), and .B ""
    // visits an empty TEXT. None is post_HP's absent-child case.
    for (filled, open, close) in [(true, "", ""), (false, ".nf\n", ".fi\n")] {
        for (child, expected) in [
            ("\\&\n", if filled { "AFTERMARK" } else { "\nAFTERMARK" }),
            ("\\z\n", "FTERMARK"),
            (".B \"\"\n", "\nAFTERMARK"),
            ("BODYWORD\n", "BODYWORD\nAFTERMARK"),
            (".ft B\nBODYWORD\n", "BODYWORD\nAFTERMARK"),
        ] {
            let source = format!(
                "{ROW_HEADER}.PD 0\n{open}BEFOREMARK\n.HP 16\n{child}.PP\nAFTERMARK\n{close}{ROW_END}"
            );
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("check.1", source.as_bytes())
                .unwrap();
            let body = hanging_bodies(&native.document.root).next().unwrap();
            assert!(!body.children.is_empty(), "{source}");
            if child.starts_with(".ft") {
                assert_eq!(body.children.len(), 2, "{source}");
                assert_eq!(
                    body.children[0].macro_token,
                    Some(libmandoc_rs::MacroToken::Roff(libmandoc_rs::RoffMacro::Ft))
                );
            }
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let plain = mant_render::render_query_man(&query);
            assert_eq!(
                plain.split_once("\nDESCRIPTION\n").unwrap().1,
                format!("BEFOREMARK\n{expected}\nEND\nENDMARK"),
                "{source}"
            );
        }
    }
}

#[test]
fn section_spacing_consumes_remaining_debt_after_an_empty_hanging_post() {
    // In these exact pinned cases one negative sp unit survives the HP's
    // print_bvspace and its empty BODY post. SH and SS each run the same
    // native term_vspace loop before visiting their heading.
    for (open, close) in [("", ""), (".nf\n", ".fi\n")] {
        for (heading, origin) in [("SH", ""), ("SS", "  ")] {
            let source = format!(
                "{ROW_HEADER}.PD 1\n{open}BEFOREMARK\n.sp -3\n.HP 16\n.{heading} NEXT\nAFTERMARK\n{close}{ROW_END}"
            );
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let plain = mant_render::render_query_man(&query);
            assert_eq!(
                plain.split_once("\nDESCRIPTION\n").unwrap().1,
                format!("BEFOREMARK\n{origin}NEXT\n{origin}AFTERMARK\n\nEND\nENDMARK"),
                "{source}"
            );
        }
    }
}
