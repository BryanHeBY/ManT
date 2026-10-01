//! Metadata-only link occurrences survive physical-row representation changes.

use mant_ir::{Inline, ReferenceScope, ReferenceTargetType, ResolvedContent, visit};
use mant_protocol::{ReferenceProjection, ReferenceProjectionMode};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const ADDRESS: &str = "https://ex.org";

fn round_trip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn assert_occurrences(query: &ResolvedContent, count: usize, source: &str) {
    struct Links<'source>(usize, &'source str);
    impl<'ir> visit::Visit<'ir> for Links<'_> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link { children, .. } = inline {
                self.0 += 1;
                assert!(
                    mant_ir::inline_plain_text(children).is_empty(),
                    "{}",
                    self.1
                );
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut links = Links(0, source);
    visit::Visit::visit_document(&mut links, query.document.as_ref().unwrap());
    assert_eq!(links.0, count, "{source}");
    let inventory = mant_query::project_references(
        query.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::External],
            ..Default::default()
        },
    );
    assert_eq!(inventory.records.len(), count, "{source}: {inventory:#?}");
    for record in inventory.records {
        assert!(record.label.is_empty(), "{source}");
        assert_eq!(
            record.target,
            mant_ir::LinkTarget::External {
                uri: ADDRESS.into()
            },
            "{source}"
        );
        let excerpt =
            mant_query::select_excerpt(query, std::slice::from_ref(&record.source_read)).unwrap();
        let serialized = serde_json::to_string(&excerpt).unwrap();
        let _: mant_protocol::QueryExcerpt = serde_json::from_str(&serialized).unwrap();
        assert!(!mant_render::render_excerpt_text(&excerpt).contains(ADDRESS));
    }
    let search = serde_json::from_value(serde_json::json!({
        "pattern": ADDRESS,
        "scope": "visible"
    }))
    .unwrap();
    assert!(
        mant_query::search_query(query, &search)
            .unwrap()
            .matches
            .is_empty()
    );
}

#[test]
fn rejected_labels_retain_each_authored_identity_across_all_row_drains() {
    // All 30 exact complete inputs ran the fixed pristine oracle first.
    // term_fill() rejects these label/colon/URI glyphs, while
    // mdoc_html.c::mdoc_lk_pre() retains each href. Real row cleanup may
    // transfer empty rows, but cannot erase an occurrence or revive its text.
    for context in ["filled", "nf", "literal", "tag", "hang"] {
        for boundary in ["", ".br\n", ".Pp\n"] {
            for repeat in [1, 2] {
                let content =
                    format!(".Lk {ADDRESS} \"\\p D\"\n{boundary}").repeat(repeat) + ".No AFTER\n";
                let body = match context {
                    "nf" => format!(".nf\n{content}.fi\n"),
                    "literal" => format!(".Bd -literal\n{content}.Ed\n"),
                    "tag" | "hang" => format!(
                        ".Bl -{context} -width 8n\n.It Xo\n{content}.Xc\n.No BodyWord\n.El\n"
                    ),
                    _ => content,
                };
                let source = format!("{HEADER}{body}.Sh NEXT\n.No END\n");
                let query = round_trip(&source);
                assert_occurrences(&query, repeat, &source);
                let plain = mant_render::render_query_man(&query);
                assert!(!plain.contains(ADDRESS), "{source}: {plain}");
                let markdown = mant_codec::encode::render_markdown(&query);
                assert!(!markdown.contains('\0'));
                let reparsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
                if !matches!(context, "nf" | "literal") {
                    assert_occurrences(&reparsed, repeat, &source);
                }
                // The established CommonMark exporter flattens literal
                // children into a code fence, which cannot carry an active
                // link. Native JSON/inventory remain exact above; portable
                // literal export must not revive the rejected URI as text.
                assert!(!mant_render::render_query_man(&reparsed).contains(ADDRESS));
            }
        }
    }
}
