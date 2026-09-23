//! Stable document nodes independent from their source parser.
use crate::{Heading, NodeId};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

mod blocks;
mod diagnostic;
mod inline;
mod source;
pub use blocks::{
    Block, DefinitionItem, DefinitionLayout, LayoutHint, ListItem, ListItemLayout, ListKind,
    TableAlignment, TableCell, TableCellKind, TableRow, TableRowKind, TableRuleCellKind,
};
pub use diagnostic::{
    CoverageScope, Diagnostic, DiagnosticImpact, DiagnosticLevel, semantics_complete,
};
pub use inline::{Inline, LinkTarget};
pub use source::{
    Provenance, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
    SourceRelationError, SourceSpan, TextRange, TextSize,
};

/// The one authoritative primary body of a document.
///
/// The tagged wire form rejects old top-level Flow fields and cannot carry
/// both a logical content tree and an annotated native surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentBody {
    /// Source-neutral logical content and topology.
    Flow(FlowBody),
    /// Annotated native output and its validated mark relations.
    Fixed(crate::FixedBody),
}

/// The complete owned Flow arm; no second copy lives on [`Document`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlowBody {
    /// One authoritative logical content store.
    pub content_store: crate::ContentStore,
    /// Optional original visible document heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<Heading>,
    /// Content before the first section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Top-level semantic sections.
    pub sections: Vec<Section>,
}

/// Borrowed primary body arm; readers must explicitly handle Fixed.
#[derive(Debug, Clone, Copy)]
pub enum DocumentBodyRef<'a> {
    /// Source-neutral logical content tree.
    Flow(FlowBodyRef<'a>),
    /// Annotated native output and validated marks.
    Fixed(&'a crate::FixedBody),
}

/// Borrowed fields that together form the one existing Flow body.
#[derive(Debug, Clone, Copy)]
pub struct FlowBodyRef<'a> {
    /// One authoritative logical content store.
    pub content_store: &'a crate::ContentStore,
    /// Optional original visible document heading.
    pub heading: &'a Option<Heading>,
    /// Content before the first section.
    pub blocks: &'a [Block],
    /// Top-level semantic sections.
    pub sections: &'a [Section],
}

/// Mutable borrowed primary body arm, used only while constructing Flow IR.
#[derive(Debug)]
pub enum DocumentBodyMut<'a> {
    /// Mutable access to the Flow body fields.
    Flow(FlowBodyMut<'a>),
    /// Mutable annotated native output and marks.
    Fixed(&'a mut crate::FixedBody),
}

/// Mutable borrowing of the current Flow body without a second owned model.
#[derive(Debug)]
pub struct FlowBodyMut<'a> {
    /// One authoritative logical content store.
    pub content_store: &'a mut crate::ContentStore,
    /// Optional original visible document heading.
    pub heading: &'a mut Option<Heading>,
    /// Content before the first section.
    pub blocks: &'a mut Vec<Block>,
    /// Top-level semantic sections.
    pub sections: &'a mut Vec<Section>,
}

/// A normalized document ready for interactive or textual rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    /// Parser provenance retained independently from process-protocol metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser: Option<ParserInfo>,
    /// All authored sources participating in this document, in key order.
    pub sources: Vec<SourceRecord>,
    /// Source containing the normalized document root.
    pub root_source: SourceKey,
    /// Exactly one primary content representation.
    pub body: DocumentBody,
    /// Metadata normalized across all supported source formats.
    pub meta: DocumentMeta,
    /// Exact source fragments resolving to the normalized document root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<crate::FragmentAlias>,
    /// Recoverable findings retained for callers that need source quality data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    /// Access the owned Flow body only when this document is Flow.
    #[must_use]
    pub const fn flow(&self) -> Option<&FlowBody> {
        match &self.body {
            DocumentBody::Flow(flow) => Some(flow),
            DocumentBody::Fixed(_) => None,
        }
    }

    /// Mutably access the owned Flow body only when this document is Flow.
    #[must_use]
    pub fn flow_mut(&mut self) -> Option<&mut FlowBody> {
        match &mut self.body {
            DocumentBody::Flow(flow) => Some(flow),
            DocumentBody::Fixed(_) => None,
        }
    }

    /// Borrow the primary body through an exhaustive arm match.
    #[must_use]
    pub fn body(&self) -> DocumentBodyRef<'_> {
        match &self.body {
            DocumentBody::Flow(flow) => DocumentBodyRef::Flow(FlowBodyRef {
                content_store: &flow.content_store,
                heading: &flow.heading,
                blocks: &flow.blocks,
                sections: &flow.sections,
            }),
            DocumentBody::Fixed(fixed) => DocumentBodyRef::Fixed(fixed),
        }
    }

    /// Mutably borrow the primary body through an exhaustive arm match.
    #[must_use]
    pub fn body_mut(&mut self) -> DocumentBodyMut<'_> {
        match &mut self.body {
            DocumentBody::Flow(flow) => DocumentBodyMut::Flow(FlowBodyMut {
                content_store: &mut flow.content_store,
                heading: &mut flow.heading,
                blocks: &mut flow.blocks,
                sections: &mut flow.sections,
            }),
            DocumentBody::Fixed(fixed) => DocumentBodyMut::Fixed(fixed),
        }
    }

    /// Resolve a document-local source key.
    #[must_use]
    pub fn source_record(&self, key: SourceKey) -> Option<&SourceRecord> {
        let index = usize::try_from(key.get() - 1).ok()?;
        self.sources.get(index).filter(|source| source.key == key)
    }

    /// Return the source record containing the normalized document root.
    #[must_use]
    pub fn root_source_record(&self) -> Option<&SourceRecord> {
        self.source_record(self.root_source)
    }

    /// Return the root source's syntax family when the source table is valid.
    #[must_use]
    pub fn root_format(&self) -> Option<SourceFormat> {
        self.root_source_record().map(|source| source.format)
    }

    /// Return the root source's caller-facing path-like identity, when present.
    #[must_use]
    pub fn root_path(&self) -> Option<&str> {
        self.root_source_record()
            .and_then(|source| source.identity.path())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DocumentWire {
    #[serde(default)]
    parser: Option<ParserInfo>,
    sources: Vec<SourceRecord>,
    root_source: SourceKey,
    body: DocumentBody,
    meta: DocumentMeta,
    #[serde(default)]
    fragment_aliases: Vec<crate::FragmentAlias>,
    #[serde(default)]
    diagnostics: Vec<Diagnostic>,
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = DocumentWire::deserialize(deserializer)?;
        let document = Self {
            parser: wire.parser,
            sources: wire.sources,
            root_source: wire.root_source,
            body: wire.body,
            meta: wire.meta,
            fragment_aliases: wire.fragment_aliases,
            diagnostics: wire.diagnostics,
        };
        match &document.body {
            DocumentBody::Flow(flow) => crate::validate_content_store(&flow.content_store)
                .map_err(serde::de::Error::custom)?,
            DocumentBody::Fixed(fixed) => fixed.validate().map_err(serde::de::Error::custom)?,
        }
        crate::validate_document_sources(&document).map_err(serde::de::Error::custom)?;
        Ok(document)
    }
}

