//! Private vertical projection over the native structured atom store.
//!
//! This is deliberately not a `mant_ir::Document`: C02b exercises native
//! prose without copying it into the legacy string-owning inline model. The
//! structured document remains the sole body owner and every projected leaf
//! is only a typed key into that document.

use std::{collections::HashMap, ops::Range};

use libmandoc_rs::structured::{
    self, ContentAtomKey, ContentAtomKind, ContentOwnerKind, ContentRootKey, ContentRootKind,
    LinkOccurrenceKey, NativeBlockKey, NativeBlockKind, NativeRole, OwnerKey, ProvenanceKey,
    StructuredDocument, StructuredError, StructuredStyle,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{
    Provenance, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
    SourceRelationError, SourceSpan, TextRange, TextSize, validate_source_span_relation,
    validate_source_table,
};

/// A C02b-only projection failure. Unsupported native structure is rejected
/// atomically rather than returned as an apparently complete prose document.
#[derive(Debug)]
pub(crate) enum NativeProjectionError {
    Native(StructuredError),
    Source(SourceRelationError),
    UnsupportedOwner(ContentOwnerKind),
    UnsupportedRoot(ContentRootKind),
    UnsupportedBlock(NativeBlockKind),
    MissingSourcePosition,
    InvalidRelation(&'static str),
}

impl From<StructuredError> for NativeProjectionError {
    fn from(error: StructuredError) -> Self {
        Self::Native(error)
    }
}

impl From<SourceRelationError> for NativeProjectionError {
    fn from(error: SourceRelationError) -> Self {
        Self::Source(error)
    }
}

/// One wrapper run over adjacent atoms with identical native facts.
///
/// The run owns no label, target, title, or body string. Link identity is the
/// typed occurrence key in the retained structured document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeInlineRun {
    pub(crate) owner: OwnerKey,
    pub(crate) root: ContentRootKey,
    pub(crate) style: StructuredStyle,
    pub(crate) role: Option<NativeRole>,
    pub(crate) link: Option<LinkOccurrenceKey>,
    pub(crate) leaves: Range<usize>,
}

/// One logical leaf in native root order. The atom store owns all text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeInlineLeaf {
    Text(ContentAtomKey),
    Whitespace(ContentAtomKey),
    BreakOpportunity(ContentAtomKey),
    HardBreak(ContentAtomKey),
}

impl NativeInlineLeaf {
    pub(crate) const fn atom(self) -> ContentAtomKey {
        match self {
            Self::Text(atom)
            | Self::Whitespace(atom)
            | Self::BreakOpportunity(atom)
            | Self::HardBreak(atom) => atom,
        }
    }
}

/// A projected logical root whose runs contain keys rather than copied text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeProseRoot {
    pub(crate) key: ContentRootKey,
    pub(crate) owner: OwnerKey,
    pub(crate) provenance: ProvenanceKey,
    pub(crate) runs: Vec<NativeInlineRun>,
    pub(crate) leaves: Vec<NativeInlineLeaf>,
}

impl NativeProseRoot {
    fn run_leaves(&self, run: &NativeInlineRun) -> Option<&[NativeInlineLeaf]> {
        self.leaves.get(run.leaves.clone())
    }
}

/// A supported prose block pointing at one projected logical root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeProseBlock {
    pub(crate) key: NativeBlockKey,
    pub(crate) owner: OwnerKey,
    pub(crate) kind: NativeBlockKind,
    pub(crate) parent: Option<NativeBlockKey>,
    pub(crate) root: Option<ContentRootKey>,
    pub(crate) provenance: ProvenanceKey,
}

/// Private structured-prose view used only by the new feature-gated path.
///
/// `document` remains the only owner of atom text, link targets, and titles.
/// The other fields contain source facts or typed relations only.
#[derive(Debug)]
pub(crate) struct NativeProseProjection {
    document: StructuredDocument,
    sources: Vec<SourceRecord>,
    root_source: SourceKey,
    spans: Vec<SourceSpan>,
    provenances: Vec<Provenance>,
    roots: Vec<NativeProseRoot>,
    blocks: Vec<NativeProseBlock>,
}

