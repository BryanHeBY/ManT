//! Argument and bibliography changes retain source-neutral consumer ownership.

use mant_ir::{
    DefinitionItem, DefinitionLayout, Document, ResolvedContent, SourceSpan,
    visit::{self, Visit},
};
use mant_protocol::{
    ContentSelector, EntryProjection, EvidenceBasis, EvidenceClass, ExplanationOptions,
    ExplanationQuery, QueryBundle,
};
use serde_json::{Value, json};

pub fn cases(source: &str, count: usize) -> Vec<Value> {
    let fixture: Value = serde_json::from_str(source).unwrap();
    assert_eq!(fixture["header"]["count"], count);
    assert_eq!(fixture["header"]["expectationsFromProduct"], false);
    assert_eq!(
        fixture["header"]["oracleSha256"],
        "6297105d1370a44fd306851ae5beab3f23492d756a682f8a0693896922249ea0"
    );
    fixture["cases"].as_array().unwrap().clone()
}

pub fn assert_case(case: &Value) {
    // Every complete source first ran the registered pristine five profiles.
    // Width/offset/column facts come from mdoc_validate.c::post_bl_norm and
    // rewrite_macro2len. Header volume is metadata, not generated prose;
    // msec.c's exact-first fallback does not rename the original section.
    let query = mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(document.heading.is_none(), "{}", case["id"]);
    assert!(
        mant_ir::validate_document(document).is_empty(),
        "{}",
        case["id"]
    );
    assert_metadata(document, case);
    let json = serde_json::to_string(&QueryBundle::from(&query)).unwrap();
    let restored: QueryBundle = serde_json::from_str(&json).unwrap();
    let restored: ResolvedContent = restored.into();
    assert_eq!(
        restored.document, query.document,
        "{}: JSON text roundtrip",
        case["id"]
    );
    let text = mant_render::render_query_man(&restored);
    assert_eq!(
        text.matches("BodyWord").count(),
        1,
        "{}: {text}",
        case["id"]
    );
    assert_eq!(
        text.matches("AfterWord").count(),
        1,
        "{}: {text}",
        case["id"]
    );
    let result = mant_query::search_query(
        &restored,
        &serde_json::from_value(json!({"pattern":"BodyWord"})).unwrap(),
    )
    .unwrap();
    assert_eq!(result.matches.len(), 1, "{}", case["id"]);
    assert_eq!(result.matches[0].occurrence_count, 1, "{}", case["id"]);
    assert_eq!(result.matches[0].occurrences[0].matched_text, "BodyWord");
    assert_eq!(result.meta.as_ref(), Some(&document.meta));
    if let Some(name) = primary_name(case) {
        let (source, layout) = assert_owner(&restored, case, name);
        assert_eq!(result.matches[0].node_source, source, "{}", case["id"]);
        if let Some(indent) = case["expected"]["bodyIndentColumns"].as_i64() {
            assert_eq!(
                i64::from(layout.body_indent_columns),
                indent,
                "{}",
                case["id"]
            );
        }
        assert_owner_queries(&restored, case, name, source);
    }
    if let Some(volume) = document
        .meta
        .volume
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        assert!(
            !text.contains(volume),
            "{}: header volume entered body",
            case["id"]
        );
        let result = mant_query::search_query(
            &restored,
            &serde_json::from_value(json!({"pattern":volume})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            result.total, 0,
            "{}: metadata became visible prose",
            case["id"]
        );
    }
}

fn assert_metadata(document: &Document, case: &Value) {
    let expected = &case["expected"]["metadata"];
    for (actual, key) in [
        (document.meta.title.as_deref(), "title"),
        (document.meta.manual_section.as_deref(), "section"),
        (document.meta.volume.as_deref(), "volume"),
        (document.meta.arch.as_deref(), "arch"),
        (document.meta.date.as_deref(), "date"),
        (document.meta.os.as_deref(), "os"),
    ] {
        assert_eq!(actual, expected[key].as_str(), "{}: {key}", case["id"]);
    }
    let names = expected["name"].as_str().into_iter().collect::<Vec<_>>();
    assert_eq!(document.meta.names, names, "{}: names", case["id"]);
    assert_eq!(
        document.parser.as_ref().unwrap().version,
        libmandoc_rs::LIBMANDOC_VERSION
    );
}

fn primary_name(case: &Value) -> Option<&'static str> {
    // Nested HEADs have independently checked AST ownership, not a claim
    // that the outer BodyWord belongs to the inner alpha declaration.
    if case["expected"]["normalizedArguments"]
        .as_array()
        .unwrap()
        .len()
        > 1
    {
        return None;
    }
    case["expected"]["positionedText"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|text| {
            if text["insideItemHead"] != true {
                return None;
            }
            match text["value"].as_str() {
                Some("alpha") => Some("-alpha"),
                Some("--alpha") => Some("--alpha"),
                _ => None,
            }
        })
}

fn assert_owner(
    query: &ResolvedContent,
    case: &Value,
    name: &str,
) -> (Option<SourceSpan>, DefinitionLayout) {
    struct Owners<'a> {
        name: &'a str,
        owners: Vec<(Option<SourceSpan>, DefinitionLayout)>,
    }
    impl<'a> Visit<'a> for Owners<'_> {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            if item
                .entry
                .as_ref()
                .is_some_and(|entry| entry.names.iter().any(|name| name == self.name))
            {
                self.owners.push((item.source, item.layout));
            }
            visit::walk_definition_item(self, item);
        }
    }
    let mut collector = Owners {
        name,
        owners: Vec::new(),
    };
    collector.visit_document(query.document.as_ref().unwrap());
    assert_eq!(
        collector.owners.len(),
        1,
        "{}: declaration owner",
        case["id"]
    );
    let (source, layout) = collector.owners[0];
    let owner = &case["expected"]["primaryOwner"];
    let span = source.unwrap();
    assert_eq!(
        u64::from(span.line),
        owner["line"].as_u64().unwrap(),
        "{}",
        case["id"]
    );
    assert_eq!(
        u64::from(span.column),
        owner["column"].as_u64().unwrap(),
        "{}",
        case["id"]
    );
    assert!(span.byte_range.is_none());
    (source, layout)
}

fn assert_owner_queries(
    query: &ResolvedContent,
    case: &Value,
    name: &str,
    source: Option<SourceSpan>,
) {
    let outline = mant_query::build_outline_projection(query, EntryProjection::All, None).unwrap();
    let json = serde_json::to_string(&outline).unwrap();
    let _: mant_protocol::QueryOutline = serde_json::from_str(&json).unwrap();
    let explanation = mant_query::explain_query(
        query,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let direct = explanation
        .evidence
        .iter()
        .filter(|evidence| {
            evidence.class == EvidenceClass::DirectEntry
                && evidence
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        })
        .collect::<Vec<_>>();
    assert_eq!(direct.len(), 1, "{}: direct evidence", case["id"]);
    assert_eq!(direct[0].source, source, "{}: original HEAD", case["id"]);
    let excerpt =
        mant_query::select_excerpt(query, &[ContentSelector::path(direct[0].outline.path())])
            .unwrap();
    assert_eq!(
        mant_render::render_excerpt_text(&excerpt)
            .matches("BodyWord")
            .count(),
        1,
        "{}",
        case["id"]
    );
    let json = serde_json::to_string(&explanation).unwrap();
    let _: mant_protocol::QueryExplanation = serde_json::from_str(&json).unwrap();
    let json = serde_json::to_string(&excerpt).unwrap();
    let _: mant_protocol::QueryExcerpt = serde_json::from_str(&json).unwrap();
}
