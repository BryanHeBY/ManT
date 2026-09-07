//! Source-neutral document, link and source-coordinate validation.
mod document;
mod links;
mod snapshot;
mod source;
pub use document::{is_normalized_node_id, is_semantic_completeness_diagnostic, validate_document};
pub use links::{
    email_address_from_mailto_uri, is_valid_email_address, is_valid_external_uri,
    mailto_uri_for_email_address,
};
pub use snapshot::DocumentValidation;
