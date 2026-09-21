//! Direct native structured-document lowering for the C03 vertical slice.

use std::{collections::HashSet, ops::Range};

use libmandoc_rs::structured::{
    ContentAtomKind, ContentRootKey, NativeBlock, NativeBlockKind, NativeItem, NativeLinkTarget,
    NativeListKind, NativeRole, ProvenanceKey, StructuredDocument,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{
    Block, DefinitionItem, DefinitionLayout, Diagnostic, DiagnosticImpact, DiagnosticLevel,
    Document, DocumentMeta, Heading, Inline, LayoutHint, LinkTarget, ListItem, ListItemLayout,
    ListKind, NodeId, ParserInfo, Provenance, Section, SourceSpan, validate_document,
};

use super::projection::{NativeProjectionError, NativeProseProjection, project_native_prose};
use crate::definitions::{
    NativeContentRange, NativeDeclarationEvidence, NativeHeadEvidence, NativeHeadRole,
};

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
    let mut used_ids = HashSet::new();
    let mut reserved_targets = native
        .items()
        .iter()
        .filter_map(|item| item.target().map(ToOwned::to_owned))
        .collect::<HashSet<_>>();
    used_ids.extend(reserved_targets.iter().cloned());

    let mut root_blocks = Vec::new();
    let mut sections = Vec::new();
    let mut evidence = NativeHeadEvidence::default();
    for block in native
        .blocks()
        .iter()
        .filter(|block| block.parent().is_none())
    {
        if block.kind() == NativeBlockKind::Heading {
            sections.push(lower_section(
                projection,
                block,
                &mut used_ids,
                &mut evidence,
            )?);
        } else {
            push_lowered_block(
                &mut root_blocks,
                lower_block(projection, block, None, &mut evidence)?,
            );
        }
    }
    for section in &sections {
        reserved_targets.insert(section.id.to_string());
    }
    crate::definitions::identify_definitions_with_evidence(
        &mut root_blocks,
        &mut sections,
        &reserved_targets,
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

fn lower_section(
    projection: &NativeProseProjection,
    block: &NativeBlock,
    used_ids: &mut HashSet<String>,
    evidence: &mut NativeHeadEvidence,
) -> Result<Section, NativeProjectionError> {
    let heading_content = root_inlines(
        projection.document(),
        block.root().ok_or(NativeProjectionError::InvalidRelation(
            "heading block has no root",
        ))?,
    )?;
    let label = mant_ir::inline_plain_text(&heading_content);
    let base = crate::definitions::document_id_slug(&label);
    let id = unique_id(if base.is_empty() { "section" } else { &base }, used_ids);
    let mut blocks = Vec::new();
    for child in projection
        .document()
        .blocks()
        .iter()
        .filter(|child| child.parent() == Some(block.key()))
    {
        push_lowered_block(&mut blocks, lower_block(projection, child, None, evidence)?);
    }
    Ok(Section {
        id: NodeId::new(id),
        fragment_aliases: Vec::new(),
        heading: Heading {
            content: heading_content,
            source: source_for(projection, block.provenance()),
        },
        spacing_before_lines: 0,
        blocks,
        children: Vec::new(),
        source: source_for(projection, block.provenance()),
    })
}

fn lower_block(
    projection: &NativeProseProjection,
    block: &NativeBlock,
    owner: Option<libmandoc_rs::structured::OwnerKey>,
    evidence: &mut NativeHeadEvidence,
) -> Result<Block, NativeProjectionError> {
    if owner.is_some_and(|owner| owner != block.owner()) {
        return Err(NativeProjectionError::InvalidRelation(
            "list child block belongs to another item",
        ));
    }
    match block.kind() {
        NativeBlockKind::Paragraph => Ok(Block::Paragraph {
            children: root_inlines(
                projection.document(),
                block.root().ok_or(NativeProjectionError::InvalidRelation(
                    "paragraph block has no root",
                ))?,
            )?,
            layout: LayoutHint::default(),
            source: source_for(projection, block.provenance()),
        }),
        NativeBlockKind::List | NativeBlockKind::DefinitionList => {
            lower_list(projection, block, evidence)
        }
        kind => Err(NativeProjectionError::UnsupportedBlock(kind)),
    }
}

fn lower_list(
    projection: &NativeProseProjection,
    block: &NativeBlock,
    evidence: &mut NativeHeadEvidence,
) -> Result<Block, NativeProjectionError> {
    let native = projection.document();
    let list = native
        .lists()
        .iter()
        .find(|list| list.block() == block.key())
        .ok_or(NativeProjectionError::InvalidRelation(
            "list block has no list record",
        ))?;
    if list.kind() == NativeListKind::NativeMarker {
        return Err(NativeProjectionError::InvalidRelation(
            "native marker list reached the codec without source classification",
        ));
    }
    if list.kind() == NativeListKind::Definition {
        let mut items = Vec::new();
        for item in native
            .items()
            .iter()
            .filter(|item| item.list() == list.key())
        {
            items.push(lower_definition_item(projection, block, item, evidence)?);
        }
        return Ok(Block::DefinitionList {
            items,
            declaration_groups: Vec::new(),
            compact: list.compact(),
            layout: LayoutHint::default(),
            source: source_for(projection, list.provenance()),
        });
    }

    let kind = match list.kind() {
        NativeListKind::Bullet => ListKind::Bullet,
        NativeListKind::Ordered => ListKind::Ordered {
            start: list.start().map(u64::from),
        },
        NativeListKind::Plain => ListKind::Plain,
        NativeListKind::Definition | NativeListKind::NativeMarker => unreachable!(),
    };
    let mut items = Vec::new();
    for item in native
        .items()
        .iter()
        .filter(|item| item.list() == list.key())
    {
        items.push(lower_list_item(projection, block, item, evidence)?);
    }
    Ok(Block::List {
        kind,
        compact: list.compact(),
        items,
        layout: LayoutHint::default(),
        source: source_for(projection, list.provenance()),
    })
}

fn lower_definition_item(
    projection: &NativeProseProjection,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<DefinitionItem, NativeProjectionError> {
    let native = projection.document();
    let source = source_for(projection, item.provenance());
    let mut terms = Vec::new();
    for root in item_term_roots(native, item)? {
        terms.push(root_inlines(native, root)?);
    }
    if let Some(target) = item.target() {
        let anchor = Inline::anchor_at(target, source);
        if let Some(term) = terms.first_mut() {
            term.insert(0, anchor);
        } else {
            terms.push(vec![anchor]);
        }
    }
    let lowered = DefinitionItem {
        source,
        entry: None,
        terms,
        description: item_blocks(projection, list_block, item, evidence)?,
        layout: DefinitionLayout::default(),
    };
    if let Some(role) = evidence_role(native, item) {
        evidence.record(&lowered, role);
    }
    evidence.record_declaration(&lowered, native_declaration_evidence(native, item)?);
    Ok(lowered)
}

fn lower_list_item(
    projection: &NativeProseProjection,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<ListItem, NativeProjectionError> {
    let source = source_for(projection, item.provenance());
    let mut blocks = item_blocks(projection, list_block, item, evidence)?;
    if let Some(target) = item.target() {
        prepend_anchor(&mut blocks, Inline::anchor_at(target, source));
    }
    Ok(ListItem {
        layout: ListItemLayout::default(),
        source,
        entry: None,
        blocks,
    })
}

fn item_blocks(
    projection: &NativeProseProjection,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<Vec<Block>, NativeProjectionError> {
    let mut blocks = Vec::new();
    for child in
        projection.document().blocks().iter().filter(|child| {
            child.parent() == Some(list_block.key()) && child.owner() == item.owner()
        })
    {
        push_lowered_block(
            &mut blocks,
            lower_block(projection, child, Some(item.owner()), evidence)?,
        );
    }
    Ok(blocks)
}

fn item_term_roots(
    document: &StructuredDocument,
    item: &NativeItem,
) -> Result<Vec<ContentRootKey>, NativeProjectionError> {
    let forms = document.forms().get(item.forms().clone()).ok_or(
        NativeProjectionError::InvalidRelation("item form range is invalid"),
    )?;
    let mut roots = Vec::new();
    for form in forms {
        let reference = document.content_refs().get(form.refs().start).ok_or(
            NativeProjectionError::InvalidRelation("form has no first content reference"),
        )?;
        let root = document
            .content_atom(reference.atom())
            .ok_or(NativeProjectionError::InvalidRelation(
                "form references an unknown atom",
            ))?
            .root();
        if roots.last() != Some(&root) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn native_declaration_evidence(
    document: &StructuredDocument,
    item: &NativeItem,
) -> Result<NativeDeclarationEvidence, NativeProjectionError> {
    let roots = item_term_roots(document, item)?;
    let forms = document.forms().get(item.forms().clone()).ok_or(
        NativeProjectionError::InvalidRelation("item form range is invalid"),
    )?;
    let mut form_ranges = Vec::new();
    for form in forms {
        form_ranges.push(content_range_for_refs(
            document,
            &roots,
            form.refs().clone(),
        )?);
    }
    let mut name_hints = Vec::new();
    for hint in document
        .name_hints()
        .iter()
        .filter(|hint| forms.iter().any(|form| form.key() == hint.form()))
    {
        name_hints.push(content_range_for_refs(
            document,
            &roots,
            hint.refs().clone(),
        )?);
    }
    Ok(NativeDeclarationEvidence {
        forms: form_ranges,
        name_hints,
    })
}

fn content_range_for_refs(
    document: &StructuredDocument,
    roots: &[ContentRootKey],
    refs: Range<usize>,
) -> Result<NativeContentRange, NativeProjectionError> {
    let references =
        document
            .content_refs()
            .get(refs)
            .ok_or(NativeProjectionError::InvalidRelation(
                "declaration content range is invalid",
            ))?;
    let first = references
        .first()
        .ok_or(NativeProjectionError::InvalidRelation(
            "declaration content range is empty",
        ))?;
    let root = document
        .content_atom(first.atom())
        .ok_or(NativeProjectionError::InvalidRelation(
            "declaration content references an unknown atom",
        ))?
        .root();
    let term = roots
        .iter()
        .position(|candidate| *candidate == root)
        .ok_or(NativeProjectionError::InvalidRelation(
            "declaration content is outside the item term roots",
        ))?;
    let mut offset = 0_usize;
    let mut atom_offsets = Vec::new();
    for atom in document
        .content_atoms()
        .iter()
        .filter(|atom| atom.root() == root)
    {
        atom_offsets.push((atom.key(), offset));
        offset += match atom.kind() {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                text.len()
            }
            ContentAtomKind::HardBreak => 1,
            ContentAtomKind::BreakOpportunity => 0,
        };
    }
    let mut parts: Vec<Range<usize>> = Vec::new();
    for reference in references {
        let atom = document.content_atom(reference.atom()).ok_or(
            NativeProjectionError::InvalidRelation(
                "declaration content references an unknown atom",
            ),
        )?;
        if atom.root() != root {
            return Err(NativeProjectionError::InvalidRelation(
                "one declaration range crosses term roots",
            ));
        }
        let atom_offset = atom_offsets
            .iter()
            .find_map(|(key, offset)| (*key == atom.key()).then_some(*offset))
            .ok_or(NativeProjectionError::InvalidRelation(
                "declaration atom has no root-relative offset",
            ))?;
        let range = atom_offset + reference.bytes().start as usize
            ..atom_offset + reference.bytes().end as usize;
        if let Some(previous) = parts.last_mut()
            && previous.end == range.start
        {
            previous.end = range.end;
        } else {
            parts.push(range);
        }
    }
    Ok(NativeContentRange { term, parts })
}

fn root_inlines(
    document: &StructuredDocument,
    root: ContentRootKey,
) -> Result<Vec<Inline>, NativeProjectionError> {
    let mut inlines = Vec::new();
    for atom in document
        .content_atoms()
        .iter()
        .filter(|atom| atom.root() == root)
    {
        let leaf = match atom.kind() {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                if atom.style().is_literal() {
                    Inline::Code {
                        value: text.clone(),
                    }
                } else {
                    Inline::Text {
                        value: text.clone(),
                    }
                }
            }
            ContentAtomKind::BreakOpportunity => continue,
            ContentAtomKind::HardBreak => Inline::LineBreak,
        };
        let mut leaf = if atom.style().is_italic() {
            Inline::Emphasis {
                children: vec![leaf],
            }
        } else {
            leaf
        };
        if atom.style().is_bold() || atom.style().is_underline() {
            leaf = Inline::Strong {
                children: vec![leaf],
            };
        }
        if let Some(link) = atom.link() {
            let link = document
                .link(link)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "atom references an unknown link",
                ))?;
            leaf = Inline::Link {
                target: lower_link(link.target()),
                title: link.title().map(ToOwned::to_owned),
                children: vec![leaf],
            };
        }
        inlines.push(leaf);
    }
    Ok(inlines)
}

fn lower_link(target: &NativeLinkTarget) -> LinkTarget {
    match target {
        NativeLinkTarget::External(uri) => LinkTarget::External { uri: uri.clone() },
        NativeLinkTarget::Email(address) => LinkTarget::Email {
            address: address.clone(),
        },
        NativeLinkTarget::Document(name) => LinkTarget::Document {
            name: name.clone(),
            fragment: None,
        },
        NativeLinkTarget::Manual { name, section } => LinkTarget::Manual {
            name: name.clone(),
            manual_section: Some(section.clone()),
        },
        NativeLinkTarget::Section(id) => LinkTarget::Section {
            id: NodeId::new(id.clone()),
        },
    }
}

fn evidence_role(document: &StructuredDocument, item: &NativeItem) -> Option<NativeHeadRole> {
    let forms = document.forms().get(item.forms().clone())?;
    let mut roles = forms
        .iter()
        .filter_map(libmandoc_rs::structured::NativeForm::role);
    let first = roles.next()?;
    if !roles.all(|role| role == first) {
        return None;
    }
    Some(match first {
        NativeRole::Flag => NativeHeadRole::Option,
        NativeRole::EnvironmentVariable => NativeHeadRole::Environment,
        NativeRole::Argument | NativeRole::CommandOrDirective | NativeRole::Path => {
            NativeHeadRole::Literal
        }
    })
}

fn source_for(projection: &NativeProseProjection, key: ProvenanceKey) -> Option<SourceSpan> {
    match projection.provenances().get(key.get() as usize - 1)? {
        Provenance::Authored { span } => Some(*span),
        Provenance::Generated { .. } | Provenance::Unknown => None,
    }
}

fn lower_diagnostics(projection: &NativeProseProjection) -> Vec<Diagnostic> {
    projection
        .document()
        .diagnostics()
        .iter()
        .map(|diagnostic| Diagnostic {
            level: match diagnostic.level() {
                libmandoc_rs::structured::StructuredDiagnosticLevel::Style => {
                    DiagnosticLevel::Style
                }
                libmandoc_rs::structured::StructuredDiagnosticLevel::Warning => {
                    DiagnosticLevel::Warning
                }
                libmandoc_rs::structured::StructuredDiagnosticLevel::Error => {
                    DiagnosticLevel::Error
                }
                libmandoc_rs::structured::StructuredDiagnosticLevel::Unsupported => {
                    DiagnosticLevel::Unsupported
                }
            },
            impact: DiagnosticImpact::None,
            code: Some(diagnostic.code().as_str().to_owned()),
            message: diagnostic.message().to_owned(),
            source: diagnostic
                .span()
                .and_then(|span| projection.spans().get(span.get() as usize - 1).copied()),
        })
        .collect()
}

fn prepend_anchor(blocks: &mut Vec<Block>, anchor: Inline) {
    if let Some(Block::Paragraph { children, .. }) = blocks.first_mut() {
        children.insert(0, anchor);
    } else {
        blocks.insert(
            0,
            Block::Paragraph {
                children: vec![anchor],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    }
}

fn push_lowered_block(blocks: &mut Vec<Block>, block: Block) {
    let merge = match (blocks.last(), &block) {
        (
            Some(Block::List {
                kind: prior_kind,
                items: prior_items,
                ..
            }),
            Block::List { kind, .. },
        ) => match (prior_kind, kind) {
            (ListKind::Bullet, ListKind::Bullet) | (ListKind::Plain, ListKind::Plain) => true,
            (ListKind::Ordered { start: Some(prior) }, ListKind::Ordered { start: Some(next) }) => {
                prior
                    .checked_add(prior_items.len() as u64)
                    .is_some_and(|expected| expected == *next)
            }
            _ => false,
        },
        _ => false,
    };
    if merge {
        let Some(Block::List {
            compact: prior_compact,
            items: prior_items,
            ..
        }) = blocks.last_mut()
        else {
            unreachable!()
        };
        let Block::List { compact, items, .. } = block else {
            unreachable!()
        };
        *prior_compact &= compact;
        prior_items.extend(items);
    } else {
        blocks.push(block);
    }
}

fn unique_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_owned()) {
        return base.to_owned();
    }
    for suffix in 2_u64.. {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests;
