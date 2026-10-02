//! Accepted link labels remain addressable across native and portable views.

use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{LinkTarget, ReferenceScope, ReferenceTargetType, ResolvedContent};
use mant_protocol::{
    EntryProjection, ReferenceProjection, ReferenceProjectionMode, SearchCase, SearchQuery,
    SearchScope, SearchSyntax,
};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const ADDRESS: &str = "https://example.com";

fn source(prefix: &str, label: &str) -> String {
    format!("{HEADER}{prefix}.Lk {ADDRESS} \"{label}\"\n.No AFTER\n.Sh NEXT\n.No END\n")
}

fn round_trip(source: &str) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&loaded, false).unwrap();
    assert!(
        !json.contains("\\u0000mant:"),
        "private output owner leaked"
    );
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn description(text: &str) -> String {
    let rows = text.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = rows.iter().position(|row| *row == "NEXT").unwrap();
    assert_eq!(
        rows[end - 1],
        "",
        "section boundary is separate from its body"
    );
    rows[start..end - 1].join("\n")
}

fn reference_policy() -> ReferenceProjection {
    ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        target_types: vec![ReferenceTargetType::External],
        ..Default::default()
    }
}

fn assert_reference(query: &ResolvedContent, label: &str, source: &str) {
    let inventory = mant_query::project_references(
        query.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &reference_policy(),
    );
    let [record] = inventory.records.as_slice() else {
        panic!("one accepted link expected: {source}\n{inventory:#?}");
    };
    assert_eq!(record.label, label, "{source}");
    assert_eq!(
        record.target,
        LinkTarget::External {
            uri: ADDRESS.into()
        }
    );
    let outline = mant_query::build_outline_with_references(
        query,
        EntryProjection::None,
        None,
        &reference_policy(),
    )
    .unwrap();
    assert_eq!(outline.references.records, inventory.records, "{source}");
    // The inventory's reveal selector must read the surviving link's actual
    // owner, rather than a display-only spelling or another source operand.
    let excerpt =
        mant_query::select_excerpt(query, std::slice::from_ref(&record.source_read)).unwrap();
    assert!(
        mant_render::render_excerpt_text(&excerpt).contains(label),
        "{source}"
    );
}

