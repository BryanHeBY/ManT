//! Direct native structured-document lowering for the C03 vertical slice.

use std::collections::HashSet;

use libmandoc_rs::structured::{NativeBlockKind, NativeTargetOrigin};
use libmandoc_rs::{InputFormat, SourceBundle};
#[cfg(test)]
use mant_ir::{Block, Inline, ListKind};
use mant_ir::{Document, DocumentMeta, ParserInfo, validate_document};

use super::projection::{NativeProjectionError, NativeProseProjection, project_native_prose};
use crate::definitions::NativeHeadEvidence;

mod blocks;
mod content;
mod evidence;
mod index;

use blocks::{lower_block, lower_section, push_lowered_block};
use content::lower_diagnostics;
use index::NativeLoweringIndex;

/// Run the private C03 entry from native execution through stable semantic IR.
pub(crate) fn project_native_manual(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
) -> Result<Document, NativeProjectionError> {
    let projection = project_native_prose(root, bundle, format)?;
    lower_projection(&projection)
}

fn lower_projection(projection: &NativeProseProjection) -> Result<Document, NativeProjectionError> {
    let native = projection.document();
    let index = NativeLoweringIndex::new(native)?;
    let mut used_ids = HashSet::new();
    let explicit_targets = native
        .items()
        .iter()
        .filter(|item| item.target_origin() == Some(NativeTargetOrigin::Authored))
        .filter_map(|item| item.target().map(ToOwned::to_owned))
        .collect::<HashSet<_>>();
    used_ids.extend(explicit_targets.iter().cloned());

    let mut root_blocks = Vec::new();
    let mut sections = Vec::new();
    let mut evidence = NativeHeadEvidence::default();
    for &block_index in index.block_children(None) {
        let block = &native.blocks()[block_index];
        if block.kind() == NativeBlockKind::Heading {
            sections.push(lower_section(
                projection,
                &index,
                block,
                &mut used_ids,
                &mut evidence,
            )?);
        } else {
            push_lowered_block(
                &mut root_blocks,
                lower_block(projection, &index, block, None, &mut evidence)?,
            );
        }
    }
    super::navigation::normalize_generated_anchors(
        &mut root_blocks,
        &mut sections,
        &explicit_targets,
    );
    crate::definitions::identify_definitions_with_evidence(
        &mut root_blocks,
        &mut sections,
        &explicit_targets,
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
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics: lower_diagnostics(projection),
        blocks: root_blocks,
        sections,
    };
    document
        .diagnostics
        .extend(crate::definitions::manual_discovery_diagnostics(
            &document.sections,
        ));
    document.diagnostics.extend(validate_document(&document));
    Ok(document)
}

#[cfg(test)]
mod tests;
