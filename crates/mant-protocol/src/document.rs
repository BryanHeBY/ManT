//! Versioned wire representation of normalized document IR.

use mant_ir::{
    Block, Diagnostic, Document as IrDocument, DocumentMeta, DocumentSource, ParserInfo, Section,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Exact schema marker for a normalized structured document response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum DocumentSchema {
    #[serde(rename = "mant.document/v7")]
    V7,
}

/// Identifies `ManT` and the parser used to build a wire document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Producer {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
}

/// Parser implementation recorded at the process boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub name: String,
    pub version: String,
}

/// Serializable v7 envelope around `ManT`'s protocol-independent document IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentResponse {
    pub schema: DocumentSchema,
    pub producer: Producer,
    pub source: DocumentSource,
    pub meta: DocumentMeta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    pub sections: Vec<Section>,
}

impl Producer {
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
            schema: DocumentSchema::V7,
            producer: Producer::for_document(document),
            source: document.source.clone(),
            meta: document.meta.clone(),
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
            source: document.source,
            meta: document.meta,
            diagnostics: document.diagnostics,
            blocks: document.blocks,
            sections: document.sections,
        }
    }
}
