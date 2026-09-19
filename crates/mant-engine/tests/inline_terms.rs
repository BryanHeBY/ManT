//! End-to-end lowering and rendering tests for definition-list term layout.
//!
//! The `inline-terms.1` fixture exercises every decision the model makes:
//!
//! * **Short terms** (`* / %`, `&&`, `space`) → `Fit` resolves run-in
//! * **Long terms** (`< > <= >= == !=`, `--verbose`) → `Fit` resolves stacked
//! * **Literal ASCII markers** (`o` in EXIT STATUS) → retained as definition tags
//!
//! Tests go through the full pipeline: `parse_manual_source` → model →
//! `render_query_text`, `render_query_man`, and `render_markdown`.

use std::path::PathBuf;

use mant_codec::encode::render_markdown;
use mant_ir::{Block, Document};
use mant_loader::load_roff_bytes;
use mant_loader::parse_manual_source;
use mant_render::{render_query_man, render_query_text};

#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/inline-terms.1")
}

fn document() -> &'static Document {
    use std::sync::OnceLock;
    static DOC: OnceLock<Document> = OnceLock::new();
    DOC.get_or_init(|| parse_manual_source(&fixture_path()).expect("parse inline-terms fixture"))
}

#[test]
fn native_definition_styles_retain_their_conditional_placement_policy() {
    // Verified before this assertion with the pinned CVS renderer.  The
    // placement branches come from man_term.c::pre_TP/post_TP and
    // mdoc_term.c::termp_it_pre/post; term.c::term_flushln performs the final
    // field-width decision.  A reader, not the producer, therefore resolves
    // `Fit` at its allocated width.
    let man = load_roff_bytes(
        b".TH K17 1\n.SH OPTIONS\n.TP 8n\n.B short\nshort body\n.TP 4n\n.B longlabel\nlong body\n",
    )
    .unwrap();
    let man_items =
        common::definition_items(common::section(man.document.as_ref().unwrap(), "OPTIONS"));
    assert_eq!(man_items.len(), 2);
    assert!(
        man_items
            .iter()
            .all(|item| item.layout.placement == mant_ir::DefinitionPlacement::Fit)
    );
    assert!(mant_ir::geometry::definition_placement(man_items[0], 0, None).run_in);
    assert!(!mant_ir::geometry::definition_placement(man_items[1], 0, None).run_in);

    let mdoc = load_roff_bytes(
        b".Dd September 19, 2026\n.Dt K17 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width 8n\n.It short\ntag body\n.It longlabel\nlong body\n.El\n.Bl -hang\n.It hang\nhang body\n.El\n.Bl -inset\n.It inset\ninset body\n.El\n.Bl -diag\n.It diag\ndiag body\n.El\n",
    )
    .unwrap();
    let mdoc_items = common::definition_items(common::section(
        mdoc.document.as_ref().unwrap(),
        "DESCRIPTION",
    ));
    assert_eq!(mdoc_items.len(), 5);
    assert_eq!(
        mdoc_items[0].layout.placement,
        mant_ir::DefinitionPlacement::Fit
    );
    assert_eq!(
        mdoc_items[1].layout.placement,
        mant_ir::DefinitionPlacement::Fit
    );
    assert!(
        mdoc_items[2..]
            .iter()
            .all(|item| { item.layout.placement == mant_ir::DefinitionPlacement::RunIn })
    );
}

// ---------------------------------------------------------------------------
// Model-layer assertions (lowering)
// ---------------------------------------------------------------------------

#[test]
fn fit_terms_resolve_against_the_retained_definition_field() {
    let doc = document();
    let operators = common::section(doc, "OPERATORS");
    let items = common::definition_items(operators);

    // (* / %, &&, space) fit the retained field.
    let short_terms = ["* / %", "&&", "space"];
    for needle in short_terms {
        let item = items
            .iter()
            .find(|item| {
                item.terms
                    .iter()
                    .any(|term| common::inline_text(term) == needle)
            })
            .unwrap_or_else(|| panic!("missing operator term {needle:?}"));
        assert!(
            mant_ir::geometry::definition_placement(item, 0, None).run_in,
            "term {needle:?} should fit"
        );
    }

    // (< > <= >= == !=) is wider than the retained six-column field.
    let wide = items
        .iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term).contains("< >"))
        })
        .expect("relational operators term");
    assert!(
        !mant_ir::geometry::definition_placement(wide, 0, None).run_in,
        "wide term should not fit"
    );
}

#[test]
fn long_option_names_are_not_inline() {
    let doc = document();
    let options = common::section(doc, "OPTIONS");
    let items = common::definition_items(options);

    let verbose = items
        .iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term).contains("--verbose"))
        })
        .expect("--verbose option");
    assert!(
        !mant_ir::geometry::definition_placement(verbose, 0, None).run_in,
        "--verbose should not fit"
    );
}

