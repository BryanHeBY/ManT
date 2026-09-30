//! MP family (review §25.4, NF05): the `.Fd` macro post across every
//! shared execution entry.
//!
//! Upstream `mdoc_term.c::termp_fd_post` is a plain `term_newln(p)` — it
//! is not conditional on filled paragraphs. The oracle cases pin:
//!
//! * `mp01` — control: filled DESCRIPTION `.Fd` keeps Strong + post;
//! * `mp02`/`mp03` — no-fill and `Bd -literal` still settle the post
//!   (`A\c` must not merge with `B` into `AB`);
//! * `mp04`/`mp05` — controls: plain `.Fd A` breaks, `.Cd "A\c"` does
//!   NOT take the Fd post (`AB` stays);
//! * `mp06`/`mp07_*` — definition HEAD/BODY across tag/hang/inset/diag
//!   and a column cell (`NObreak` settles the interval, no hard row);
//! * `mp08_*` — SYNOPSIS declaration machinery around `.Fd`;
//! * `mp09_*` — empty/`\&`/`\z`/`\zX`/`\p`/repeated `\p`/font operands;
//! * `mp10_*` — Fd next to links and `.Xc` closers.
//!
//! Fd's post runs in the common inline node exit, including the native
//! column field execution contract. Every pin is now a required pass.

use super::{MatrixRun, case_names};
const KNOWN_RED: &[&str] = &[];

#[test]
fn mp_matrix_matches_the_pinned_reference() {
    let mut matrix = MatrixRun::new();
    for name in case_names("mp", 22) {
        matrix.evaluate(&name, KNOWN_RED, None);
    }
    matrix.finish(KNOWN_RED, "MP");
}