impl NativeProseProjection {
    pub(super) fn new(document: StructuredDocument) -> Result<Self, NativeProjectionError> {
        let sources = project_sources(&document)?;
        let root_source = source_key(document.root_source())?;
        validate_source_table(&sources, root_source)?;

        let mut spans = fallible_vec(document.spans().len(), "source span projection allocation")?;
        for span in document.spans() {
            spans.push(project_span(span, &sources)?);
        }
        let mut provenances = fallible_vec(
            document.provenances().len(),
            "provenance projection allocation",
        )?;
        for provenance in document.provenances() {
            provenances.push(project_provenance(provenance, &spans)?);
        }
        validate_owners(&document, &provenances)?;
        let mut roots = project_roots(&document, &provenances)?;
        project_atoms(&document, &provenances, &mut roots)?;
        validate_links(&document, &roots)?;
        let blocks = project_blocks(&document, &provenances, &roots)?;

        Ok(Self {
            document,
            sources,
            root_source,
            spans,
            provenances,
            roots,
            blocks,
        })
    }

    pub(crate) fn document(&self) -> &StructuredDocument {
        &self.document
    }

    pub(crate) fn take_content_tables(
        &mut self,
    ) -> libmandoc_rs::structured::StructuredContentTables {
        self.document.take_content_tables()
    }

    pub(crate) fn sources(&self) -> &[SourceRecord] {
        &self.sources
    }

    pub(crate) const fn root_source(&self) -> SourceKey {
        self.root_source
    }

    pub(crate) fn spans(&self) -> &[SourceSpan] {
        &self.spans
    }

    pub(crate) fn provenances(&self) -> &[Provenance] {
        &self.provenances
    }

    pub(crate) fn roots(&self) -> &[NativeProseRoot] {
        &self.roots
    }

    pub(crate) fn root(&self, key: ContentRootKey) -> Option<&NativeProseRoot> {
        let index = usize::try_from(key.get()).ok()?.checked_sub(1)?;
        self.roots.get(index).filter(|root| root.key == key)
    }

    pub(crate) fn blocks(&self) -> &[NativeProseBlock] {
        &self.blocks
    }

    /// Resolve one projected leaf directly in the retained native atom store.
    pub(crate) fn resolve_leaf(&self, leaf: NativeInlineLeaf) -> Option<&str> {
        self.document
            .content_atom(leaf.atom())?
            .kind()
            .logical_text()
    }
}

/// Run the feature-gated native path without replacing the product parser.
pub(crate) fn project_native_prose(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
) -> Result<NativeProseProjection, NativeProjectionError> {
    let document = structured::render_bundle(root, bundle, format)?;
    NativeProseProjection::new(document)
}

fn validate_owners(
    document: &StructuredDocument,
    provenances: &[Provenance],
) -> Result<(), NativeProjectionError> {
    for owner in document.owners() {
        if !matches!(
            owner.kind(),
            ContentOwnerKind::Document
                | ContentOwnerKind::Section
                | ContentOwnerKind::Paragraph
                | ContentOwnerKind::ListItem
                | ContentOwnerKind::DefinitionItem
        ) {
            return Err(NativeProjectionError::UnsupportedOwner(owner.kind()));
        }
        provenance(provenances, owner.provenance())?;
    }
    Ok(())
}

fn project_roots(
    document: &StructuredDocument,
    provenances: &[Provenance],
) -> Result<Vec<NativeProseRoot>, NativeProjectionError> {
    let mut roots = Vec::new();
    roots
        .try_reserve_exact(document.content_roots().len())
        .map_err(|_| NativeProjectionError::InvalidRelation("root projection allocation"))?;
    for root in document.content_roots() {
        if !matches!(
            root.kind(),
            ContentRootKind::Heading | ContentRootKind::Term | ContentRootKind::Body
        ) {
            return Err(NativeProjectionError::UnsupportedRoot(root.kind()));
        }
        provenance(provenances, root.provenance())?;
        roots.push(NativeProseRoot {
            key: root.key(),
            owner: root.owner(),
            provenance: root.provenance(),
            runs: Vec::new(),
            leaves: Vec::new(),
        });
    }
    Ok(roots)
}

