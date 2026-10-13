//! Parser fill restoration reaches actual body blocks, wire text and search.
use mant_ir::{
    Block, ResolvedContent,
    visit::{self, Visit},
};
use mant_protocol::QueryBundle;
use serde_json::{Value, json};

#[test]
fn implicit_display_close_and_later_requests_keep_body_execution_in_source_order() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../libmandoc-rs/tests/fixtures/display_modes.json"
    ))
    .unwrap();
    assert_eq!(fixture["header"]["count"], 18);
    assert_eq!(fixture["header"]["expectationsFromProduct"], false);
    assert_eq!(
        fixture["header"]["oracleSha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    // Exact sources first ran pristine five-profile capture. The native
    // mdoc_macro.c::rew_pending Bd BLOCK checkpoint restores ROFF_NOFILL
    // before It allocates its children; later explicit nf/fi remains ordered.
    // mdoc_term.c::print_mdoc_node observes those per-node no-fill flags.
    for case in fixture["cases"].as_array().unwrap() {
        let query =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        assert!(
            mant_ir::validate_document(document).is_empty(),
            "{}",
            case["id"]
        );
        let json = serde_json::to_string(&QueryBundle::from(&query)).unwrap();
        let restored: QueryBundle = serde_json::from_str(&json).unwrap();
        let restored: ResolvedContent = restored.into();
        assert_eq!(
            restored.document, query.document,
            "{}: actual JSON text roundtrip",
            case["id"]
        );
        assert_body_flow(&restored, case);
        let text = mant_render::render_query_man(&restored);
        for word in ["BodyAlpha", "BodyBeta", "AfterAlpha", "AfterBeta"] {
            assert_eq!(text.matches(word).count(), 1, "{}: {text}", case["id"]);
            let result = mant_query::search_query(
                &restored,
                &serde_json::from_value(json!({"pattern":word})).unwrap(),
            )
            .unwrap();
            assert_eq!(result.matches.len(), 1, "{}: {word}", case["id"]);
            assert_eq!(
                result.matches[0].occurrence_count, 1,
                "{}: {word}",
                case["id"]
            );
            assert_eq!(result.matches[0].occurrences[0].matched_text, word);
        }
    }
}

fn assert_body_flow(query: &ResolvedContent, case: &Value) {
    #[derive(Default)]
    struct Roots(Vec<(bool, String)>);
    impl<'ir> Visit<'ir> for Roots {
        fn visit_block(&mut self, block: &'ir Block) {
            let root = match block {
                Block::Paragraph { children, .. } => Some((false, children)),
                Block::Preformatted { children, .. } => Some((true, children)),
                _ => None,
            };
            if let Some((literal, children)) = root {
                let text = mant_ir::inline_plain_text(children);
                if text.contains("BodyAlpha") {
                    self.0.push((literal, text));
                }
            }
            visit::walk_block(self, block);
        }
    }
    let mut roots = Roots::default();
    roots.visit_document(query.document.as_ref().unwrap());
    let body = case["expected"]["positionedText"]
        .as_array()
        .unwrap()
        .iter()
        .find(|word| word["label"] == "BodyAlpha")
        .unwrap();
    assert_eq!(body["owner"], "It BODY");
    let no_fill = body["no_fill"].as_bool().unwrap();
    let expected = if no_fill {
        "BodyAlpha\nBodyBeta"
    } else {
        "BodyAlpha BodyBeta"
    };
    assert_eq!(
        roots.0,
        [(no_fill, expected.into())],
        "{}: native body role and hard rows",
        case["id"]
    );
}
