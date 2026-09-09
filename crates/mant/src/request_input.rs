//! Bounded native request decoding; no query execution or output policy.
use crate::{arguments::QuerySource, error::Failure, json_boundary};
use mant_protocol::{QueryRequest, RequestSchema, ScopeQueryRequest, ScopeRequestSchema};
use std::io::Read;
pub(super) const MAX_REQUEST_BYTES: u64 = 64 * 1024;

pub(super) fn read_query_request(
    source: QuerySource,
    input: &mut dyn Read,
) -> Result<QueryRequest, Failure> {
    match source {
        QuerySource::Arguments(request) => return Ok(request),
        QuerySource::StdinJson => {}
        QuerySource::InputStdin { .. } => {
            unreachable!("direct stdin input is consumed before protocol request decoding");
        }
        QuerySource::ScopeArguments { .. } => {
            unreachable!("scope requests are consumed before single-document decoding");
        }
    }

    let request = read_utf8_input(input, MAX_REQUEST_BYTES, "request JSON")?;
    serde_json::from_str(&request)
        .map_err(|error| Failure::usage(format!("invalid query request JSON: {error}")))
}

pub(super) enum NativeRequest {
    Query(QueryRequest),
    Scope(ScopeQueryRequest),
}

pub(super) fn read_native_request(input: &mut dyn Read) -> Result<NativeRequest, Failure> {
    let request = read_utf8_input(input, MAX_REQUEST_BYTES, "request JSON")?;
    let value = match serde_json::from_str::<serde_json::Value>(&request) {
        Ok(value) => value,
        Err(error) if error.to_string().contains("recursion limit exceeded") => {
            if let Some(schema) = json_boundary::top_level_string(request.as_bytes(), "schema")
                && !matches!(schema.as_str(), RequestSchema::ID | ScopeRequestSchema::ID)
            {
                return Err(Failure::usage(format!(
                    "unsupported request schema '{schema}'; expected '{}' or '{}'",
                    RequestSchema::ID,
                    ScopeRequestSchema::ID
                )));
            }
            return Err(Failure::usage(format!("invalid request JSON: {error}")));
        }
        Err(error) => return Err(Failure::usage(format!("invalid request JSON: {error}"))),
    };
    match value.get("schema").and_then(serde_json::Value::as_str) {
        Some(RequestSchema::ID) => serde_json::from_value(value)
            .map(NativeRequest::Query)
            .map_err(|error| Failure::usage(format!("invalid query request JSON: {error}"))),
        Some(ScopeRequestSchema::ID) => serde_json::from_value(value)
            .map(NativeRequest::Scope)
            .map_err(|error| Failure::usage(format!("invalid scope request JSON: {error}"))),
        Some(schema) => Err(Failure::usage(format!(
            "unsupported request schema '{schema}'; expected '{}' or '{}'",
            RequestSchema::ID,
            ScopeRequestSchema::ID
        ))),
        None => Err(Failure::usage(
            "request JSON requires a schema discriminator",
        )),
    }
}

pub(super) fn read_utf8_input(
    input: &mut dyn Read,
    limit: u64,
    label: &str,
) -> Result<String, Failure> {
    let bytes = read_input_bytes(input, limit, label)?;
    String::from_utf8(bytes).map_err(|_| Failure::usage(format!("{label} must be UTF-8")))
}

pub(super) fn read_input_bytes(
    input: &mut dyn Read,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    input
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::usage(format!("cannot read {label}: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(Failure::usage(format!(
            "{label} exceeds the {limit}-byte limit"
        )));
    }
    Ok(bytes)
}
