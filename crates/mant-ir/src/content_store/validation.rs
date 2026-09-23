use std::{collections::HashMap, error::Error, fmt};

use crate::LinkTarget;

use super::{ContentAtomKind, ContentStore, LinkLabelPart, PointBoundary};

/// Structural error in a document or response-local content store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentStoreError {
    detail: String,
}

impl fmt::Display for ContentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for ContentStoreError {}

/// Validate dense keys and every closed relation inside a content store.
///
/// # Errors
///
/// Returns an error for a non-dense table, dangling relation, invalid UTF-8
/// byte range, owner/root mismatch, duplicate label part, or incomplete link.
#[allow(clippy::missing_panics_doc, clippy::too_many_lines)]
pub fn validate_content_store(store: &ContentStore) -> Result<(), ContentStoreError> {
    validate_dense(&store.owners, |record| record.key.get(), "owner")?;
    validate_dense(&store.roots, |record| record.key.get(), "root")?;
    validate_dense(&store.atoms, |record| record.key.get(), "atom")?;
    validate_dense(&store.points, |record| record.key.get(), "point")?;
    validate_dense(&store.links, |record| record.key.get(), "link")?;

    let mut root_membership = vec![0_u8; store.roots.len()];
    for owner in &store.owners {
        if owner
            .roots
            .windows(2)
            .any(|pair| pair[0].get() >= pair[1].get())
        {
            return invalid("content owner roots must follow dense source order");
        }
        for &root in &owner.roots {
            let Some(record) = store.root(root) else {
                return invalid("content owner references an unknown root");
            };
            if record.owner != owner.key {
                return invalid("content owner/root relation is inconsistent");
            }
            let count = &mut root_membership[root.index().expect("validated key fits usize")];
            *count = count.saturating_add(1);
        }
    }
    if root_membership.iter().any(|count| *count != 1) {
        return invalid("every content root must occur in exactly one owner");
    }

    let mut atom_membership = vec![0_u8; store.atoms.len()];
    let mut point_membership = vec![0_u8; store.points.len()];
    for root in &store.roots {
        if store.owner(root.owner).is_none() {
            return invalid("content root references an unknown owner");
        }
        if root
            .atoms
            .windows(2)
            .any(|pair| pair[0].get() >= pair[1].get())
        {
            return invalid("content root atoms must follow retained execution order");
        }
        for &atom in &root.atoms {
            let Some(record) = store.atom(atom) else {
                return invalid("content root references an unknown atom");
            };
            if record.root != root.key || record.owner != root.owner {
                return invalid("content root/atom relation is inconsistent");
            }
            let count = &mut atom_membership[atom.index().expect("validated key fits usize")];
            *count = count.saturating_add(1);
        }
        let mut previous_point_scalar = None;
        for &point in &root.points {
            let Some(record) = store.point(point) else {
                return invalid("content root references an unknown point");
            };
            if record.root != root.key || record.owner != root.owner {
                return invalid("content root/point relation is inconsistent");
            }
            if previous_point_scalar.is_some_and(|previous| previous > record.scalar_boundary) {
                return invalid("content root points must follow logical root order");
            }
            previous_point_scalar = Some(record.scalar_boundary);
            let count = &mut point_membership[point.index().expect("validated key fits usize")];
            *count = count.saturating_add(1);
        }
    }
    if atom_membership.iter().any(|count| *count != 1) {
        return invalid("every content atom must occur in exactly one root");
    }
    if point_membership.iter().any(|count| *count != 1) {
        return invalid("every content point must occur in exactly one root");
    }

    for atom in &store.atoms {
        if store.owner(atom.owner).is_none() || store.root(atom.root).is_none() {
            return invalid("content atom references an unknown root or owner");
        }
        if let ContentAtomKind::Text {
            text,
            display_override: Some(display),
        }
        | ContentAtomKind::Whitespace {
            text,
            display_override: Some(display),
            ..
        } = &atom.kind
        {
            // term.c::encode1()/logical_emit() project one logical scalar to
            // profile glyphs. A multiline or terminal-control projection
            // cannot share that scalar's row and hit-test coordinates.
            if text.chars().count() != 1 {
                return invalid("display override must project one logical scalar");
            }
            if display.is_empty()
                || display.chars().any(|character| {
                    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
                })
            {
                return invalid("display override must contain only inline printable glyphs");
            }
        }
        if matches!(atom.kind, ContentAtomKind::BreakOpportunity {}) && atom.link.is_some() {
            return invalid("break opportunities cannot belong to a link occurrence");
        }
        if atom.link.is_some_and(|key| store.link(key).is_none()) {
            return invalid("content atom references an unknown link occurrence");
        }
    }

    // Compute each root-relative scalar prefix once. Points may be numerous
    // in one root; repeatedly summing all earlier atoms would make admission
    // quadratic in the number of zero-width destinations.
    let mut atom_scalar_prefix = vec![0_u32; store.atoms.len()];
    let mut root_scalar_total = vec![0_u32; store.roots.len()];
    // Root points are ordered by scalar boundary. Once a point is accepted,
    // later points in the same atom can only move forward. Count just the
    // newly crossed UTF-8 slice instead of rescanning a long atom for every
    // zero-width destination at (or near) its end.
    let mut in_atom_progress = vec![(0_usize, 0_u32); store.atoms.len()];
    for root in &store.roots {
        let mut offset = 0_u32;
        for &key in &root.atoms {
            atom_scalar_prefix[key.index().expect("validated atom key fits usize")] = offset;
            offset = offset
                .checked_add(
                    atom_scalar_len(&store.atom(key).expect("validated atom").kind).ok_or_else(
                        || ContentStoreError {
                            detail: "content root scalar length exceeds u32".to_owned(),
                        },
                    )?,
                )
                .ok_or_else(|| ContentStoreError {
                    detail: "content root scalar length exceeds u32".to_owned(),
                })?;
        }
        root_scalar_total[root.key.index().expect("validated root key fits usize")] = offset;
    }

    for point in &store.points {
        let Some(root) = store.root(point.root) else {
            return invalid("content point references an unknown root");
        };
        if root.owner != point.owner {
            return invalid("content point owner differs from its root owner");
        }
        let expected_scalar = match point.boundary {
            PointBoundary::BetweenAtoms { atom_boundary } => {
                if atom_boundary as usize > root.atoms.len() {
                    return invalid("content point lies outside its root atom boundaries");
                }
                root.atoms.get(atom_boundary as usize).map_or_else(
                    || root_scalar_total[root.key.index().expect("validated root key fits usize")],
                    |key| atom_scalar_prefix[key.index().expect("validated atom key fits usize")],
                )
            }
            PointBoundary::InAtom { atom, byte_offset } => {
                let Some(record) = store.atom(atom).filter(|record| record.root == root.key) else {
                    return invalid("in-atom point references an atom outside its root");
                };
                let Some(text) = record.kind.text() else {
                    return invalid("in-atom point must reference text or whitespace");
                };
                if !text.is_char_boundary(byte_offset as usize) {
                    return invalid("in-atom point is not a UTF-8 boundary");
                }
                let index = atom.index().expect("validated atom key fits usize");
                let prefix = atom_scalar_prefix[index];
                let end = byte_offset as usize;
                let (last_byte, last_scalar) = &mut in_atom_progress[index];
                let (slice, prior) = if end >= *last_byte {
                    (&text[*last_byte..end], *last_scalar)
                } else {
                    // An out-of-order point is invalid once its claimed
                    // scalar is checked below. Keep that diagnostic rather
                    // than assuming input validation has already succeeded.
                    (&text[..end], 0)
                };
                let counted = prior
                    .checked_add(u32::try_from(slice.chars().count()).map_err(|_| {
                        ContentStoreError {
                            detail: "content point scalar boundary exceeds u32".to_owned(),
                        }
                    })?)
                    .ok_or_else(|| ContentStoreError {
                        detail: "content point scalar boundary exceeds u32".to_owned(),
                    })?;
                if end >= *last_byte {
                    *last_byte = end;
                    *last_scalar = counted;
                }
                prefix
                    .checked_add(counted)
                    .ok_or_else(|| ContentStoreError {
                        detail: "content point scalar boundary exceeds u32".to_owned(),
                    })?
            }
        };
        if expected_scalar != point.scalar_boundary {
            return invalid("content point scalar boundary disagrees with its structural boundary");
        }
    }

    let mut linked_parts = vec![0_u8; store.atoms.len()];
    let mut root_positions = vec![usize::MAX; store.atoms.len()];
    for root in &store.roots {
        for (position, atom) in root.atoms.iter().enumerate() {
            root_positions[atom.index().expect("validated atom key fits usize")] = position;
        }
    }
    let link_bounds = store
        .links
        .iter()
        .map(|link| {
            let atom = |part: &LinkLabelPart| match part {
                LinkLabelPart::Content { content } => content.atom,
                LinkLabelPart::HardBreak { atom } => *atom,
            };
            link.label
                .first()
                .zip(link.label.last())
                .map(|(first, last)| (atom(first), atom(last)))
        })
        .collect::<Vec<_>>();
    for link in &store.links {
        if store.owner(link.owner).is_none() {
            return invalid("link occurrence references an unknown owner");
        }
        let mut previous = None;
        let mut previous_by_root = HashMap::new();
        for part in &link.label {
            let atom = match part {
                LinkLabelPart::Content { content } => {
                    let Some(atom) = store.atom(content.atom) else {
                        return invalid("link label references an unknown content atom");
                    };
                    let Some(text) = atom.kind.text() else {
                        return invalid("link content part must reference text or whitespace");
                    };
                    if content.bytes.start != 0
                        || content.bytes.end as usize != text.len()
                        || store.text(*content).is_none()
                    {
                        return invalid("link label contains an invalid content range");
                    }
                    content.atom
                }
                LinkLabelPart::HardBreak { atom } => {
                    let Some(record) = store.atom(*atom) else {
                        return invalid("link label references an unknown hard break");
                    };
                    if !matches!(record.kind, ContentAtomKind::HardBreak {}) {
                        return invalid("link hard-break part does not reference a hard break");
                    }
                    *atom
                }
            };
            let record = store.atom(atom).expect("label atom was resolved");
            if record.link != Some(link.key) {
                return invalid("link label atom has inconsistent occurrence");
            }
            let count = &mut linked_parts[atom.index().expect("validated key fits usize")];
            *count = count.saturating_add(1);
            if let Some(previous_key) = previous
                && previous_key >= atom
            {
                return invalid("link label atoms must follow retained execution order");
            }
            let current_index =
                root_positions[atom.index().expect("validated atom key fits usize")];
            if let Some((previous_index, previous_key)) =
                previous_by_root.insert(record.root, (current_index, atom))
            {
                let root = store.root(record.root).expect("label root resolved");
                if previous_index >= current_index
                    || root.atoms[previous_index + 1..current_index]
                        .iter()
                        .any(|key| {
                            let Some(gap) = store.atom(*key) else {
                                return true;
                            };
                            if matches!(gap.kind, ContentAtomKind::BreakOpportunity {}) {
                                return false;
                            }
                            let Some(nested) = gap.link else {
                                return true;
                            };
                            let Some((first, last)) = nested
                                .index()
                                .and_then(|index| link_bounds.get(index))
                                .and_then(|bounds| *bounds)
                            else {
                                return true;
                            };
                            nested == link.key || first <= previous_key || last >= atom
                        })
                {
                    return invalid(
                        "same-root link label parts may be separated only by nested links or break opportunities",
                    );
                }
            }
            previous = Some(atom);
        }
    }
    if store.atoms.iter().any(|atom| {
        let count = linked_parts[atom.key.index().expect("validated key fits usize")];
        (atom.link.is_some() && count != 1) || (atom.link.is_none() && count != 0)
    }) {
        return invalid("linked atoms and occurrence label parts must correspond exactly once");
    }
    Ok(())
}

