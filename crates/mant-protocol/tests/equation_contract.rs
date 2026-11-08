//! Formula invariants apply equally to complete and detached native responses.
use mant_ir::{Block, EquationExpression, Inline};
use mant_protocol::{QueryBundle, QueryExcerpt, QueryExplanation};
use serde_json::{Value, json};

fn atom() -> Value {
    json!({"kind":"text", "font":"none", "operator":"none",
        "atomRole":"identifier", "text":"x", "children":[]})
}

fn inline(value: &str) -> Value {
    json!({"type":"equation", "value":value, "expression":atom()})
}

#[test]
fn formula_fields_and_operand_roles_have_closed_shapes() {
    assert!(serde_json::from_value::<EquationExpression>(atom()).is_ok());
    for field in ["size", "expectedArgs", "actualArgs", "position", "unknown"] {
        let mut invalid = atom();
        invalid[field] = 2.into();
        assert!(
            serde_json::from_value::<EquationExpression>(invalid).is_err(),
            "{field}"
        );
    }
    let mut opaque = atom();
    opaque["kind"] = "opaque".into();
    opaque.as_object_mut().unwrap().remove("atomRole");
    opaque["operandScope"] = "compound".into();
    assert!(serde_json::from_value::<EquationExpression>(opaque.clone()).is_ok());
    opaque["atomRole"] = "function".into();
    assert!(serde_json::from_value::<EquationExpression>(opaque).is_err());
    for (operator, children) in [
        ("fraction", json!([atom()])),
        ("square-root", json!([])),
        ("subscript-superscript", json!([atom(), atom()])),
    ] {
        let invalid = json!({"kind":"operation", "font":"none", "operator":operator,
            "children":children});
        assert!(
            serde_json::from_value::<EquationExpression>(invalid).is_err(),
            "{operator}"
        );
    }
}

#[test]
fn detached_inline_and_block_equations_reject_inconsistent_projection() {
    assert!(serde_json::from_value::<Inline>(inline("x")).is_ok());
    assert!(serde_json::from_value::<Inline>(inline("not x")).is_err());
    for value in ["x", "not x"] {
        let decoded = serde_json::from_value::<Block>(json!({"type":"equation",
            "value":value, "expression":atom(), "display":true}));
        assert_eq!(decoded.is_ok(), value == "x");
    }
}

#[test]
fn complete_queries_and_detached_excerpts_use_identical_formula_checks() {
    for value in ["x", "not x"] {
        let mut query: Value = serde_json::from_str(include_str!(
            "../../../tests/contracts/minimal-query-v0.14.json"
        ))
        .unwrap();
        query["document"]["blocks"] = json!([{"type":"paragraph", "children":[inline(value)]}]);
        assert_eq!(
            serde_json::from_value::<QueryBundle>(query).is_ok(),
            value == "x"
        );
        let excerpt = json!({"schema":"mant.excerpt/v0.14", "label":"formula",
            "selections":[{"kind":"document-root", "outline":{"node":{
                "kind":"document-root", "path":"root", "id":"root", "title":"formula"}},
                "blocks":[{"type":"paragraph", "children":[inline(value)]}]}]});
        assert_eq!(
            serde_json::from_value::<QueryExcerpt>(excerpt).is_ok(),
            value == "x"
        );
    }
}

#[test]
fn explanation_form_payloads_do_not_bypass_formula_projection_checks() {
    for value in ["x", "not x"] {
        let mut explanation: Value = serde_json::from_str(include_str!(
            "../../../tests/contracts/explanation-v0.14.json"
        ))
        .unwrap();
        explanation["evidence"][0]["entry"]["forms"][0]
            .as_array_mut()
            .unwrap()
            .push(inline(value));
        assert_eq!(
            serde_json::from_value::<QueryExplanation>(explanation).is_ok(),
            value == "x"
        );
    }
}
