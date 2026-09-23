use std::{error::Error, fmt};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::LinkTarget;

use super::fixed::placement_display;
use super::scalar::ScalarBoundaryIndex;
use super::{
    CellMapKind, ContentAtomKey, ContentAtomKind, ContentRootKey, ContentStore, LinkLabelPart,
    LinkOccurrenceKey, PlacementTarget, PointBoundary,
};

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
    // A bounded checkpoint lookup also handles points and physical slices
    // whose atom byte offsets are not presented in monotone order.
    let scalar_index = ScalarBoundaryIndex::build(store).ok_or_else(|| ContentStoreError {
        detail: "fixed scalar index exceeds available resources".to_owned(),
    })?;
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
                let counted = scalar_index
                    .prefix(index, text, byte_offset as usize)
                    .ok_or_else(|| ContentStoreError {
                        detail: "content point scalar boundary exceeds u32".to_owned(),
                    })?;
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
    for link in &store.links {
        if store.owner(link.owner).is_none() {
            return invalid("link occurrence references an unknown owner");
        }
        let mut previous = None;
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
            previous = Some(atom);
        }
    }
    if store.atoms.iter().any(|atom| {
        let count = linked_parts[atom.key.index().expect("validated key fits usize")];
        (atom.link.is_some() && count != 1) || (atom.link.is_none() && count != 0)
    }) {
        return invalid("linked atoms and occurrence label parts must correspond exactly once");
    }
    validate_link_nesting(store)?;
    validate_fixed_views(store, &atom_scalar_prefix, &scalar_index)
}

