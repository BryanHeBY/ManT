//! Generates discoverable JSON Schemas from the authoritative Rust contracts.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, generate::SchemaSettings};

use crate::{QueryBundle, QueryExcerpt, QueryOutline, QueryRequest};

/// Generate the JSON representation accepted when deserializing a request.
#[must_use]
pub fn query_request_json_schema() -> Schema {
    deserialize_schema::<QueryRequest>()
}

/// Generate the complete-query JSON representation emitted by `mant-cli`.
#[must_use]
pub fn query_bundle_json_schema() -> Schema {
    serialize_schema::<QueryBundle>()
}

/// Generate the outline JSON representation emitted by `mant-cli`.
#[must_use]
pub fn query_outline_json_schema() -> Schema {
    serialize_schema::<QueryOutline>()
}

/// Generate the selected-excerpt JSON representation emitted by `mant-cli`.
#[must_use]
pub fn query_excerpt_json_schema() -> Schema {
    serialize_schema::<QueryExcerpt>()
}

/// Generate every public request and query-response contract in stable order.
#[must_use]
pub fn query_json_schema_catalog() -> BTreeMap<&'static str, Schema> {
    BTreeMap::from([
        ("excerpt", query_excerpt_json_schema()),
        ("outline", query_outline_json_schema()),
        ("query", query_bundle_json_schema()),
        ("request", query_request_json_schema()),
    ])
}

fn deserialize_schema<T: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12()
        .for_deserialize()
        .into_generator()
        .into_root_schema_for::<T>()
}

fn serialize_schema<T: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<T>()
}
