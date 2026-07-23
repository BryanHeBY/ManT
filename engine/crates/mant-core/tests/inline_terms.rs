//! End-to-end lowering and rendering tests for definition-list term layout.
//!
//! The `inline-terms.1` fixture exercises every decision the model makes:
//!
//! * **Short terms** (`* / %`, `&&`, `space`) → `inline_term = true`
//! * **Long terms** (`< > <= >= == !=`, `--verbose`) → `inline_term = false`
//! * **Uniform bullet markers** (`o` in EXIT STATUS) → normalised to `Block::List`
//!
//! Tests go through the full pipeline: `parse_manual_source` → model →
//! `render_query_text`, `render_query_man`, and `render_markdown`.

use std::path::PathBuf;

use mant_ast::{Block, MantDocument};
use mant_core::{parse_manual_source, render_markdown, render_query_man, render_query_text};

#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/inline-terms.1")
}

fn document() -> &'static MantDocument {
    use std::sync::OnceLock;
    static DOC: OnceLock<MantDocument> = OnceLock::new();
    DOC.get_or_init(|| parse_manual_source(&fixture_path()).expect("parse inline-terms fixture"))
}

// ---------------------------------------------------------------------------
// Model-layer assertions (lowering)
// ---------------------------------------------------------------------------

#[test]
fn short_terms_are_flagged_inline_and_long_terms_are_not() {
    let doc = document();
    let operators = common::section(doc, "OPERATORS");
    let items = common::definition_items(operators);

    // (* / %, &&, space) → inline_term = true
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
            item.inline_term,
            "term {needle:?} should be inline_term=true"
        );
    }

    // (< > <= >= == !=) is wider than 6 chars → inline_term = false
    let wide = items
        .iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term).contains("< >"))
        })
        .expect("relational operators term");
    assert!(!wide.inline_term, "wide term should be inline_term=false");
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
        !verbose.inline_term,
        "--verbose should be inline_term=false"
    );
}

#[test]
fn uniform_bullet_markers_are_normalised_to_a_bullet_list() {
    let doc = document();
    let exit = common::section(doc, "EXIT STATUS");

    // The three `.IP o 4` items should have been normalised into a
    // Block::List { kind: Bullet } rather than a DefinitionList.
    let has_bullet_list = exit.blocks.iter().any(|block| {
        matches!(
            block,
            Block::List {
                kind: mant_ast::ListKind::Bullet,
                ..
            }
        )
    });
    assert!(
        has_bullet_list,
        "EXIT STATUS should contain a normalised bullet list, got: {:?}",
        exit.blocks
            .iter()
            .map(|b| match b {
                Block::DefinitionList { .. } => "DefinitionList",
                Block::List { kind, .. } => match kind {
                    mant_ast::ListKind::Bullet => "BulletList",
                    mant_ast::ListKind::Ordered => "OrderedList",
                    mant_ast::ListKind::Plain => "PlainList",
                },
                _ => "other",
            })
            .collect::<Vec<_>>()
    );
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
        !item.inline_term,
        "combined '-a, --all' width must not inherit --all's stale layout"
    );
}

// ---------------------------------------------------------------------------
// Text / --format man rendering
// ---------------------------------------------------------------------------

fn query() -> mant_ast::QueryBundle {
    common::query_for_document("inline-terms", document())
}

#[test]
fn text_format_renders_inline_terms_tight_and_block_terms_hanging() {
    let output = render_query_text(&query());

    // inline_term=true: tight single-space layout.
    assert!(
        output.contains("* / % Multiplication, division, and modulus."),
        "got: {output:?}"
    );
    assert!(output.contains("&& Logical AND."), "got: {output:?}");
    assert!(
        output.contains("space String concatenation."),
        "got: {output:?}"
    );

    // inline_term=false: term on its own line.
    assert!(
        output.contains("--verbose\n"),
        "--verbose should be on its own line, got: {output:?}"
    );
}

#[test]
fn man_format_renders_inline_terms_tight() {
    let output = render_query_man(&query());

    // Same tight layout via --format man (tldr omitted, same renderer).
    assert!(
        output.contains("* / % Multiplication, division, and modulus."),
        "got: {output:?}"
    );
    assert!(
        output.contains("space String concatenation."),
        "got: {output:?}"
    );

    // No leaked double-space between term and description.
    assert!(!output.contains("* / %  "), "got: {output:?}");
    assert!(!output.contains("space  "), "got: {output:?}");
    assert!(
        output.contains("-a, --all\n"),
        "aliases should use the shared term separator, got: {output:?}"
    );
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

#[test]
fn markdown_renders_inline_terms_on_the_same_line() {
    let output = render_markdown(&query());

    // inline_term=true: term bold + space + description on one line.
    assert!(
        output.contains("**\\* / %** Multiplication, division, and modulus.")
            || output.contains("**\\* / %** Multiplication"),
        "markdown inline term should be on one line, got: {output:?}"
    );

    // inline_term=false: term on its own line, description on the next.
    assert!(
        output.contains("**--verbose**\n"),
        "markdown block term should be on its own line, got: {output:?}"
    );
    assert!(
        output.contains("**-a**, **--all**\n"),
        "aliases should be comma-separated without a Markdown hard break, got: {output:?}"
    );
}