fn assert_search_artifacts(query: &ResolvedContent, source: &str) {
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let artifact =
            render_addressable_markdown_with_options(query, MarkdownOptions::ADDRESSABLE);
        let result = mant_query::search_query(
            query,
            &SearchQuery {
                pattern: ADDRESS.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::default(),
                scope,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert!(!result.matches.is_empty(), "{scope:?}: {source}");
        assert_eq!(
            usize::try_from(result.render.line_count).unwrap(),
            artifact.text().lines().count()
        );
        for found in &result.matches {
            for occurrence in &found.occurrences {
                let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                assert_eq!(&artifact.text()[range], ADDRESS, "{scope:?}: {source}");
            }
        }
    }
}

#[test]
fn invisible_descriptions_keep_the_accepted_address_in_every_projection() {
    // Exact complete inputs ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // mdoc_term.c::termp_lk_pre executes description -> ':' -> URI once;
    // export keeps ':' and annotates the accepted URI when there is no
    // accepted readable label. Both operands belong to the only visible body.
    for label in ["", r"\&", r"\zX", r"\fB"] {
        let source = source("", label);
        let query = round_trip(&source);
        assert_eq!(
            description(&mant_render::render_query_man(&query)),
            format!(": {ADDRESS} AFTER")
        );
        assert_reference(&query, ADDRESS, &source);
        assert_search_artifacts(&query, &source);
        let exported = mant_codec::encode::render_markdown(&query);
        let reparsed = mant_loader::load_markdown_text(&exported, None).unwrap();
        assert_reference(&reparsed, ADDRESS, &source);
        assert!(description(&mant_render::render_query_man(&reparsed)).contains(ADDRESS));
    }
}

#[test]
fn preceding_delayed_glyph_does_not_become_part_of_the_fallback_link() {
    // term_word() writes the next separator before encode1 consumes
    // BACKBEFORE (term.c). The caller's X survives before the colon; only
    // the URI operand supplies fallback label glyphs and activation range.
    let source = source(".No \\zX\n", "");
    let query = round_trip(&source);
    assert_eq!(
        description(&mant_render::render_query_man(&query)),
        format!("X: {ADDRESS} AFTER")
    );
    assert_reference(&query, ADDRESS, &source);
    assert_search_artifacts(&query, &source);
}

#[test]
fn accepted_descriptions_keep_the_uri_suffix_in_markdown() {
    let source = source("", "label");
    let query = round_trip(&source);
    assert_eq!(
        description(&mant_render::render_query_man(&query)),
        format!("label: {ADDRESS} AFTER")
    );
    assert_reference(&query, "label", &source);
    let exported = mant_codec::encode::render_markdown(&query);
    let reparsed = mant_loader::load_markdown_text(&exported, None).unwrap();
    assert_reference(&reparsed, "label", &source);
    assert_eq!(
        description(&mant_render::render_query_man(&reparsed)),
        format!("label: {ADDRESS} AFTER")
    );
}

#[test]
fn declared_column_layout_keeps_fallback_identity_and_both_cells() {
    // These exact column inputs ran pristine CVS before this assertion.
    // termp_lk_pre's colon/URI and termp_it_pre's column field execute once;
    // exceeding the first declaration moves the second cell to its origin.
    for label in ["", r"\&", r"\zX", r"\fB"] {
        let source = format!(
            "{HEADER}.Bl -column \"xx\" \"xx\"\n.It Lk {ADDRESS} \"{label}\" Ta RightWord\n.El\n.Sh NEXT\n.No END\n"
        );
        let query = round_trip(&source);
        assert_eq!(
            description(&mant_render::render_query_man(&query)),
            format!(": {ADDRESS}\n      RightWord")
        );
        assert_reference(&query, ADDRESS, &source);
        assert_search_artifacts(&query, &source);
        let portable = mant_codec::encode::render_markdown(&query);
        assert!(portable.contains(ADDRESS));
        assert_eq!(portable.matches("RightWord").count(), 1);
    }
}

#[test]
fn invalid_identity_keeps_accepted_native_suffix_and_its_closed_head_row() {
    // Exact complete input was rerun with pristine ASCII/UTF-8/HTML/tree/lint
    // before asserting this carrier transition. term_fill() accepts X/URI,
    // rejects Z, and termp_it_post closes the HEAD before BodyWord. An invalid
    // href cannot justify hiding the accepted URI in portable output.
    let source = format!(
        "{HEADER}.Bl -hang -width 4n\n.It Xo\n.Lk \"https://example.org\\p \\p\" X\n.No Z\n.Xc\n.No BodyWord\n.El\n"
    );
    let query = round_trip(&source);
    let native = mant_render::render_query_man(&query);
    let body = native.split_once("DESCRIPTION\n").unwrap().1;
    assert_eq!(body, "X: https://example.org\n      BodyWord");
    let portable = mant_codec::encode::render_markdown(&query);
    assert!(portable.contains("https://example.org"));
    assert_eq!(portable.matches("BodyWord").count(), 1);
    assert!(!body.contains('Z'));
}

#[test]
fn zero_column_descriptions_keep_their_native_text_and_a_readable_uri_label() {
    // Exact unquoted Unicode escape sources ran pristine CVS first. encode1
    // still writes a graph cell at width zero; presentation eligibility must
    // not reject it, but cannot hide the URI behind a zero-column link label.
    for (operand, native_label) in [
        (r"\[u200B]", "\u{200b}"),
        (r"\[u200D]", "\u{200d}"),
        (r"\[u0301]", "\u{0301}"),
    ] {
        let source = format!("{HEADER}.Lk {ADDRESS} {operand}\n.No AFTER\n.Sh NEXT\n.No END\n");
        let query = round_trip(&source);
        assert_eq!(
            description(&mant_render::render_query_man(&query)),
            format!("{native_label}: {ADDRESS} AFTER")
        );
        assert_reference(&query, ADDRESS, &source);
        assert_search_artifacts(&query, &source);
        let portable = mant_codec::encode::render_markdown(&query);
        assert!(portable.contains(native_label));
        let reparsed = mant_loader::load_markdown_text(&portable, None).unwrap();
        assert_reference(&reparsed, ADDRESS, &source);
    }
}
