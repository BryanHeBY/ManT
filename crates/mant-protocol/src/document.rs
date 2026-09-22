//! Versioned wire representation of normalized document IR.

use mant_ir::{
    Block, ContentContext, ContentProjection, ContentStore, Diagnostic, Document as IrDocument,
    DocumentMeta, Heading, ParserInfo, Section, SourceKey, SourceRecord, validate_source_table,
    visit::{self, Visit},
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
    /// Authoritative logical content referenced by all retained IR below.
    pub content_store: ContentStore,
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
    pub content_store: ContentStore,
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
        mant_ir::validate_content_store(&document.content_store)
            .map_err(serde::de::Error::custom)?;
        validate_content_context(
            document.content(),
            &document.content_store,
            document.heading.as_ref(),
            &document.blocks,
            &document.sections,
        )
        .map_err(serde::de::Error::custom)?;
        // Complete documents, unlike excerpt/explanation projections, must
        // account for every non-opportunity atom exactly once. Reuse the IR
        // coverage rule without imposing it on partial response envelopes.
        if let Some(finding) = mant_ir::validate_document(&document)
            .into_iter()
            .find(|finding| {
                matches!(
                    finding.code.as_deref(),
                    Some("ir.invalid-content-coverage" | "ir.invalid-content-reference")
                )
            })
        {
            return Err(serde::de::Error::custom(finding.message));
        }
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

pub(crate) fn validate_projection_sources(
    source_context: Option<&SourceContext>,
    projection: Option<&ContentProjection>,
) -> Result<(), String> {
    let Some(projection) = projection else {
        return Ok(());
    };
    let document = IrDocument {
        parser: None,
        sources: source_context.map_or_else(Vec::new, |context| context.sources.clone()),
        root_source: source_context.map_or(SourceKey::FIRST, |context| context.root_source),
        content_store: projection.content_store.clone(),
        meta: DocumentMeta::default(),
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections: Vec::new(),
    };
    if source_context.is_none() && mant_ir::document_has_source_spans(&document) {
        return Err("source-qualified content projection requires a source context".to_owned());
    }
    match source_context {
        Some(_) => mant_ir::validate_document_sources(&document).map_err(|error| error.to_string()),
        None => Ok(()),
    }
}

/// Require one response-local store whenever a response retains key-backed IR,
/// then prove that every retained inline leaf resolves through that store.
pub(crate) fn validate_projected_content<'a>(
    projection: Option<&'a ContentProjection>,
    heading: Option<&'a Heading>,
    blocks: &'a [Block],
    sections: &'a [Section],
) -> Result<(), String> {
    let retains_ir = heading.is_some() || !blocks.is_empty() || !sections.is_empty();
    let Some(projection) = projection else {
        return if retains_ir {
            Err("retained document content requires a content projection".to_owned())
        } else {
            Ok(())
        };
    };

    validate_content_context(
        projection.content(),
        &projection.content_store,
        heading,
        blocks,
        sections,
    )
}