#[allow(clippy::too_many_lines)] // One pass owns dense geometry, bounded scalar work, and widths.
fn validate_fixed_views(
    store: &ContentStore,
    atom_scalar_prefix: &[u32],
    scalar_index: &ScalarBoundaryIndex,
) -> Result<(), ContentStoreError> {
    validate_dense(&store.fixed_views, |view| view.key.get(), "fixed view")?;
    let (mut next_line, mut next_placement, mut next_decoration) = (0_usize, 0_usize, 0_usize);
    let mut total_columns = 0_usize;
    for view in &store.fixed_views {
        if store.owner(view.owner).is_none() {
            return invalid("fixed view references an unknown owner");
        }
        for line in &view.lines {
            if line.terminal_columns > 1_048_576 {
                return invalid("fixed line exceeds the terminal-column limit");
            }
            total_columns = total_columns.saturating_add(line.terminal_columns as usize);
            if total_columns > 32 * 1024 * 1024 {
                return invalid("fixed views exceed the total terminal-column limit");
            }
            if line.key.index() != Some(next_line) {
                return invalid("fixed line keys must be dense in view order");
            }
            next_line = next_line.checked_add(1).ok_or_else(|| ContentStoreError {
                detail: "fixed line key space is exhausted".to_owned(),
            })?;
            let mut previous_start = 0_u32;
            let mut previous_end = 0_u32;
            let mut occupied: Vec<(u32, u32)> =
                Vec::with_capacity(line.placements.len() + line.decorations.len());
            for placement in &line.placements {
                if placement.key.index() != Some(next_placement) {
                    return invalid("fixed placement keys must be dense in line order");
                }
                next_placement =
                    next_placement
                        .checked_add(1)
                        .ok_or_else(|| ContentStoreError {
                            detail: "fixed placement key space is exhausted".to_owned(),
                        })?;
                if placement.start_column > placement.end_column
                    || placement.end_column > line.terminal_columns
                    || placement.root_scalar_range.start > placement.root_scalar_range.end
                {
                    return invalid("fixed placement lies outside its physical line");
                }
                let overlay = matches!(placement.map, CellMapKind::Overlay {});
                if !matches!(placement.target, PlacementTarget::Point(_))
                    && placement.start_column < placement.end_column
                {
                    let same_origin =
                        placement.start_column == previous_start && previous_end > previous_start;
                    if placement.start_column < previous_end && !(same_origin && overlay) {
                        return invalid(
                            "fixed placements overlap without an observed same-origin overstrike",
                        );
                    }
                    if same_origin && placement.start_column < previous_end {
                        if let Some((_, end)) = occupied.last_mut() {
                            *end = (*end).max(placement.end_column);
                        }
                    } else {
                        occupied.push((placement.start_column, placement.end_column));
                    }
                    previous_start = placement.start_column;
                    previous_end = placement.end_column;
                }
                match placement.target {
                    PlacementTarget::Point(key) => {
                        let Some(point) = store.point(key) else {
                            return invalid("fixed placement references an unknown point");
                        };
                        if placement.root_scalar_range.start != point.scalar_boundary
                            || placement.root_scalar_range.end != point.scalar_boundary
                            || placement.start_column != placement.end_column
                        {
                            return invalid(
                                "fixed point placement disagrees with its logical point",
                            );
                        }
                    }
                    PlacementTarget::Content(content) => {
                        let Some(atom) = store.atom(content.atom) else {
                            return invalid("fixed placement references an unknown atom");
                        };
                        let Some(text) = store.text(content) else {
                            return invalid("fixed placement has an invalid UTF-8 content range");
                        };
                        if text.is_empty() {
                            return invalid("fixed content placement has an empty range");
                        }
                        let Some(whole) = atom.kind.text() else {
                            return invalid(
                                "fixed content placement must reference text or whitespace",
                            );
                        };
                        let display = placement_display(&atom.kind, content, text);
                        if display.is_empty()
                            || display.chars().any(|scalar| {
                                scalar.is_control() || matches!(scalar, '\u{2028}' | '\u{2029}')
                            })
                        {
                            return invalid("fixed placement contains non-printing glyphs");
                        }
                        let prefix =
                            atom_scalar_prefix[content.atom.index().expect("valid atom key")];
                        let atom_index = content.atom.index().expect("valid atom key");
                        let Some(start) = scalar_index
                            .prefix(atom_index, whole, content.bytes.start as usize)
                            .and_then(|boundary| prefix.checked_add(boundary))
                        else {
                            return invalid("fixed scalar boundary exceeds u32");
                        };
                        let Some(end) = scalar_index
                            .prefix(atom_index, whole, content.bytes.end as usize)
                            .and_then(|boundary| prefix.checked_add(boundary))
                        else {
                            return invalid("fixed scalar boundary exceeds u32");
                        };
                        if placement.root_scalar_range != (start..end)
                            || (placement.start_column == placement.end_column
                                && !matches!(placement.map, CellMapKind::GraphemeCluster {}))
                        {
                            return invalid(
                                "fixed content placement disagrees with its logical range",
                            );
                        }
                        match placement.map {
                            CellMapKind::Affine { columns_per_scalar } => {
                                let step = usize::from(columns_per_scalar);
                                if columns_per_scalar == 0
                                    || UnicodeSegmentation::graphemes(text, true)
                                        .any(|grapheme| grapheme.chars().count() != 1)
                                    || (display != text && UnicodeWidthStr::width(display) != step)
                                    || (display == text
                                        && text.chars().any(|scalar| {
                                            UnicodeWidthStr::width(scalar.to_string().as_str())
                                                != step
                                        }))
                                    || end
                                        .saturating_sub(start)
                                        .saturating_mul(u32::from(columns_per_scalar))
                                        != placement.end_column - placement.start_column
                                {
                                    return invalid(
                                        "fixed affine placement has inconsistent scalar columns",
                                    );
                                }
                            }
                            CellMapKind::GraphemeCluster {} => {
                                if UnicodeSegmentation::graphemes(text, true).count() != 1
                                    || UnicodeWidthStr::width(display)
                                        != (placement.end_column - placement.start_column) as usize
                                {
                                    return invalid(
                                        "fixed cluster placement has inconsistent glyph columns",
                                    );
                                }
                            }
                            CellMapKind::Overlay {} => {
                                if UnicodeSegmentation::graphemes(text, true).count() != 1
                                    || UnicodeWidthStr::width(display)
                                        != (placement.end_column - placement.start_column) as usize
                                {
                                    return invalid("fixed overlay has inconsistent glyph columns");
                                }
                            }
                        }
                    }
                }
            }
            for decoration in &line.decorations {
                if decoration.key.index() != Some(next_decoration) {
                    return invalid("fixed decoration keys must be dense in line order");
                }
                next_decoration =
                    next_decoration
                        .checked_add(1)
                        .ok_or_else(|| ContentStoreError {
                            detail: "fixed decoration key space is exhausted".to_owned(),
                        })?;
                let Some(end) = decoration
                    .start_column
                    .checked_add(decoration.width_columns)
                else {
                    return invalid("fixed decoration columns overflow");
                };
                if decoration.text.is_empty()
                    || decoration.text.chars().any(char::is_control)
                    || decoration.width_columns == 0
                    || end > line.terminal_columns
                    || UnicodeWidthStr::width(decoration.text.as_str())
                        != decoration.width_columns as usize
                {
                    return invalid("fixed decoration has invalid glyphs or columns");
                }
                occupied.push((decoration.start_column, end));
            }
            occupied.sort_unstable();
            if occupied.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                return invalid("fixed content and decorations occupy the same columns");
            }
        }
    }
    Ok(())
}