#[test]
fn uniform_ascii_markers_remain_authored_definition_tags() {
    let doc = document();
    let exit = common::section(doc, "EXIT STATUS");

    // Repetition does not prove that the character is semantically disposable.
    let has_bullet_list = exit.blocks.iter().any(|block| {
        matches!(
            block,
            Block::List {
                kind: mant_ir::ListKind::Bullet,
                ..
            }
        )
    });
    assert!(
        !has_bullet_list,
        "EXIT STATUS must not replace literal source tags, got: {:?}",
        exit.blocks
            .iter()
            .map(|b| match b {
                Block::DefinitionList { .. } => "DefinitionList",
                Block::List { kind, .. } => match kind {
                    mant_ir::ListKind::Bullet => "BulletList",
                    mant_ir::ListKind::Ordered { .. } => "OrderedList",
                    mant_ir::ListKind::Plain => "PlainList",
                },
                _ => "other",
            })
            .collect::<Vec<_>>()
    );
    let items = common::definition_items(exit);
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|item| {
        item.terms
            .iter()
            .all(|term| common::inline_text(term) == "o")
    }));
}

#[test]
fn a_literal_tp_bullet_glyph_remains_a_definition_term() {
    let operators = common::section(document(), "OPERATORS");
    let literal = common::definition_items(operators)
        .into_iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term) == "*")
        })
        .expect("literal .TP * definition");

    let Block::Paragraph { children, .. } = &literal.description[0] else {
        panic!("literal term description should be a paragraph");
    };
    assert_eq!(
        common::inline_text(children),
        "A literal punctuation term from a tagged paragraph."
    );
}

#[test]
fn tq_aliases_share_one_definition_and_recompute_its_layout() {
    let aliases = common::definition_items(common::section(document(), "ALIASES"));
    let [item] = aliases.as_slice() else {
        panic!(
            "expected one merged alias definition, got {}",
            aliases.len()
        );
    };
    assert_eq!(
        item.terms
            .iter()
            .map(|term| common::inline_text(term))
            .collect::<Vec<_>>(),
        ["-a", "--all"]
    );
    assert!(
        mant_ir::geometry::definition_placement(item, 0, None).run_in,
        "separate source heads are measured independently; --all fits width 7"
    );
}

#[test]
fn explicit_tp_widths_control_layout_and_persist() {
    let widths = common::definition_items(common::section(document(), "WIDTHS"));
    let find = |needle: &str| {
        widths
            .iter()
            .find(|item| {
                item.terms
                    .iter()
                    .any(|term| common::inline_text(term) == needle)
            })
            .copied()
            .unwrap_or_else(|| panic!("missing width term {needle:?}"))
    };

    assert!(
        mant_ir::geometry::definition_placement(find("tenletters"), 0, None).run_in,
        "a ten-column term fits a `.TP 20` hanging margin"
    );
    assert!(
        !mant_ir::geometry::definition_placement(find("short"), 0, None).run_in,
        "a five-column term does not fit a `.TP 3` hanging margin"
    );
    assert!(
        mant_ir::geometry::definition_placement(find("xy"), 0, None).run_in,
        "a width-less `.TP` inherits the preceding three-column margin"
    );
}

// ---------------------------------------------------------------------------
// Text / --format man rendering
// ---------------------------------------------------------------------------

fn query() -> mant_ir::ResolvedContent {
    common::query_for_document("inline-terms", document())
}

#[test]
fn text_format_resolves_run_in_and_stacked_terms() {
    let output = render_query_text(&query());

    // Inline heads share their structural body origin, clearing long terms.
    assert!(
        output.contains("* / %  Multiplication, division, and modulus."),
        "got: {output:?}"
    );
    assert!(output.contains("&&     Logical AND."), "got: {output:?}");
    assert!(
        output.contains("space  String concatenation."),
        "got: {output:?}"
    );

    // Fit=false: term on its own line.
    assert!(
        output.contains("--verbose\n"),
        "--verbose should be on its own line, got: {output:?}"
    );
}

#[test]
fn man_format_resolves_run_in_terms() {
    let output = render_query_man(&query());

    // Same tight layout via --format man (tldr omitted, same renderer).
    assert!(
        output.contains("* / %  Multiplication, division, and modulus."),
        "got: {output:?}"
    );
    assert!(
        output.contains("space  String concatenation."),
        "got: {output:?}"
    );

    // The native body starts at seven cells, not one space after each head.
    assert!(
        output.contains("-a\n--all  Show all entries."),
        "separate source heads must not acquire an invented comma, got: {output:?}"
    );
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

#[test]
fn markdown_resolves_definition_placement_without_changing_source_content() {
    let output = render_markdown(&query());

    // Fit=true: term bold + space + description on one line.
    assert!(
        output.contains("**\\* / %** Multiplication, division, and modulus.")
            || output.contains("**\\* / %** Multiplication"),
        "markdown inline term should be on one line, got: {output:?}"
    );

    // Fit=false: term on its own line, description on the next.
    assert!(
        output.contains("**--verbose**\n"),
        "markdown block term should be on its own line, got: {output:?}"
    );
    assert!(
        output.contains("**-a**  \n  **--all** Show all entries."),
        "separate authored heads retain a hard break without invented commas, got: {output:?}"
    );

    // CommonMark has no terminal field-width primitive. Export resolves the
    // retained policy once for text placement, while reparsing preserves all
    // authored text rather than inventing a legacy layout annotation.
    let reparsed = mant_loader::load_markdown_text(&output, None).unwrap();
    let text = render_query_text(&reparsed);
    for expected in [
        "Multiplication, division, and modulus.",
        "The regular relational operators.",
        "Enable verbose diagnostic output.",
        "Show all entries.",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text:?}");
    }
}
