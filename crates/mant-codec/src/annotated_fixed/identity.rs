//! Native mark identities, aliases and dense typed-key resolution.

use std::collections::{HashMap, HashSet};

use super::{
    AnnotatedDocument, AnnotatedProjectionError, FragmentAlias, NodeId, NonZeroU32, Result, key,
};

pub(super) struct KeyMap {
    heading: Vec<u32>,
    owner: Vec<u32>,
    link: Vec<u32>,
    anchor: Vec<u32>,
    region: Vec<u32>,
    // Inclusive nearest typed key for heading, owner, and region. Native marks
    // are preorder, so each row is derived once from its earlier parent.
    nearest: Vec<[u32; 3]>,
}

pub(super) struct Identities {
    heading: Vec<Option<NodeId>>,
    owner: Vec<Option<NodeId>>,
    anchor: Vec<Option<NodeId>>,
    heading_aliases: Vec<Vec<FragmentAlias>>,
    heading_generated_aliases: Vec<Vec<FragmentAlias>>,
    heading_rendered_aliases: Vec<Vec<FragmentAlias>>,
    anchor_rendered: Vec<Option<FragmentAlias>>,
    consumed_anchor: Vec<bool>,
    section_targets: crate::mandoc::navigation::SectionTargets,
}

impl Identities {
    #[allow(clippy::too_many_lines)] // One collision domain for heading, anchor and owner IDs.
    pub(super) fn new(page: &AnnotatedDocument) -> Result<Self> {
        let mut used = HashSet::new();
        let mut next_suffix = HashMap::new();
        used.insert(mant_ir::DOCUMENT_ROOT_ID.to_owned());
        let length = page
            .marks
            .len()
            .checked_add(1)
            .ok_or(AnnotatedProjectionError::Relation("mark count overflow"))?;
        // This owned input is public and may not have crossed the native FFI
        // checker. Validate the global preorder keys before using any key as
        // a vector index or as the carrier of a moved tag.c target.
        for (index, mark) in page.marks.iter().enumerate() {
            if mark.key as usize != index + 1
                || !(1..=6).contains(&mark.kind)
                || (mark.parent != 0 && mark.parent >= mark.key)
            {
                return Err(AnnotatedProjectionError::Relation(
                    "invalid global native mark key or parent",
                ));
            }
        }
        let mut heading = vec![None; length];
        let mut owner = vec![None; length];
        let mut anchor = vec![None; length];
        let mut heading_aliases = vec![Vec::new(); length];
        let mut heading_generated_aliases = vec![Vec::new(); length];
        let mut heading_rendered_aliases = vec![Vec::new(); length];
        let mut anchor_rendered = vec![None; length];
        let mut consumed_anchor = vec![false; length];
        let mut section_targets = crate::mandoc::navigation::SectionTargets::new();

        // A surviving tag on a heading HEAD is the heading's own target,
        // not an independent anchor.  The native parent points at that exact
        // HEAD region; no display-neighbor or prior-root guess is involved.
        let mut carrier = vec![0_u32; length];
        for mark in &page.marks {
            if mark.kind != 1 {
                continue;
            }
            if mark.title_region != 0 {
                *carrier.get_mut(mark.title_region as usize).ok_or(
                    AnnotatedProjectionError::Relation("heading title region outside mark table"),
                )? = mark.key;
            }
        }
        // Native HTML emits one unique ID for each surviving NODE_ID, in
        // preorder. Keep the original declaration separately from that ID.
        // A canonical slug may equal its own emitted target, but never
        // another target's fragment.
        let mut target_claims = HashMap::<String, HashSet<u32>>::new();
        let mut html_ordinals = HashMap::<String, u64>::new();
        for mark in &page.marks {
            if mark.kind != 4 {
                continue;
            }
            let name = mark
                .name
                .as_deref()
                .ok_or(AnnotatedProjectionError::Relation("anchor has no name"))?;
            let rendered = render_html_fragment(name, &mut html_ordinals)?;
            let heading_key = carrier.get(mark.parent as usize).copied().unwrap_or(0);
            let attached =
                heading_key != 0 && page.marks[(heading_key - 1) as usize].token == mark.token;
            if attached {
                consumed_anchor[mark.key as usize] = true;
                if valid_alias(name) {
                    if mark.flags & 4 != 0 {
                        heading_aliases[heading_key as usize].push(FragmentAlias::from(name));
                    } else {
                        heading_generated_aliases[heading_key as usize]
                            .push(FragmentAlias::from(name));
                    }
                    heading_rendered_aliases[heading_key as usize]
                        .push(FragmentAlias::from(rendered.as_str()));
                    target_claims
                        .entry(rendered)
                        .or_default()
                        .insert(heading_key);
                }
            } else if valid_alias(name) {
                anchor_rendered[mark.key as usize] = Some(FragmentAlias::from(rendered.as_str()));
                target_claims.entry(rendered).or_default().insert(mark.key);
            }
        }
        // The authored phrase is captured from the final AST HEAD with the
        // pinned roff.c::deroff rule.  Final display spelling can differ after
        // overprint, deletion, or an unknown text join and is never an ID.
        for mark in &page.marks {
            if mark.kind != 1 {
                continue;
            }
            let base = mark.name.as_deref().map_or_else(
                || "section".to_owned(),
                |phrase| identity_base(phrase, "section", "section"),
            );
            // An authored alias on a *different* heading, or on an independent
            // anchor, owns its exact fragment spelling.  The current
            // heading's own alias may also be its canonical normalized ID.
            let id = allocate_id_excluding(&base, &mut used, &mut next_suffix, |candidate| {
                target_claims
                    .get(candidate)
                    .is_some_and(|owners| owners.iter().any(|owner| *owner != mark.key))
            })?;
            if let Some(phrase) = &mark.name {
                section_targets
                    .entry(phrase.clone())
                    .and_modify(|target| *target = None)
                    .or_insert_with(|| Some(id.clone()));
            }
            *heading
                .get_mut(mark.key as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "heading identity key outside mark table",
                ))? = Some(NodeId::new(id));
        }
        for mark in &page.marks {
            if mark.kind != 4 || consumed_anchor[mark.key as usize] {
                continue;
            }
            let name = mark
                .name
                .as_deref()
                .ok_or(AnnotatedProjectionError::Relation("anchor has no name"))?;
            let base = identity_base(name, "anchor", "anchor");
            let id = allocate_id_excluding(&base, &mut used, &mut next_suffix, |candidate| {
                target_claims
                    .get(candidate)
                    .is_some_and(|owners| owners.iter().any(|owner| *owner != mark.key))
            })?;
            anchor[mark.key as usize] = Some(NodeId::new(id));
        }
        for mark in &page.marks {
            if mark.kind != 2 {
                continue;
            }
            let base = format!("native-owner-{}", mark.key);
            let id = allocate_id_excluding(&base, &mut used, &mut next_suffix, |candidate| {
                target_claims.contains_key(candidate)
            })?;
            owner[mark.key as usize] = Some(NodeId::new(id));
        }
        Ok(Self {
            heading,
            owner,
            anchor,
            heading_aliases,
            heading_generated_aliases,
            heading_rendered_aliases,
            anchor_rendered,
            consumed_anchor,
            section_targets,
        })
    }

    pub(super) fn heading(&self, global: u32) -> Result<NodeId> {
        self.heading
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "heading identity missing",
            ))
    }

    pub(super) fn owner(&self, global: u32) -> Result<NodeId> {
        self.owner
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation("owner identity missing"))
    }

    pub(super) fn anchor(&self, global: u32) -> Result<NodeId> {
        self.anchor
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "anchor identity missing",
            ))
    }

    pub(super) fn heading_aliases(&self, global: u32) -> Result<Vec<FragmentAlias>> {
        self.heading_aliases.get(global as usize).cloned().ok_or(
            AnnotatedProjectionError::Relation("heading alias key missing"),
        )
    }

    pub(super) fn heading_generated_aliases(&self, global: u32) -> Result<Vec<FragmentAlias>> {
        self.heading_generated_aliases
            .get(global as usize)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "heading generated alias key missing",
            ))
    }

    pub(super) fn heading_rendered_aliases(&self, global: u32) -> Result<Vec<FragmentAlias>> {
        self.heading_rendered_aliases
            .get(global as usize)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "heading rendered alias key missing",
            ))
    }

    pub(super) fn anchor_rendered(&self, global: u32) -> Result<FragmentAlias> {
        self.anchor_rendered
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "anchor rendered fragment missing",
            ))
    }

    pub(super) fn consumed_anchor(&self, global: u32) -> Result<bool> {
        self.consumed_anchor
            .get(global as usize)
            .copied()
            .ok_or(AnnotatedProjectionError::Relation("anchor key missing"))
    }

    pub(super) fn section(&self, phrase: &str) -> Option<NodeId> {
        crate::mandoc::navigation::resolve_section_target(&self.section_targets, phrase)
            .map(NodeId::new)
    }
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

