//! Versioned wire representation of normalized document IR.

use mant_ir::{
    Block, Diagnostic, Document as IrDocument, DocumentMeta, ParserInfo, Section, SourceKey,
    SourceRecord, validate_source_table,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Exact schema marker for a normalized structured document response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum DocumentSchema {
    /// Version 0.12 of the pre-stable structured-document protocol.
    #[serde(rename = "mant.document/v0.12")]
    V0Dot12,
}

impl DocumentSchema {
    /// Serialized identifier of the current document contract.
    pub const ID: &'static str = "mant.document/v0.12";
}

/// Identifies `ManT` and the parser used to build a wire document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Producer {
    /// Process implementation name.
    pub name: String,
    /// Process package version.
    pub version: String,
    /// Parser implementation, when an authoritative document was parsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
}

/// Parser implementation recorded at the process boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    /// Parser implementation name.
    pub name: String,
    /// Parser implementation version.
    pub version: String,
}

/// Closed source table and root identity shared by projected document results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceContext {
    /// Authored sources participating in this result, in dense key order.
    pub sources: Vec<SourceRecord>,
    /// Source containing the normalized document root.
    pub root_source: SourceKey,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceContextWire {
    sources: Vec<SourceRecord>,
    root_source: SourceKey,
}

impl<'de> Deserialize<'de> for SourceContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SourceContextWire::deserialize(deserializer)?;
        validate_source_table(&wire.sources, wire.root_source).map_err(serde::de::Error::custom)?;
        Ok(Self {
            sources: wire.sources,
            root_source: wire.root_source,
        })
    }
}

impl From<&IrDocument> for SourceContext {
    fn from(document: &IrDocument) -> Self {
        Self {
            sources: document.sources.clone(),
            root_source: document.root_source,
        }
    }
}

/// Serializable v0.12 envelope around `ManT`'s protocol-independent document IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentResponse {
    /// Exact response schema discriminator.
    pub schema: DocumentSchema,
    /// Process and parser provenance.
    pub producer: Producer,
    /// Source table and root identity, flattened as `sources`/`rootSource`.
    #[serde(flatten)]
    pub source_context: SourceContext,
    /// Source-neutral document metadata.
    pub meta: DocumentMeta,
    /// Original visible heading; independent from bibliographic metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<mant_ir::Heading>,
    /// Exact source fragments resolving to the normalized document root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<mant_ir::FragmentAlias>,
    /// Recoverable parsing and validation findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Content preceding the first section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Top-level semantic sections in source order.
    pub sections: Vec<Section>,
}

#[derive(Deserialize)]
#[serde(
    remote = "DocumentResponse",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct DocumentResponseWire {
    pub schema: DocumentSchema,
    pub producer: Producer,
    #[serde(flatten)]
    pub source_context: SourceContext,
    pub meta: DocumentMeta,
    #[serde(default)]
    pub heading: Option<mant_ir::Heading>,
    #[serde(default)]
    pub fragment_aliases: Vec<mant_ir::FragmentAlias>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    pub sections: Vec<Section>,
}

impl<'de> Deserialize<'de> for DocumentResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let response = DocumentResponseWire::deserialize(deserializer)?;
        let document: IrDocument = response.clone().into();
        mant_ir::validate_document_sources(&document).map_err(serde::de::Error::custom)?;
        Ok(response)
    }
}

pub(crate) fn validate_optional_source_spans(
    source_context: Option<&SourceContext>,
    spans: impl IntoIterator<Item = mant_ir::SourceSpan>,
) -> Result<(), String> {
    for span in spans {
        let context = source_context
            .ok_or_else(|| "source-qualified span requires a source context".to_owned())?;
        mant_ir::validate_source_span_relation(&context.sources, span)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

impl Producer {
    /// Construct process provenance for a normalized document.
    #[must_use]
    pub fn for_document(document: &IrDocument) -> Self {
        Self {
            name: "mant".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            engine: document.parser.as_ref().map(|parser| Engine {
                name: parser.name.clone(),
                version: parser.version.clone(),
            }),
        }
    }
}

impl From<&IrDocument> for DocumentResponse {
    fn from(document: &IrDocument) -> Self {
        Self {
            schema: DocumentSchema::V0Dot12,
            producer: Producer::for_document(document),
            source_context: SourceContext::from(document),
            meta: document.meta.clone(),
            heading: document.heading.clone(),
            fragment_aliases: document.fragment_aliases.clone(),
            diagnostics: document.diagnostics.clone(),
            blocks: document.blocks.clone(),
            sections: document.sections.clone(),
        }
    }
}

impl From<DocumentResponse> for IrDocument {
    fn from(document: DocumentResponse) -> Self {
        Self {
            parser: document.producer.engine.map(|engine| ParserInfo {
                name: engine.name,
                version: engine.version,
            }),
            sources: document.source_context.sources,
            root_source: document.source_context.root_source,
            meta: document.meta,
            heading: document.heading,
            fragment_aliases: document.fragment_aliases,
            diagnostics: document.diagnostics,
            blocks: document.blocks,
            sections: document.sections,
        }
    }
}
