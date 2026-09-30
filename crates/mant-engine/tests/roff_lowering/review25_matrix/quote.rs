//! AQ family (review §25.4, NF06): quote-enclosure topology.
//!
//! The eight base forms (AQ01–AQ04: plain word, sole `.Mt`, `.Mt` plus a
//! trailing macro sibling, explicit `.Ao`/`.Ac` pairs, multiple `.Mt`)
//! are already pinned by the codec-level contract
//! `basic_inline::quote_enclosure_angle_marks_follow_the_sole_mt_child_topology`
//! and are deliberately NOT re-recorded here. This matrix records the
//! remaining review shapes against the pristine oracle:
//!
//! * `aq05` — invisible sibling (`No \&`) still breaks the sole-Mt
//!   topology (CVS keeps the math angle glyphs);
//! * `aq06_*` — sibling before `.Mt`, empty `.Aq`, empty `.Ao`/`.Ac`;
//! * `aq07` — one `.Mt` child carrying two addresses still takes the
//!   ASCII mail pair (child count, not address count);
//! * `aq08_*` — nested/cross-closed enclosures each deciding their own
//!   topology, and `\z`/`\p`/`\c` prefixes before `.Mt`.
//!
//! Upstream: `mdoc_term.c::termp_quote_pre/post` decide on the parsed
//! child list (`n->child && n->child->next == NULL &&
//! n->child->tok == MDOC_Mt`), never on visible text.

use super::{MatrixRun, case_names};

/// Cases whose layer-1 projection is still red against the oracle pin.
const KNOWN_RED: &[&str] = &[];

#[test]
fn aq_matrix_matches_the_pinned_reference() {
    let mut matrix = MatrixRun::new();
    for name in case_names("aq", 10) {
        matrix.evaluate(&name, KNOWN_RED, None);
    }
    matrix.finish(KNOWN_RED, "AQ");
}