// Pinned CVS html.c::html_make_id: sanitize each byte, then reserve '~' for
// the ordinal suffix. The annotated marks are emitted in native AST preorder.
fn render_html_fragment(name: &str, ordinals: &mut HashMap<String, u64>) -> Result<String> {
    let mut base = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            28 => base.push('-'), // mandoc.h::ASCII_HYPH
            b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => base.push(char::from(byte)),
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b'-' | b'.' | b'/'
            | b':' | b';' | b'=' | b'?' | b'@' | b'_' => {
                base.push(char::from(byte));
            }
            _ => base.push('_'),
        }
    }
    let ordinal = ordinals.entry(base.clone()).or_insert(0);
    *ordinal = ordinal
        .checked_add(1)
        .ok_or(AnnotatedProjectionError::Relation(
            "HTML ID ordinal overflow",
        ))?;
    Ok(if *ordinal == 1 {
        base
    } else {
        format!("{base}~{ordinal}")
    })
}

fn identity_base(value: &str, fallback: &str, reserved_suffix: &str) -> String {
    // Same grammar and reserved-selector rule as the existing native Flow
    // address planner; Fixed uses its final title selection as the phrase.
    let has_identity_character =
        value.chars().any(char::is_alphanumeric) || value.trim_start_matches(['-', '/']) == "?";
    let base = if has_identity_character {
        crate::definitions::document_id_slug(value)
    } else {
        fallback.to_owned()
    };
    if crate::producer_identity::is_reserved_selector(&base) {
        format!("{base}-{reserved_suffix}")
    } else {
        base
    }
}