fn validate_link_nesting(store: &ContentStore) -> Result<(), ContentStoreError> {
    struct ActiveLink {
        link: LinkOccurrenceKey,
        end: usize,
        previous: ContentAtomKey,
        nested_bounds: Option<(ContentAtomKey, ContentAtomKey)>,
    }

    let mut global_bounds: Vec<Option<(ContentAtomKey, ContentAtomKey)>> =
        vec![None; store.links.len()];
    for atom in &store.atoms {
        if let Some(link) = atom.link {
            let slot = &mut global_bounds[link.index().expect("validated link key")];
            if let Some((_, last)) = slot {
                *last = atom.key;
            } else {
                *slot = Some((atom.key, atom.key));
            }
        }
    }
    let mut bounds: Vec<Option<(ContentRootKey, usize, usize)>> = vec![None; store.links.len()];
    let mut active: Vec<ActiveLink> = Vec::new();
    for root in &store.roots {
        // Compute per-root intervals once. Dense link keys make the scratch
        // table reusable across roots without clearing all links each time.
        for (position, key) in root.atoms.iter().enumerate() {
            if let Some(link) = store.atom(*key).expect("validated root atom").link {
                let slot = &mut bounds[link.index().expect("validated link key")];
                if let Some((seen_root, _, last)) = slot
                    && *seen_root == root.key
                {
                    *last = position;
                } else {
                    *slot = Some((root.key, position, position));
                }
            }
        }
        active.clear();
        for (position, key) in root.atoms.iter().enumerate() {
            let atom = store.atom(*key).expect("validated root atom");
            if matches!(atom.kind, ContentAtomKind::BreakOpportunity {}) {
                continue;
            }
            match atom.link {
                Some(link) if active.last().is_some_and(|frame| frame.link == link) => {
                    let frame = active.last_mut().expect("matching frame exists");
                    if let Some((first, last)) = frame.nested_bounds.take()
                        && (first <= frame.previous || last >= atom.key)
                    {
                        return invalid("nested link bounds cross the enclosing label gap");
                    }
                    frame.previous = atom.key;
                }
                Some(link) => {
                    let (_, first, last) = bounds[link.index().expect("validated link key")]
                        .expect("link atom has interval");
                    if first != position || active.last().is_some_and(|frame| last >= frame.end) {
                        return invalid("same-root link intervals must be properly nested");
                    }
                    active.push(ActiveLink {
                        link,
                        end: last,
                        previous: atom.key,
                        nested_bounds: None,
                    });
                }
                None if !active.is_empty() => {
                    return invalid(
                        "link occurrence is interrupted by unrelated visible content within one root",
                    );
                }
                None => {}
            }
            if let Some(finished) = active.pop_if(|frame| frame.end == position)
                && let Some(parent) = active.last_mut()
            {
                let (first, last) = global_bounds[finished.link.index().unwrap()]
                    .expect("active link has global bounds");
                parent.nested_bounds = Some(
                    parent
                        .nested_bounds
                        .map_or((first, last), |(minimum, maximum)| {
                            (minimum.min(first), maximum.max(last))
                        }),
                );
            }
        }
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