fn project_atoms(
    document: &StructuredDocument,
    provenances: &[Provenance],
    roots: &mut [NativeProseRoot],
) -> Result<(), NativeProjectionError> {
    let mut next_atom_ordinals = fallible_vec(roots.len(), "atom ordinal allocation")?;
    next_atom_ordinals.resize(roots.len(), 0_u32);
    let mut root_atom_counts = fallible_vec(roots.len(), "root atom count allocation")?;
    root_atom_counts.resize(roots.len(), 0_usize);
    for atom in document.content_atoms() {
        let root_index = one_based_index(
            atom.root().get(),
            "atom root key does not fit this platform",
        )?;
        let count =
            root_atom_counts
                .get_mut(root_index)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "atom references an unknown root",
                ))?;
        *count = count
            .checked_add(1)
            .ok_or(NativeProjectionError::InvalidRelation(
                "root atom count overflows",
            ))?;
    }
    for (root, count) in roots.iter_mut().zip(root_atom_counts) {
        root.leaves
            .try_reserve_exact(count)
            .map_err(|_| allocation("root leaf allocation"))?;
        root.runs
            .try_reserve_exact(count)
            .map_err(|_| allocation("root run allocation"))?;
    }
    for atom in document.content_atoms() {
        let root_index = one_based_index(
            atom.root().get(),
            "atom root key does not fit this platform",
        )?;
        let root = roots
            .get_mut(root_index)
            .ok_or(NativeProjectionError::InvalidRelation(
                "atom references an unknown root",
            ))?;
        let expected_ordinal = next_atom_ordinals.get_mut(root_index).ok_or(
            NativeProjectionError::InvalidRelation("atom ordinal state is missing"),
        )?;
        if atom.owner() != root.owner || atom.ordinal() != *expected_ordinal {
            return Err(NativeProjectionError::InvalidRelation(
                "root atom order or owner does not match",
            ));
        }
        *expected_ordinal =
            expected_ordinal
                .checked_add(1)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "atom ordinal overflows",
                ))?;
        provenance(provenances, atom.provenance())?;

        let leaf = match atom.kind() {
            ContentAtomKind::Text { .. } => NativeInlineLeaf::Text(atom.key()),
            ContentAtomKind::Whitespace { .. } => NativeInlineLeaf::Whitespace(atom.key()),
            ContentAtomKind::BreakOpportunity => NativeInlineLeaf::BreakOpportunity(atom.key()),
            ContentAtomKind::HardBreak => NativeInlineLeaf::HardBreak(atom.key()),
        };
        let joins_previous = root.runs.last().is_some_and(|run| {
            run.owner == atom.owner()
                && run.root == atom.root()
                && run.style == atom.style()
                && run.role == atom.role()
                && run.link == atom.link()
        });
        let leaf_index = root.leaves.len();
        root.leaves.push(leaf);
        if joins_previous {
            root.runs.last_mut().expect("run exists").leaves.end = leaf_index + 1;
        } else {
            root.runs.push(NativeInlineRun {
                owner: atom.owner(),
                root: atom.root(),
                style: atom.style(),
                role: atom.role(),
                link: atom.link(),
                leaves: leaf_index..leaf_index + 1,
            });
        }
    }
    Ok(())
}

fn project_blocks(
    document: &StructuredDocument,
    provenances: &[Provenance],
    roots: &[NativeProseRoot],
) -> Result<Vec<NativeProseBlock>, NativeProjectionError> {
    let mut used_roots = fallible_vec(roots.len(), "block root state allocation")?;
    used_roots.resize(roots.len(), false);
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(document.blocks().len())
        .map_err(|_| NativeProjectionError::InvalidRelation("block projection allocation"))?;
    for block in document.blocks() {
        if !matches!(
            block.kind(),
            NativeBlockKind::Heading
                | NativeBlockKind::Paragraph
                | NativeBlockKind::List
                | NativeBlockKind::DefinitionList
        ) {
            return Err(NativeProjectionError::UnsupportedBlock(block.kind()));
        }
        let root = block.root();
        if let Some(root) = root {
            let root_index =
                one_based_index(root.get(), "block root key does not fit this platform")?;
            let projected_root =
                roots
                    .get(root_index)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "prose block references an unknown root",
                    ))?;
            let used =
                used_roots
                    .get_mut(root_index)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "block root state is missing",
                    ))?;
            if projected_root.key != root || projected_root.owner != block.owner() || *used {
                return Err(NativeProjectionError::InvalidRelation(
                    "prose block root ownership is not unique",
                ));
            }
            *used = true;
        } else if !matches!(
            block.kind(),
            NativeBlockKind::List | NativeBlockKind::DefinitionList
        ) {
            return Err(NativeProjectionError::InvalidRelation(
                "prose block has no content root",
            ));
        }
        provenance(provenances, block.provenance())?;
        blocks.push(NativeProseBlock {
            key: block.key(),
            owner: block.owner(),
            kind: block.kind(),
            parent: block.parent(),
            root,
            provenance: block.provenance(),
        });
    }
    if roots
        .iter()
        .zip(&used_roots)
        .any(|(root, used)| root_kind(document, root.key) != Some(ContentRootKind::Term) && !used)
    {
        return Err(NativeProjectionError::InvalidRelation(
            "native prose root is not owned by exactly one block",
        ));
    }
    Ok(blocks)
}

