//! Offline process schema presentation, independent of enabled execution capabilities.

use crate::{
    CLI_PROTOCOL_VERSION, arguments::SchemaContract, error::Failure, presentation::render_json,
};
use mant_protocol::{
    CatalogSchema, DocumentSchema, ExcerptSchema, OutlineSchema, QuerySchema, RequestSchema,
    ScopeQuerySchema, ScopeRequestSchema, SearchSchema,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolDescription<'a> {
    protocol: &'a str,
    native_api_version: &'a str,
    request_schema: &'a str,
    query_schema: &'a str,
    document_schema: &'a str,
    outline_schema: &'a str,
    excerpt_schema: &'a str,
    explanation_schema: &'a str,
    search_schema: &'a str,
    scope_request_schema: &'a str,
    scope_query_schema: &'a str,
    catalog_schema: &'a str,
}

pub(crate) fn render_protocol_description(pretty: bool) -> Result<String, Failure> {
    render_json(
        &ProtocolDescription {
            protocol: CLI_PROTOCOL_VERSION,
            native_api_version: mant_protocol::NATIVE_API_VERSION,
            request_schema: RequestSchema::ID,
            query_schema: QuerySchema::ID,
            document_schema: DocumentSchema::ID,
            outline_schema: OutlineSchema::ID,
            excerpt_schema: ExcerptSchema::ID,
            explanation_schema: mant_protocol::ExplanationSchema::ID,
            search_schema: SearchSchema::ID,
            scope_request_schema: ScopeRequestSchema::ID,
            scope_query_schema: ScopeQuerySchema::ID,
            catalog_schema: CatalogSchema::ID,
        },
        pretty,
    )
}

pub(crate) fn render_schema(contract: SchemaContract, pretty: bool) -> Result<String, Failure> {
    match contract {
        SchemaContract::Doctor => render_json(&mant_protocol::doctor_report_json_schema(), pretty),
        SchemaContract::TldrUpdate => {
            render_json(&mant_protocol::tldr_cache_update_json_schema(), pretty)
        }
        SchemaContract::Request => render_json(&mant_protocol::query_request_json_schema(), pretty),
        SchemaContract::Query => render_json(&mant_protocol::query_bundle_json_schema(), pretty),
        SchemaContract::Outline => render_json(&mant_protocol::query_outline_json_schema(), pretty),
        SchemaContract::Excerpt => render_json(&mant_protocol::query_excerpt_json_schema(), pretty),
        SchemaContract::Explanation => {
            render_json(&mant_protocol::query_explanation_json_schema(), pretty)
        }
        SchemaContract::Search => render_json(&mant_protocol::query_search_json_schema(), pretty),
        SchemaContract::ScopeRequest => {
            render_json(&mant_protocol::scope_query_request_json_schema(), pretty)
        }
        SchemaContract::ScopeQuery => {
            render_json(&mant_protocol::scope_query_response_json_schema(), pretty)
        }
        SchemaContract::Catalog => {
            render_json(&mant_protocol::document_catalog_json_schema(), pretty)
        }
        SchemaContract::All => render_json(&mant_protocol::query_json_schema_catalog(), pretty),
    }
}
