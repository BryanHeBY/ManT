//! Direct native structured-document lowering for the C03 vertical slice.

use libmandoc_rs::structured::NativeBlockKind;
use libmandoc_rs::{InputFormat, SourceBundle};
#[cfg(test)]
use mant_ir::{Block, Inline, ListKind};
use mant_ir::{Document, DocumentMeta, ParserInfo, validate_document};

use super::projection::{NativeProjectionError, NativeProseProjection, project_native_prose};
use crate::definitions::NativeHeadEvidence;

mod address;
mod blocks;
mod content;
mod evidence;
mod index;
mod store;
mod table;

use address::AddressPlan;
use blocks::{lower_block, lower_section, push_lowered_block};
use content::lower_diagnostics;
use index::NativeLoweringIndex;
use store::NativeContentMap;

/// Run the private C03 entry from native execution through stable semantic IR.
pub(crate) fn project_native_manual(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
) -> Result<Document, NativeProjectionError> {
    let projection = project_native_prose(root, bundle, format)?;
    lower_projection(projection)
}

fn lower_projection(
    mut projection: NativeProseProjection,
) -> Result<Document, NativeProjectionError> {
    let index = NativeLoweringIndex::new(projection.document())?;
    let addresses = AddressPlan::build(&projection)?;
    let content = NativeContentMap::build(&mut projection, &addresses)?;
    let native = projection.document();

    let mut root_blocks = Vec::new();
    let mut sections = Vec::new();
    let mut evidence = NativeHeadEvidence::default();
    for &block_index in index.block_children(None) {
        let block = &native.blocks()[block_index];
        if block.kind() == NativeBlockKind::Heading {
            sections.push(lower_section(
                &projection,
                &index,
                block,
                &addresses,
                &content,
                &mut evidence,
            )?);
        } else {
            push_lowered_block(
                &mut root_blocks,
                lower_block(
                    &projection,
                    &index,
                    &addresses,
                    &content,
                    block,
                    None,
                    &mut evidence,
                )?,
            );
        }
    }
    let mut content_store = content.into_store();
    crate::definitions::identify_definitions_with_evidence(
        &mut content_store,
        &mut root_blocks,
        &mut sections,
        addresses.reserved(),
        native.metadata().name(),
        &evidence,
    );

    let metadata = native.metadata();
    let mut document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc-structured".to_owned(),
            version: libmandoc_rs::LIBMANDOC_VERSION.to_owned(),
        }),
        sources: projection.sources().to_vec(),
        root_source: projection.root_source(),
        body: mant_ir::DocumentBody::Flow(mant_ir::FlowBody {
            content_store,
            heading: None,
            blocks: root_blocks,
            sections,
        }),
        meta: DocumentMeta {
            title: metadata.title().map(ToOwned::to_owned),
            manual_section: metadata.section().map(ToOwned::to_owned),
            date: metadata.date().map(ToOwned::to_owned),
            volume: metadata.volume().map(ToOwned::to_owned),
            os: metadata.operating_system().map(ToOwned::to_owned),
            arch: metadata.architecture().map(ToOwned::to_owned),
            names: metadata.name().map(ToOwned::to_owned).into_iter().collect(),
            alias_target: metadata.alias_target().map(ToOwned::to_owned),
        },
        fragment_aliases: Vec::new(),
        diagnostics: lower_diagnostics(&projection),
    };
    document
        .diagnostics
        .extend_from_slice(addresses.diagnostics());
    if let mant_ir::DocumentBody::Flow(flow) = &document.body {
        let discovery = crate::definitions::manual_discovery_diagnostics(
            flow.content_store.content(),
            &flow.sections,
        );
        document.diagnostics.extend(discovery);
    }
    document.diagnostics.extend(validate_document(&document));
    Ok(document)
}

#[cfg(test)]
mod tests;
