//! Versioned wire representation of normalized document IR.

use mant_ir::{
    Block, Diagnostic, Document as IrDocument, DocumentMeta, DocumentSource, ParserInfo, Section,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// Serializable v0.12 envelope around `ManT`'s protocol-independent document IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    try_from = "DocumentResponseUnchecked"
)]
pub struct DocumentResponse {
    /// Exact response schema discriminator.
    pub schema: DocumentSchema,
    /// Process and parser provenance.
    pub producer: Producer,
    /// Original source identity.
    pub source: DocumentSource,
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

// The wire reader checks the equation text compatibility projection before
// handing a document to any consumer. Its parsed structure remains the only
// authority even when a JSON producer supplies both fields.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DocumentResponseUnchecked {
    /// Exact response schema discriminator.
    schema: DocumentSchema,
    /// Process and parser provenance.
    producer: Producer,
    /// Original source identity.
    source: DocumentSource,
    /// Source-neutral document metadata.
    meta: DocumentMeta,
    /// Original visible heading; independent from bibliographic metadata.
    #[serde(default)]
    heading: Option<mant_ir::Heading>,
    /// Exact source fragments resolving to the normalized document root.
    #[serde(default)]
    fragment_aliases: Vec<mant_ir::FragmentAlias>,
    /// Recoverable parsing and validation findings.
    #[serde(default)]
    diagnostics: Vec<Diagnostic>,
    /// Content preceding the first section.
    #[serde(default)]
    blocks: Vec<Block>,
    /// Top-level semantic sections in source order.
    sections: Vec<Section>,
}

impl TryFrom<DocumentResponseUnchecked> for DocumentResponse {
    type Error = &'static str;

    fn try_from(value: DocumentResponseUnchecked) -> Result<Self, Self::Error> {
        use mant_ir::visit::{self, Visit};

        struct ProjectionCheck(bool);
        impl<'ir> Visit<'ir> for ProjectionCheck {
            fn visit_block(&mut self, block: &'ir Block) {
                if let Block::Equation {
                    value,
                    expression: Some(expression),
                    ..
                } = block
                {
                    self.0 &= *value == expression.readable_text();
                }
                visit::walk_block(self, block);
            }

            fn visit_inline(&mut self, inline: &'ir mant_ir::Inline) {
                if let mant_ir::Inline::Equation { value, expression } = inline {
                    self.0 &= *value == expression.readable_text();
                }
                visit::walk_inline(self, inline);
            }
        }

        let response = Self {
            schema: value.schema,
            producer: value.producer,
            source: value.source,
            meta: value.meta,
            heading: value.heading,
            fragment_aliases: value.fragment_aliases,
            diagnostics: value.diagnostics,
            blocks: value.blocks,
            sections: value.sections,
        };
        let mut check = ProjectionCheck(true);
        if let Some(heading) = &response.heading {
            check.visit_heading(heading);
        }
        for block in &response.blocks {
            check.visit_block(block);
        }
        for section in &response.sections {
            check.visit_section(section);
        }
        check
            .0
            .then_some(response)
            .ok_or("equation text does not match its parsed structure")
    }
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
            source: document.source.clone(),
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
            source: document.source,
            meta: document.meta,
            heading: document.heading,
            fragment_aliases: document.fragment_aliases,
            diagnostics: document.diagnostics,
            blocks: document.blocks,
            sections: document.sections,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_wire_rejects_conflicting_equation_text() {
        let expression = mant_ir::EquationExpression {
            kind: mant_ir::EquationKind::Text,
            font: mant_ir::EquationFont::None,
            position: mant_ir::EquationPosition::None,
            size: None,
            expected_args: Some(0),
            actual_args: 0,
            text: Some("x".into()),
            left: None,
            right: None,
            top: None,
            bottom: None,
            children: Vec::new(),
        };
        let document = IrDocument {
            parser: None,
            source: DocumentSource {
                format: mant_ir::SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            heading: None,
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: vec![
                Block::Equation {
                    value: "x".into(),
                    expression: Some(expression.clone()),
                    display: true,
                    layout: mant_ir::LayoutHint::default(),
                    source: None,
                },
                Block::Paragraph {
                    children: vec![mant_ir::Inline::Equation {
                        value: "x".into(),
                        expression,
                    }],
                    layout: mant_ir::LayoutHint::default(),
                    source: None,
                },
            ],
            sections: Vec::new(),
        };
        let original = serde_json::to_value(DocumentResponse::from(&document)).unwrap();
        assert!(serde_json::from_value::<DocumentResponse>(original.clone()).is_ok());
        let mut bad_block = original.clone();
        bad_block["blocks"][0]["value"] = "wrong".into();
        assert!(serde_json::from_value::<DocumentResponse>(bad_block).is_err());
        let mut bad_inline = original.clone();
        bad_inline["blocks"][1]["children"][0]["value"] = "wrong".into();
        assert!(serde_json::from_value::<DocumentResponse>(bad_inline).is_err());

        let mut unknown_inline = original.clone();
        unknown_inline["blocks"][1]["children"][0]["legacyText"] = "x".into();
        assert!(serde_json::from_value::<DocumentResponse>(unknown_inline).is_err());
        let mut unknown_box = original;
        unknown_box["blocks"][0]["expression"]["legacyChildren"] = 1.into();
        assert!(serde_json::from_value::<DocumentResponse>(unknown_box).is_err());
    }
}