fn atom_scalar_len(kind: &ContentAtomKind) -> Option<u32> {
    match kind {
        ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
            u32::try_from(text.chars().count()).ok()
        }
        ContentAtomKind::HardBreak {} => Some(1),
        ContentAtomKind::BreakOpportunity {} => Some(0),
    }
}

pub(super) fn validate_projection_limits(store: &ContentStore) -> Result<(), ContentStoreError> {
    const MAX_OBJECTS: usize = 1_000_000;
    const MAX_EDGES: usize = 8_000_000;
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    const MAX_STEPS: usize = 16_000_000;

    let objects = store
        .owners
        .len()
        .saturating_add(store.roots.len())
        .saturating_add(store.atoms.len())
        .saturating_add(store.points.len())
        .saturating_add(store.links.len());
    if objects > MAX_OBJECTS {
        return invalid("content projection exceeds the object limit");
    }
    let edges = store
        .owners
        .iter()
        .map(|owner| owner.roots.len())
        .chain(
            store
                .roots
                .iter()
                .map(|root| root.atoms.len().saturating_add(root.points.len())),
        )
        .chain(store.links.iter().map(|link| link.label.len()))
        .fold(0_usize, usize::saturating_add);
    if edges > MAX_EDGES {
        return invalid("content projection exceeds the edge limit");
    }
    if objects.saturating_add(edges).saturating_mul(2) > MAX_STEPS {
        return invalid("content projection exceeds the traversal/remap step limit");
    }
    let bytes = store
        .atoms
        .iter()
        .filter_map(|atom| atom.kind.text())
        .map(str::len)
        .chain(
            store
                .atoms
                .iter()
                .filter_map(|atom| match &atom.kind {
                    ContentAtomKind::Text {
                        display_override, ..
                    }
                    | ContentAtomKind::Whitespace {
                        display_override, ..
                    } => display_override.as_deref(),
                    ContentAtomKind::BreakOpportunity {} | ContentAtomKind::HardBreak {} => None,
                })
                .map(str::len),
        )
        .chain(store.links.iter().map(|link| target_bytes(&link.target)))
        .chain(
            store
                .links
                .iter()
                .filter_map(|link| link.title.as_deref())
                .map(str::len),
        )
        .fold(0_usize, usize::saturating_add);
    if bytes > MAX_BYTES {
        return invalid("content projection exceeds the retained-byte limit");
    }
    Ok(())
}

fn target_bytes(target: &LinkTarget) -> usize {
    match target {
        LinkTarget::External { uri } => uri.len(),
        LinkTarget::Email { address } => address.len(),
        LinkTarget::Document { name, fragment } => name
            .len()
            .saturating_add(fragment.as_deref().map_or(0, str::len)),
        LinkTarget::Manual {
            name,
            manual_section,
        } => name
            .len()
            .saturating_add(manual_section.as_deref().map_or(0, str::len)),
        LinkTarget::Section { id } => id.as_str().len(),
    }
}

fn validate_dense<T>(
    records: &[T],
    key: impl Fn(&T) -> u32,
    label: &str,
) -> Result<(), ContentStoreError> {
    for (index, record) in records.iter().enumerate() {
        let expected = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1));
        if expected != Some(key(record)) {
            return invalid(format!("content {label} keys must be dense and one-based"));
        }
    }
    Ok(())
}

fn invalid<T>(detail: impl Into<String>) -> Result<T, ContentStoreError> {
    Err(ContentStoreError {
        detail: detail.into(),
    })
}
