use std::ops::Range;

use libmandoc_rs::structured::{
    ContentAtomKey, ContentRootKey, NativeItem, NativeRole, StructuredDocument,
};

use super::{NativeProjectionError, NativeProseProjection, store::NativeContentMap};
use crate::definitions::{NativeContentRange, NativeDeclarationEvidence, NativeHeadRole};

pub(super) fn item_term_roots(
    document: &StructuredDocument,
    content: &NativeContentMap,
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
        let root = native_root(content, reference.atom())?;
        if roots.last().is_some_and(|previous| *previous > root) {
            return Err(NativeProjectionError::InvalidRelation(
                "item term roots are not ordered",
            ));
        }
        if roots.last() != Some(&root) {
            roots.push(root);
        }
    }
    Ok(roots)
}

pub(super) fn native_declaration_evidence(
    projection: &NativeProseProjection,
    content: &NativeContentMap,
    item: &NativeItem,
) -> Result<NativeDeclarationEvidence, NativeProjectionError> {
    let document = projection.document();
    let roots = item_term_roots(document, content, item)?;
    let mut root_offsets = Vec::new();
    root_offsets
        .try_reserve_exact(roots.len())
        .map_err(|_| NativeProjectionError::InvalidRelation("term offset allocation"))?;
    for root in &roots {
        root_offsets.push(root_atom_offsets(content, *root)?);
    }
    let forms = document.forms().get(item.forms().clone()).ok_or(
        NativeProjectionError::InvalidRelation("item form range is invalid"),
    )?;
    let mut form_ranges = Vec::new();
    for form in forms {
        form_ranges.push(content_range_for_refs(
            document,
            content,
            &roots,
            &root_offsets,
            form.refs().clone(),
        )?);
    }
    let mut name_hints = Vec::new();
    if forms.is_empty() {
        return Ok(NativeDeclarationEvidence {
            forms: form_ranges,
            name_hints,
        });
    }
    let first_form = forms
        .first()
        .ok_or(NativeProjectionError::InvalidRelation("item has no forms"))?
        .key()
        .get();
    let last_form = forms
        .last()
        .ok_or(NativeProjectionError::InvalidRelation("item has no forms"))?
        .key()
        .get();
    let hints = document.name_hints();
    let hint_start = hints.partition_point(|hint| hint.form().get() < first_form);
    let hint_end = hints.partition_point(|hint| hint.form().get() <= last_form);
    for hint in &hints[hint_start..hint_end] {
        name_hints.push(content_range_for_refs(
            document,
            content,
            &roots,
            &root_offsets,
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
    content: &NativeContentMap,
    roots: &[ContentRootKey],
    root_offsets: &[Vec<(ContentAtomKey, usize)>],
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
    let root = native_root(content, first.atom())?;
    let term = roots.binary_search(&root).map_err(|_| {
        NativeProjectionError::InvalidRelation("declaration content is outside the item term roots")
    })?;
    let atom_offsets = root_offsets
        .get(term)
        .ok_or(NativeProjectionError::InvalidRelation(
            "term root has no atom offset index",
        ))?;
    let mut parts: Vec<Range<usize>> = Vec::new();
    for reference in references {
        if native_root(content, reference.atom())? != root {
            return Err(NativeProjectionError::InvalidRelation(
                "one declaration range crosses term roots",
            ));
        }
        let atom_offset = atom_offsets
            .binary_search_by_key(&reference.atom(), |(key, _)| *key)
            .ok()
            .map(|index| atom_offsets[index].1)
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

fn root_atom_offsets(
    content: &NativeContentMap,
    root: ContentRootKey,
) -> Result<Vec<(ContentAtomKey, usize)>, NativeProjectionError> {
    let public_root = content.root(root)?;
    let projected =
        content
            .store()
            .root(public_root)
            .ok_or(NativeProjectionError::InvalidRelation(
                "term root has no public content record",
            ))?;
    let mut offsets = Vec::new();
    offsets
        .try_reserve_exact(projected.atoms.len())
        .map_err(|_| NativeProjectionError::InvalidRelation("term atom offset allocation"))?;
    let mut offset = 0_usize;
    for public_atom in &projected.atoms {
        let atom =
            content
                .store()
                .atom(*public_atom)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "projected atom has no public content record",
                ))?;
        let native_atom = content.native_atom(*public_atom)?;
        offsets.push((native_atom, offset));
        offset += match &atom.kind {
            mant_ir::ContentAtomKind::Text { text, .. }
            | mant_ir::ContentAtomKind::Whitespace { text, .. } => text.len(),
            mant_ir::ContentAtomKind::HardBreak {} => 1,
            mant_ir::ContentAtomKind::BreakOpportunity {} => 0,
        };
    }
    Ok(offsets)
}

fn native_root(
    content: &NativeContentMap,
    atom: ContentAtomKey,
) -> Result<ContentRootKey, NativeProjectionError> {
    let atom =
        content
            .store()
            .atom(content.atom(atom)?)
            .ok_or(NativeProjectionError::InvalidRelation(
                "native atom has no public content record",
            ))?;
    content.native_root(atom.root)
}

pub(super) fn evidence_role(
    document: &StructuredDocument,
    item: &NativeItem,
) -> Option<NativeHeadRole> {
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