fn allocate_id_excluding(
    base: &str,
    used: &mut HashSet<String>,
    next_suffix: &mut HashMap<String, u64>,
    mut excluded: impl FnMut(&str) -> bool,
) -> Result<String> {
    let mut suffix = next_suffix.get(base).copied().unwrap_or(1);
    loop {
        let candidate = if suffix == 1 {
            base.to_owned()
        } else {
            format!("{base}-{suffix}")
        };
        if !excluded(&candidate) && used.insert(candidate.clone()) {
            next_suffix.insert(
                base.to_owned(),
                suffix
                    .checked_add(1)
                    .ok_or(AnnotatedProjectionError::Relation(
                        "identity suffix overflow",
                    ))?,
            );
            return Ok(candidate);
        }
        suffix = suffix
            .checked_add(1)
            .ok_or(AnnotatedProjectionError::Relation(
                "identity suffix overflow",
            ))?;
    }
}

impl KeyMap {
    #[allow(clippy::too_many_lines)] // One preorder pass closes all typed native keys.
    pub(super) fn new(page: &AnnotatedDocument, identities: &Identities) -> Result<Self> {
        let length = page
            .marks
            .len()
            .checked_add(1)
            .ok_or(AnnotatedProjectionError::Relation("mark count overflow"))?;
        let mut map = Self {
            heading: vec![0; length],
            owner: vec![0; length],
            link: vec![0; length],
            anchor: vec![0; length],
            region: vec![0; length],
            nearest: vec![[0; 3]; length],
        };
        let mut counts = [0_u32; 5];
        for (index, mark) in page.marks.iter().enumerate() {
            if mark.key as usize != index + 1 || !(1..=6).contains(&mark.kind) {
                return Err(AnnotatedProjectionError::Relation(
                    "invalid global native mark key",
                ));
            }
            // Public owned results can be constructed without crossing the
            // FFI checker. Keep role evidence closed at this boundary too.
            let allowed_flags = match mark.kind {
                1 => 0b1001,         // authored heading and subsection
                2 => 0b11_1111_0001, // authored owner, candidate, definition and one head role
                4 => 0b0101,         // authored anchor and manual target
                3 | 5 => 0b0001,     // authored link or region
                6 => 0b1_1110_0001,  // authored HEAD component and one native role
                _ => unreachable!(),
            };
            if mark.flags & !allowed_flags != 0 {
                return Err(AnnotatedProjectionError::Relation(
                    "native mark has invalid kind flags",
                ));
            }
            if (mark.line == 0) != (mark.column == 0)
                || (mark.source == 0 && mark.line != 0)
                || ((mark.flags & 1 != 0) != (mark.line != 0))
            {
                return Err(AnnotatedProjectionError::Relation(
                    "native mark has inconsistent authored coordinates",
                ));
            }
            if mark.kind == 2
                && mark.flags & 0b1_1110_0000 != 0
                && (mark.flags & 16 == 0 || (mark.flags & 0b1_1110_0000).count_ones() != 1)
            {
                return Err(AnnotatedProjectionError::Relation(
                    "native owner has invalid head role flags",
                ));
            }
            if mark.kind == 6
                && (mark.source == 0
                    || (mark.flags & 0b1_1110_0000).count_ones() != 1
                    || mark.parent == 0
                    || mark.owner != mark.parent
                    || page.marks[(mark.parent - 1) as usize].kind != 5
                    || page.marks[(mark.parent - 1) as usize].region_kind != 3)
            {
                return Err(AnnotatedProjectionError::Relation(
                    "native head component has invalid parent or role",
                ));
            }
            if mark.name.as_deref().is_some_and(str::is_empty)
                || mark.name.is_some()
                    && !matches!(mark.kind, 1 | 4)
                    && !(mark.kind == 2 && mark.flags & (32 | 64 | 256) != 0)
            {
                return Err(AnnotatedProjectionError::Relation(
                    "native mark has an invalid authored operand",
                ));
            }
            // Native marks form a preorder forest. Public owned results can
            // also be constructed by callers, so reject cycles before any
            // ancestry lookup instead of relying on the FFI validator.
            if mark.parent >= mark.key && mark.parent != 0 {
                return Err(AnnotatedProjectionError::Relation(
                    "native mark parent does not precede child",
                ));
            }
            if mark.kind == 4 && identities.consumed_anchor(mark.key)? {
                // tag.c::tag_move_id can make a manual target the heading's
                // authored alias. The original global mark stays in native
                // identity order, but it is not a separate Fixed anchor.
                map.nearest[mark.key as usize] = map.nearest[mark.parent as usize];
                continue;
            }
            if mark.kind == 6 {
                map.nearest[mark.key as usize] = map.nearest[mark.parent as usize];
                continue;
            }
            let slot = (mark.kind - 1) as usize;
            counts[slot] =
                counts[slot]
                    .checked_add(1)
                    .ok_or(AnnotatedProjectionError::Relation(
                        "typed mark key overflow",
                    ))?;
            let target = match mark.kind {
                1 => &mut map.heading,
                2 => &mut map.owner,
                3 => &mut map.link,
                4 => &mut map.anchor,
                5 => &mut map.region,
                _ => unreachable!(),
            };
            *target
                .get_mut(mark.key as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "native mark key outside mark table",
                ))? = counts[slot];
            let parent = map.nearest[mark.parent as usize];
            let mut nearest = parent;
            match mark.kind {
                1 => nearest[0] = counts[0],
                2 => nearest[1] = counts[1],
                5 => nearest[2] = counts[4],
                _ => {}
            }
            map.nearest[mark.key as usize] = nearest;
        }
        Ok(map)
    }

    pub(super) fn lookup(&self, global: u32, kind: u32) -> Result<Option<NonZeroU32>> {
        if global == 0 {
            return Ok(None);
        }
        let domain = match kind {
            1 => &self.heading,
            2 => &self.owner,
            3 => &self.link,
            4 => &self.anchor,
            5 => &self.region,
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown typed key domain",
                ));
            }
        };
        let value = domain.get(global as usize).copied().unwrap_or(0);
        Ok(Some(key(value, "global mark has wrong typed key domain")?))
    }

    pub(super) fn required(&self, global: u32, kind: u32) -> Result<NonZeroU32> {
        self.lookup(global, kind)?
            .ok_or(AnnotatedProjectionError::Relation("missing typed mark key"))
    }

    pub(super) fn nearest(&self, global: u32, kind: u32) -> Result<Option<NonZeroU32>> {
        let slot = match kind {
            1 => 0,
            2 => 1,
            5 => 2,
            _ => return Err(AnnotatedProjectionError::Relation("unknown ancestry kind")),
        };
        let value = self
            .nearest
            .get(global as usize)
            .ok_or(AnnotatedProjectionError::Relation(
                "mark ancestry is broken",
            ))?[slot];
        Ok(NonZeroU32::new(value))
    }
}