/// Parser implementation that produced this normalized document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParserInfo {
    /// Stable parser implementation name.
    pub name: String,
    /// Parser version used to produce the document.
    pub version: String,
}

/// Metadata normalized from TH, Dt, and the validated libmandoc result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    /// Native bibliographic title; Markdown visible titles belong to `FlowBody::heading`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Native manual category such as `1` or `3p`; unrelated to document headings.
    pub manual_section: Option<String>,
    /// Normalized publication or revision date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Manual volume or collection label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    /// Operating-system label declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Architecture qualifier declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Primary name followed by any aliases from the NAME section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Logical target of a native `.so` alias page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_target: Option<String>,
}

/// Source-neutral document section headed by Markdown, man, or mdoc content.
///
/// This is a content subtree, not the native manual category stored in
/// [`DocumentMeta::manual_section`]. Depth is derived from tree position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Section {
    /// Unique within one document; consumers must not treat it as a global ID.
    pub id: NodeId,
    /// Exact source fragments resolving to this normalized section identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<crate::FragmentAlias>,
    /// Authoritative visible heading content, including links and styles.
    pub heading: Heading,
    /// Terminal rows requested before this heading by the source macro set.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
    /// Content directly owned by this section.
    pub blocks: Vec<Block>,
    /// Nested subsections in source order.
    pub children: Vec<Section>,
    /// Heading location in the original source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

#[cfg(test)]
mod body_tests {
    use super::*;
    use crate::{DisplaySurface, SourceCoordinates, SourceIdentity};

    fn document(body: DocumentBody) -> Document {
        Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "body-wire".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            body,
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn primary_body_wire_is_tagged_exclusive_and_round_trips() {
        let flow = document(DocumentBody::Flow(FlowBody {
            content_store: crate::ContentStore::default(),
            heading: None,
            blocks: Vec::new(),
            sections: Vec::new(),
        }));
        let fixed = document(DocumentBody::Fixed(crate::FixedBody {
            surface: DisplaySurface {
                text: String::new(),
                rows: Vec::new(),
                runs: Vec::new(),
            },
            headings: Vec::new(),
            owners: Vec::new(),
            links: Vec::new(),
            anchors: Vec::new(),
            regions: Vec::new(),
        }));
        for (document, kind) in [(flow, "flow"), (fixed, "fixed")] {
            let wire = serde_json::to_value(&document).unwrap();
            assert_eq!(wire["body"]["kind"], kind);
            assert_eq!(serde_json::from_value::<Document>(wire).unwrap(), document);
        }
    }

    #[test]
    fn retired_top_level_and_mixed_body_fields_are_rejected() {
        let mut wire = serde_json::to_value(document(DocumentBody::Flow(FlowBody {
            content_store: crate::ContentStore::default(),
            heading: None,
            blocks: Vec::new(),
            sections: Vec::new(),
        })))
        .unwrap();
        let store = wire["body"]["contentStore"].clone();
        wire["contentStore"] = store;
        assert!(serde_json::from_value::<Document>(wire.clone()).is_err());
        wire.as_object_mut().unwrap().remove("contentStore");
        wire["body"]["surface"] = serde_json::json!({"text":"","rows":[],"runs":[]});
        assert!(serde_json::from_value::<Document>(wire.clone()).is_err());
        wire["body"].as_object_mut().unwrap().remove("surface");
        wire["body"].as_object_mut().unwrap().remove("kind");
        assert!(serde_json::from_value::<Document>(wire).is_err());
    }
}