#[allow(clippy::too_many_lines)]
fn validate_content_context<'a>(
    content: ContentContext<'a>,
    store: &'a ContentStore,
    heading: Option<&'a Heading>,
    blocks: &'a [Block],
    sections: &'a [Section],
) -> Result<(), String> {
    // Explanation and excerpt envelopes may present the same authoritative
    // atom in both a support and a selected view. Validate each inline
    // container's exact ranges and wrappers, not global atom uniqueness.
    struct ProjectionValidator<'store> {
        content: ContentContext<'store>,
        positions: mant_ir::InlinePositionIndex<'store>,
        store: &'store ContentStore,
        link: Option<mant_ir::LinkOccurrenceKey>,
        strong_depth: usize,
        emphasis_depth: usize,
        linked_leaf_count: usize,
        invalid: bool,
    }
    impl<'store> Visit<'store> for ProjectionValidator<'store> {
        fn visit_heading(&mut self, heading: &'store mant_ir::Heading) {
            self.invalid |= !self.positions.is_positioned(&heading.content);
            visit::walk_heading(self, heading);
        }

        fn visit_block(&mut self, block: &'store mant_ir::Block) {
            if let mant_ir::Block::Paragraph { children, .. }
            | mant_ir::Block::Preformatted { children, .. } = block
            {
                self.invalid |= !self.positions.is_positioned(children);
            }
            visit::walk_block(self, block);
        }

        fn visit_definition_item(&mut self, item: &'store mant_ir::DefinitionItem) {
            for term in &item.terms {
                self.invalid |= !self.positions.is_positioned(term);
            }
            if item.entry.is_some()
                && self
                    .content
                    .entry_forms(mant_ir::EntryOwner::Definition(item))
                    .ok()
                    .flatten()
                    .is_none()
            {
                self.invalid = true;
                return;
            }
            visit::walk_definition_item(self, item);
        }

        fn visit_list_item(&mut self, item: &'store mant_ir::ListItem) {
            if item.entry.is_some()
                && self
                    .content
                    .entry_forms(mant_ir::EntryOwner::List(item))
                    .ok()
                    .flatten()
                    .is_none()
            {
                self.invalid = true;
                return;
            }
            visit::walk_list_item(self, item);
        }

        fn visit_inline(&mut self, inline: &'store mant_ir::Inline) {
            if self.content.inline(inline).is_err() {
                self.invalid = true;
                return;
            }
            match inline {
                mant_ir::Inline::Text { content } | mant_ir::Inline::Code { content } => {
                    if self.link.is_some() {
                        self.linked_leaf_count = self.linked_leaf_count.saturating_add(1);
                    }
                    let Some(atom) = self.store.atom(content.atom) else {
                        self.invalid = true;
                        return;
                    };
                    self.invalid |= atom.link != self.link
                        || atom.kind.text().is_none_or(|text| {
                            content.bytes.start != 0 || content.bytes.end as usize != text.len()
                        })
                        || (self.strong_depth > 0 && !atom.style.strong)
                        || (self.emphasis_depth > 0 && !atom.style.emphasis)
                        || matches!(inline, mant_ir::Inline::Code { .. }) != atom.style.literal;
                }
                mant_ir::Inline::LineBreak { atom } => {
                    if self.link.is_some() {
                        self.linked_leaf_count = self.linked_leaf_count.saturating_add(1);
                    }
                    self.invalid |= self
                        .store
                        .atom(*atom)
                        .is_none_or(|record| record.link != self.link);
                }
                mant_ir::Inline::Link {
                    occurrence,
                    children,
                } => {
                    self.invalid |= self.link.is_some();
                    let outer = self.link.replace(*occurrence);
                    let before = self.linked_leaf_count;
                    for child in children {
                        self.visit_inline(child);
                    }
                    if self.linked_leaf_count == before
                        && self
                            .store
                            .link(*occurrence)
                            .is_some_and(|link| !link.label.is_empty())
                    {
                        self.invalid = true;
                    }
                    self.link = outer;
                }
                mant_ir::Inline::Strong { children } => {
                    self.strong_depth = self.strong_depth.saturating_add(1);
                    for child in children {
                        self.visit_inline(child);
                    }
                    self.strong_depth -= 1;
                }
                mant_ir::Inline::Emphasis { children } => {
                    self.emphasis_depth = self.emphasis_depth.saturating_add(1);
                    for child in children {
                        self.visit_inline(child);
                    }
                    self.emphasis_depth -= 1;
                }
                mant_ir::Inline::Anchor { .. } => {}
            }
        }
    }

    let positions = content.inline_position_index().ok_or_else(|| {
        "retained document content has invalid logical scalar positions".to_owned()
    })?;
    let mut validator = ProjectionValidator {
        content,
        positions,
        store,
        link: None,
        strong_depth: 0,
        emphasis_depth: 0,
        linked_leaf_count: 0,
        invalid: false,
    };
    if let Some(heading) = heading {
        validator.visit_heading(heading);
    }
    for block in blocks {
        validator.visit_block(block);
    }
    for section in sections {
        validator.visit_section(section);
    }
    if validator.invalid {
        Err("retained document content does not resolve in its content projection".to_owned())
    } else {
        Ok(())
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
            source_context: SourceContext::from(document),
            content_store: document.content_store.clone(),
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
            content_store: document.content_store,
            meta: document.meta,
            heading: document.heading,
            fragment_aliases: document.fragment_aliases,
            diagnostics: document.diagnostics,
            blocks: document.blocks,
            sections: document.sections,
        }
    }
}
