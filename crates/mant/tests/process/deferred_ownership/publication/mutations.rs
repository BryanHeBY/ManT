//! Single-field counterexamples make sure the assertions distinguish causes.
use super::{Case, array, cases, descendants, entries, executable, members};
use serde_json::{Value, json};

fn case(id: &str) -> Case<'static> {
    cases().find(|case| case.id == id).unwrap()
}

fn document(case: &Case<'_>) -> Value {
    case.json(executable(), &["--compact"])["document"].clone()
}

fn rejected(field: &str, check: impl FnOnce() + std::panic::UnwindSafe) {
    let panic = std::panic::catch_unwind(check).expect_err("mutated observation was accepted");
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(message.contains(field), "wrong assertion: {message}");
}

#[test]
fn publication_entry_assertions_reject_missing_name_and_plausible_wrong_support() {
    let head = case("N1a/DESCRIPTION/digit/xr");
    let mut doc = document(&head);
    let blocks = &mut doc["sections"][1]["blocks"];
    entries::head(&head, array(blocks));
    blocks[0]["items"][0]["entry"]["names"] = json!([]);
    rejected(" names", || entries::head(&head, array(blocks)));

    let group = case("N1b/provider/adjacent");
    let doc = document(&group);
    let original = &doc["sections"][1]["blocks"][0];
    let response = group.json(executable(), &["--compact", "--explain=-a"]);
    entries::shared_support(&group, original, &response);
    let index = usize::try_from(
        response["evidence"][0]["content"]["support"]
            .as_u64()
            .unwrap(),
    )
    .unwrap();
    let support = response["supports"][index].clone();
    let mut wrong_owner = response.clone();
    let mut neighbor = support.clone();
    neighbor["block"]["items"][1]["source"]["line"] = json!(99);
    let supports = wrong_owner["supports"].as_array_mut().unwrap();
    let new_index = supports.len();
    supports.push(neighbor);
    wrong_owner["evidence"][0]["content"]["support"] = json!(new_index);
    rejected(" source owner", || {
        entries::shared_support(&group, original, &wrong_owner);
    });

    let mut wrong_provider = response;
    wrong_provider["supports"][index]["block"]["items"][1]["description"][0]["children"] =
        json!([{"type":"text","value":"OTHERTEXT"}]);
    rejected(" provider content", || {
        entries::shared_support(&group, original, &wrong_provider);
    });
}

#[test]
fn publication_row_assertions_reject_semantic_and_physical_boundary_damage() {
    let prefix = case("N2/UR/prev/zero/2n/same");
    let doc = document(&prefix);
    let link = descendants(&doc["sections"][1]["blocks"], "link")[0];
    members::prefix(&prefix, &[link]);
    let mut widened = link.clone();
    widened["children"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"type":"text","value":" "}));
    rejected(" label", || members::prefix(&prefix, &[&widened]));

    let gap = case("N3/UR/first/one");
    let mut doc = document(&gap);
    let blocks = &mut doc["sections"][1]["blocks"];
    members::spacing_sources(&gap, array(blocks));
    blocks[0].as_object_mut().unwrap().remove("source");
    rejected("N3/UR/first/one", || {
        members::spacing_sources(&gap, array(blocks));
    });
    let rows = String::from_utf8(gap.output(executable(), "man", &[])).unwrap();
    members::assert_rows(&gap, &rows);
    let without_row = rows.replacen("DESCRIPTION\n\n", "DESCRIPTION\n", 1);
    rejected(" physical rows", || {
        members::assert_rows(&gap, &without_row);
    });

    let boundary = case("N4/UR/filled/EE/text");
    let doc = document(&boundary);
    let link = descendants(&doc["sections"][1]["blocks"], "link")[0];
    members::label_rows(&boundary, &[link]);
    let mut glued = link.clone();
    let break_node = glued["children"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|node| node["type"] == "line-break")
        .unwrap();
    *break_node = json!({"type":"text","value":" "});
    rejected(" internal hard row", || {
        members::label_rows(&boundary, &[&glued]);
    });
}

#[test]
fn publication_annotation_assertions_reject_literal_loss_and_incomplete_diagnostics() {
    let literal = case("E12/raw/BR/plain");
    let mut doc = document(&literal);
    let section = &doc["sections"][1];
    let links = descendants(&section["blocks"], "link");
    members::manual(&literal, section, &links);
    let children = doc["sections"][1]["blocks"][0]["children"]
        .as_array_mut()
        .unwrap();
    let suffix = children
        .iter_mut()
        .find(|node| {
            node["value"]
                .as_str()
                .is_some_and(|value| value.contains("<>"))
        })
        .unwrap();
    suffix["value"] = json!("");
    let section = &doc["sections"][1];
    let links = descendants(&section["blocks"], "link");
    rejected(" visible suffix", || {
        members::manual(&literal, section, &links);
    });

    let source = case("E3/UR/first-eq");
    let mut doc = document(&source);
    let section = &doc["sections"][1];
    let links = descendants(&section["blocks"], "link");
    members::equation(&source, &doc, section, &links);
    doc["diagnostics"] = json!([]);
    let section = &doc["sections"][1];
    let links = descendants(&section["blocks"], "link");
    rejected(" semantic diagnostic", || {
        members::equation(&source, &doc, section, &links);
    });

    let response = source.json(executable(), &["--compact", "--explain=TAIL"]);
    let mut explain: mant_protocol::QueryExplanation = serde_json::from_value(response).unwrap();
    members::explanation_coverage(&source, &explain);
    explain.semantics_complete = true;
    rejected(" explicit explain completeness", || {
        members::explanation_coverage(&source, &explain);
    });
}
