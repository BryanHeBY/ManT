//! Closed local decoding and schema constraints shared by every formula owner.
use super::{
    EquationAtomRole, EquationExpression, EquationFont, EquationKind, EquationOperandScope,
    EquationOperator,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize, JsonSchema)]
#[serde(
    remote = "EquationExpression",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct EquationExpressionWire {
    kind: EquationKind,
    font: EquationFont,
    operator: EquationOperator,
    #[serde(default, skip_serializing_if = "EquationAtomRole::is_unknown")]
    atom_role: EquationAtomRole,
    #[serde(default, skip_serializing_if = "EquationOperandScope::is_atomic")]
    operand_scope: EquationOperandScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    right: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    top: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bottom: Option<String>,
    children: Vec<EquationExpression>,
}

impl<'de> Deserialize<'de> for EquationExpression {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let expression = EquationExpressionWire::deserialize(deserializer)?;
        // Children have already checked their own node. Do not re-walk their
        // subtrees here; aggregate work limits belong to one root validation.
        expression
            .validate_shallow()
            .map_err(serde::de::Error::custom)?;
        Ok(expression)
    }
}

impl JsonSchema for EquationExpression {
    fn schema_name() -> Cow<'static, str> {
        "EquationExpression".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = EquationExpressionWire::json_schema(generator);
        let constraints = json_schema!({"allOf": [
            {
                "if": {"properties": {"kind": {"enum": ["text", "opaque"]}}},
                "then": {
                    "required": ["text"],
                    "properties": {"text": {"type": "string"}, "children": {"maxItems": 0}, "operator": {"const": "none"}}
                },
                "else": {"properties": {"text": {"type": "null"}}}
            },
            {"if": {"properties": {"kind": {"not": {"const": "text"}}}}, "then": {"properties": {"atomRole": {"const": "unknown"}}}},
            {"if": {"properties": {"kind": {"not": {"const": "opaque"}}}}, "then": {"properties": {"operandScope": {"const": "atomic"}}}},
            {
                "if": {"properties": {"kind": {"const": "operation"}}},
                "then": {"properties": {"operator": {"not": {"const": "none"}}}},
                "else": {"properties": {"operator": {"const": "none"}}}
            },
            {"if": {"properties": {"operator": {"const": "square-root"}}}, "then": {"properties": {"children": {"minItems": 1, "maxItems": 1}}}},
            {"if": {"properties": {"operator": {"enum": ["subscript-superscript", "lower-upper-limits"]}}}, "then": {"properties": {"children": {"minItems": 3, "maxItems": 3}}}},
            {"if": {"properties": {"operator": {"enum": ["superscript", "subscript", "upper-limit", "lower-limit", "fraction"]}}}, "then": {"properties": {"children": {"minItems": 2, "maxItems": 2}}}}
        ]});
        schema.insert(
            "allOf".into(),
            constraints
                .get("allOf")
                .expect("static schema constraints")
                .clone(),
        );
        schema
    }
}