fn root_kind(document: &StructuredDocument, key: ContentRootKey) -> Option<ContentRootKind> {
    document
        .content_root(key)
        .map(libmandoc_rs::structured::ContentRoot::kind)
}

fn one_based_index(one_based: u32, relation: &'static str) -> Result<usize, NativeProjectionError> {
    one_based
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or(NativeProjectionError::InvalidRelation(relation))
}

fn project_sources(
    document: &StructuredDocument,
) -> Result<Vec<SourceRecord>, NativeProjectionError> {
    let mut projected = fallible_vec(document.sources().len(), "source projection allocation")?;
    for source in document.sources() {
        projected.push(SourceRecord {
            key: source_key(source.key())?,
            identity: match source.identity() {
                structured::SourceIdentity::Path(name) => SourceIdentity::Path {
                    name: fallible_string(name, "source path allocation")?,
                },
                structured::SourceIdentity::BundleMember(name) => SourceIdentity::BundleMember {
                    name: fallible_string(name, "source member allocation")?,
                },
                structured::SourceIdentity::Anonymous(name) => SourceIdentity::Anonymous {
                    name: fallible_string(name, "anonymous source allocation")?,
                },
            },
            format: match source.format() {
                structured::SourceFormat::Man => SourceFormat::Man,
                structured::SourceFormat::Mdoc => SourceFormat::Mdoc,
                structured::SourceFormat::Markdown => SourceFormat::Markdown,
            },
            decoded_byte_length: source.decoded_byte_len(),
            content_sha256: source.content_sha256().copied(),
            coordinates: match source.coordinates() {
                structured::SourceCoordinates::DecodedUtf8Bytes => {
                    SourceCoordinates::DecodedUtf8Bytes
                }
                structured::SourceCoordinates::NativeNormalizedBytes => {
                    SourceCoordinates::NativeNormalizedBytes
                }
            },
        });
    }
    Ok(projected)
}

fn allocation(relation: &'static str) -> NativeProjectionError {
    NativeProjectionError::InvalidRelation(relation)
}

fn fallible_vec<T>(
    capacity: usize,
    relation: &'static str,
) -> Result<Vec<T>, NativeProjectionError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| allocation(relation))?;
    Ok(values)
}

fn fallible_string(value: &str, relation: &'static str) -> Result<String, NativeProjectionError> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| allocation(relation))?;
    owned.push_str(value);
    Ok(owned)
}

fn project_span(
    span: &structured::SourceSpan,
    sources: &[SourceRecord],
) -> Result<SourceSpan, NativeProjectionError> {
    let position = span
        .line_columns()
        .ok_or(NativeProjectionError::MissingSourcePosition)?;
    let start = position.start();
    let (end_line, end_column) = position
        .end()
        .map_or((None, None), |end| (Some(end.line()), Some(end.column())));
    let projected = SourceSpan {
        source: source_key(span.source())?,
        byte_range: span.byte_range().map(|range| TextRange {
            start: TextSize::new(range.start),
            end: TextSize::new(range.end),
        }),
        line: start.line(),
        column: start.column(),
        end_line,
        end_column,
    };
    validate_source_span_relation(sources, projected)?;
    Ok(projected)
}

