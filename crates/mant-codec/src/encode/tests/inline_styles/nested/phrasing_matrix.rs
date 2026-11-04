//! Finite phrasing corpus, with independent IR0 evidence on both readbacks.
//!
//! The routine suite runs the 13,729 risk configurations; `link_semantics`
//! separately runs all 2,720 real-source metadata/bracket configurations.
//! The full 252,362 corpus is ignored in routine/packaged gates and runs via:
//! `cargo test --locked -p mant-codec --release --lib
//! complete_phrasing_link_matrix_has_252362_original_ir_two_round_cases
//! -- --ignored --nocapture` (one shell command).
//!
//! Statistics distinguish first-read loss from IR0, further loss at read two,
//! and any IR0 loss at either read. The historical 765 counted only further
//! changes between reads; it is not the denominator for all IR0 style loss.
//! Exact text/code/link identity is mandatory in every configuration. Only
//! optional Strong/Emphasis may be reduced, never added or leaked as text.
use super::*;

const WORDS: [&str; 30] = [
    "a",
    "a_b",
    "*",
    "_",
    "!",
    "!!",
    "x!",
    "[",
    "]",
    "\\",
    "`",
    "a`b",
    "a**b",
    "a_b*",
    "a\n# b",
    "a\n\nb",
    "(x)",
    "-x=",
    " a ",
    " ",
    "中",
    "\u{a0}",
    "a\u{a0}",
    "\u{a0}b",
    "\u{2003}x\u{202f}",
    "e\u{301}",
    "👩‍👩‍👧‍👦",
    "\t\n",
    "&amp;",
    "<br>",
];

fn wrap(kind: u8, nodes: Vec<Inline>) -> Inline {
    match kind {
        0 => nodes.into_iter().next().unwrap(),
        1 => styled(true, nodes),
        2 => styled(false, nodes),
        4 => styled(true, vec![styled(false, nodes)]),
        5 => styled(false, vec![styled(true, nodes)]),
        6 => Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://one.test/a%20b%25".into(),
            },
            title: Some("title".into()),
            children: nodes,
        },
        _ => unreachable!(),
    }
}

fn atom(kind: u8, value: &str) -> Inline {
    if kind == 3 {
        Inline::Code {
            value: value.into(),
        }
    } else {
        wrap(kind, vec![text(value)])
    }
}

#[derive(Debug, Default)]
struct MatrixCounts {
    configurations: usize,
    first_loss: usize,
    second_further_loss: usize,
    any_ir0_loss: usize,
}

fn matrix(words: &[&str], include_nested: bool) -> MatrixCounts {
    let mut counts = MatrixCounts::default();
    let mut check = |nodes: &[Inline]| {
        let changes = link_semantics::roundtrip_styles(nodes);
        counts.first_loss += usize::from(changes.first_loss);
        counts.second_further_loss += usize::from(changes.second_further_loss);
        counts.any_ir0_loss += usize::from(changes.any_ir0_loss);
        counts.configurations += 1;
    };
    for source in [r"**_`a`*b\**_**", r"**_[a](https://a.test)*b\**_**"] {
        check(&parsed_children(source));
    }
    for &a in words {
        for &b in words {
            for left in 0..7 {
                for right in 0..7 {
                    check(&[atom(left, a), atom(right, b)]);
                    if !include_nested {
                        continue;
                    }
                    for outer in [1, 2, 4, 5, 6] {
                        // Markdown disallows nested Link wrappers. A generated
                        // Link preceded by a literal bang also has the separate
                        // image-seam matrix and is not part of this old corpus.
                        if outer == 6
                            && (left == 6 || right == 6 || (left == 0 && a.ends_with('!')))
                        {
                            continue;
                        }
                        check(&[wrap(outer, vec![atom(left, a), atom(right, b)])]);
                    }
                }
            }
        }
    }
    counts
}

#[test]
fn phrasing_link_matrix_keeps_marker_whitespace_unicode_and_code_risk_pairs() {
    let counts = matrix(
        &["a", "*", "!", " a ", "a\n\nb", "中", "\u{2003}x\u{202f}"],
        true,
    );
    assert!(counts.configurations > 10_000);
    eprintln!("phrasing risk matrix: {counts:?}");
}

#[test]
#[ignore = "complete finite corpus; run explicitly with --release and --ignored"]
fn complete_phrasing_link_matrix_has_252362_original_ir_two_round_cases() {
    let counts = matrix(&WORDS, true);
    assert_eq!(counts.configurations, 252_362);
    // The old review's 765 counted only IR1 != IR2, not all loss from IR0.
    // Report the three distinct measures rather than comparing unlike totals.
    eprintln!("complete phrasing matrix: {counts:?}");
}
