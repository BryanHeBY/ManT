use mant_protocol::{ContentSelector, EvidenceClass};

use super::support::{check_head, check_occurrences, explain, load, roundtrip};

#[test]
fn kstat_parallel_fields_share_their_original_definition_owner() {
    // CVS -Ttree keeps both Fa elements at line 105 under one It HEAD,
    // and -Thtml includes their shared description in the corresponding dd.
    let content = load(include_str!(
        "../../../../tests/fixtures/roff/real/mdoc-subjects/illumos-kstat.3kstat"
    ));
    let items = super::definitions(content.document.as_ref().unwrap());
    let combined = items
        .iter()
        .find(|item| item.source.is_some_and(|span| span.line == 105))
        .expect("original definition HEAD");
    check_head(
        combined,
        "ks_module, ks_instance",
        &["ks_module", "ks_instance"],
    );

    let mut owner_path = None;
    for (name, range) in [
        ("ks_module", 0..9),
        ("ks_instance", 11..22),
        ("ks_name", 0..7),
    ] {
        let explanation = roundtrip(&explain(&content, name));
        assert_eq!(explanation.counts.direct_entry.total, 1, "{name}");
        let occurrence = explanation
            .evidence
            .iter()
            .find(|evidence| evidence.class == EvidenceClass::DirectEntry)
            .unwrap();
        check_occurrences(occurrence, name, &[range]);
        let path = occurrence.outline.path();
        if name == "ks_name" {
            assert_ne!(owner_path.as_deref(), Some(path));
            continue;
        }
        assert_eq!(occurrence.source, combined.source);
        if let Some(existing) = &owner_path {
            assert_eq!(path, existing);
        }
        owner_path = Some(path.to_owned());
        let excerpt = mant_query::select_excerpt(&content, &[ContentSelector::path(path)]).unwrap();
        assert_eq!(
            excerpt,
            mant_query::select_excerpt(
                &content,
                &[ContentSelector::id(occurrence.outline.node.id())]
            )
            .unwrap(),
        );
        assert!(
            mant_render::render_excerpt_text(&excerpt).contains("contain the name and instance")
        );
    }
}