fn project_provenance(
    provenance: &structured::Provenance,
    spans: &[SourceSpan],
) -> Result<Provenance, NativeProjectionError> {
    Ok(match provenance {
        structured::Provenance::Authored { span } => Provenance::Authored {
            span: *lookup(spans, span.get(), "authored provenance span")?,
        },
        structured::Provenance::Generated { trigger } => Provenance::Generated {
            trigger: trigger
                .map(|span| lookup(spans, span.get(), "generated provenance trigger").copied())
                .transpose()?,
        },
        structured::Provenance::Unknown => Provenance::Unknown,
    })
}

fn validate_links(
    document: &StructuredDocument,
    roots: &[NativeProseRoot],
) -> Result<(), NativeProjectionError> {
    for link in document.links() {
        provenance_key(document, link.provenance())?;
        let label = document
            .link_label(link)
            .ok_or(NativeProjectionError::InvalidRelation(
                "link label range is invalid",
            ))?;
        if label.is_empty() {
            return Err(NativeProjectionError::InvalidRelation(
                "link label must not be empty",
            ));
        }
        let mut previous_by_root = HashMap::new();
        for part in label {
            let atom_key = match part {
                structured::LinkLabelPart::Content(reference) => reference.atom(),
                structured::LinkLabelPart::HardBreak(atom) => *atom,
            };
            let atom =
                document
                    .content_atom(atom_key)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "link label references an unknown atom",
                    ))?;
            if atom.owner() != link.owner()
                || atom.link() != Some(link.key())
                || match part {
                    structured::LinkLabelPart::Content(reference) => {
                        document.resolve_content_ref(reference).is_none()
                    }
                    structured::LinkLabelPart::HardBreak(_) => {
                        !matches!(atom.kind(), structured::ContentAtomKind::HardBreak)
                    }
                }
            {
                return Err(NativeProjectionError::InvalidRelation(
                    "link label does not resolve to its linked owner atom",
                ));
            }
            if let Some(previous) = previous_by_root.insert(atom.root(), atom.ordinal()) {
                let root = roots
                    .get(one_based_index(atom.root().get(), "link label root key")?)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "link label references an unknown root",
                    ))?;
                let gap_start = usize::try_from(previous)
                    .ok()
                    .and_then(|ordinal| ordinal.checked_add(1));
                let gap_end = usize::try_from(atom.ordinal()).ok();
                let gap = gap_start
                    .zip(gap_end)
                    .and_then(|(start, end)| (start <= end).then_some(start..end))
                    .and_then(|range| root.leaves.get(range))
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "link label atom order is invalid within its root",
                    ))?;
                if !only_break_opportunities(gap.iter().map(|leaf| {
                    document
                        .content_atom(leaf.atom())
                        .expect("projected roots contain validated native atoms")
                        .kind()
                })) {
                    return Err(NativeProjectionError::InvalidRelation(
                        "link occurrence is interrupted by visible content within one root",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn only_break_opportunities<'a>(kinds: impl IntoIterator<Item = &'a ContentAtomKind>) -> bool {
    kinds
        .into_iter()
        .all(|kind| matches!(kind, ContentAtomKind::BreakOpportunity))
}

fn source_key(key: structured::SourceKey) -> Result<SourceKey, NativeProjectionError> {
    SourceKey::new(key.get()).ok_or(NativeProjectionError::InvalidRelation(
        "native SourceKey is zero",
    ))
}

fn provenance(
    provenances: &[Provenance],
    key: ProvenanceKey,
) -> Result<&Provenance, NativeProjectionError> {
    lookup(provenances, key.get(), "provenance")
}

fn provenance_key(
    document: &StructuredDocument,
    key: ProvenanceKey,
) -> Result<(), NativeProjectionError> {
    document
        .provenance(key)
        .map(|_| ())
        .ok_or(NativeProjectionError::InvalidRelation(
            "link references an unknown provenance",
        ))
}

fn lookup<'a, T>(
    values: &'a [T],
    one_based: u32,
    relation: &'static str,
) -> Result<&'a T, NativeProjectionError> {
    one_based
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| values.get(index))
        .ok_or(NativeProjectionError::InvalidRelation(relation))
}

#[cfg(test)]
mod tests;
