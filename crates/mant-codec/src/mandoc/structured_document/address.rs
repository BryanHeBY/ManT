//! Native address evidence is resolved before structural content is lowered.
//!
//! The native result owns authored heading phrases, zero-width points, target
//! provenance, and link occurrence identity.  Keeping those facts in one plan
//! prevents block lowering from reconstructing destinations from display text
//! or from the legacy item target fields.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use libmandoc_rs::structured::{
    ContentPoint, ContentPointKey, NativeBlock, NativeBlockKey, NativeLinkTarget,
    NativeTargetOrigin, PointBoundary,
};
use mant_ir::{
    Diagnostic, DiagnosticImpact, DiagnosticLevel, FragmentAlias, Inline, LinkTarget, NodeId,
    SourceSpan,
};

use super::{NativeProjectionError, NativeProseProjection, content::source_for};

#[derive(Debug)]
pub(super) struct SectionAddress {
    id: NodeId,
    aliases: Vec<FragmentAlias>,
}

impl SectionAddress {
    pub(super) fn id(&self) -> &NodeId {
        &self.id
    }

    pub(super) fn aliases(&self) -> &[FragmentAlias] {
        &self.aliases
    }
}

#[derive(Debug)]
pub(super) struct AnchorPlacement {
    id: NodeId,
    aliases: Vec<FragmentAlias>,
    owner_source: Option<SourceSpan>,
}

impl AnchorPlacement {
    pub(super) fn inline(&self) -> Inline {
        Inline::Anchor {
            id: self.id.clone(),
            fragment_aliases: self.aliases.clone(),
            owner_source: self.owner_source,
        }
    }
}

#[derive(Debug)]
struct AnchorGroup<'a> {
    point: &'a ContentPoint,
    authored_spelling: Option<&'a str>,
    authored: Vec<&'a str>,
    generated: Vec<&'a str>,
}

type AnchorGroups<'a> = BTreeMap<ContentPointKey, AnchorGroup<'a>>;
type AliasClaims = HashMap<String, BTreeSet<ContentPointKey>>;

struct SectionAssignments {
    sections: HashMap<NativeBlockKey, SectionAddress>,
    targets: super::super::navigation::SectionTargets,
    consumed_points: HashSet<ContentPointKey>,
}

struct AnchorAssignments {
    anchors: HashMap<ContentPointKey, AnchorPlacement>,
    by_owner: HashMap<libmandoc_rs::structured::OwnerKey, Vec<ContentPointKey>>,
}

/// One immutable address plan for a native result.
#[derive(Debug)]
pub(super) struct AddressPlan {
    sections: HashMap<NativeBlockKey, SectionAddress>,
    anchors: HashMap<ContentPointKey, AnchorPlacement>,
    anchors_by_owner: HashMap<libmandoc_rs::structured::OwnerKey, Vec<ContentPointKey>>,
    links: Vec<Option<LinkTarget>>,
    reserved: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
}

impl AddressPlan {
    pub(super) fn build(projection: &NativeProseProjection) -> Result<Self, NativeProjectionError> {
        let native = projection.document();
        let mut diagnostics = Vec::new();
        let (groups, claims) = collect_anchor_groups(projection, &mut diagnostics)?;
        let headings = heading_rows(native)?;
        let mut used = HashSet::new();
        let sections = build_sections(native, &headings, &groups, &claims, &mut used)?;
        let anchors = build_anchors(
            projection,
            groups,
            &sections.consumed_points,
            &claims,
            &mut used,
        );
        let links = build_links(projection, &sections.targets, &mut diagnostics)?;
        let mut reserved = claims.into_keys().collect::<HashSet<_>>();
        reserved.extend(
            sections
                .sections
                .values()
                .map(|section| section.id.to_string()),
        );
        reserved.extend(anchors.anchors.values().map(|anchor| anchor.id.to_string()));
        Ok(Self {
            sections: sections.sections,
            anchors: anchors.anchors,
            anchors_by_owner: anchors.by_owner,
            links,
            reserved,
            diagnostics,
        })
    }

    pub(super) fn section(
        &self,
        block: NativeBlockKey,
    ) -> Result<&SectionAddress, NativeProjectionError> {
        self.sections
            .get(&block)
            .ok_or(NativeProjectionError::InvalidRelation(
                "heading address assignment is missing",
            ))
    }

    pub(super) fn anchor(&self, point: ContentPointKey) -> Option<&AnchorPlacement> {
        self.anchors.get(&point)
    }

    pub(super) fn owner_anchors(&self, owner: libmandoc_rs::structured::OwnerKey) -> Vec<Inline> {
        self.anchors_by_owner
            .get(&owner)
            .into_iter()
            .flatten()
            .filter_map(|point| self.anchors.get(point))
            .map(AnchorPlacement::inline)
            .collect()
    }

    pub(super) fn link_target(
        &self,
        key: libmandoc_rs::structured::LinkOccurrenceKey,
    ) -> Result<Option<&LinkTarget>, NativeProjectionError> {
        self.links
            .get(key.get() as usize - 1)
            .map(Option::as_ref)
            .ok_or(NativeProjectionError::InvalidRelation(
                "content atom references an unknown link assignment",
            ))
    }

    pub(super) fn reserved(&self) -> &HashSet<String> {
        &self.reserved
    }

    pub(super) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

fn collect_anchor_groups<'a>(
    projection: &'a NativeProseProjection,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(AnchorGroups<'a>, AliasClaims), NativeProjectionError> {
    let native = projection.document();
    let mut groups = AnchorGroups::new();
    let mut claims = AliasClaims::new();
    for anchor in native.anchors() {
        let point =
            native
                .content_point(anchor.point())
                .ok_or(NativeProjectionError::InvalidRelation(
                    "anchor point is missing during lowering",
                ))?;
        let group = groups.entry(point.key()).or_insert_with(|| AnchorGroup {
            point,
            authored_spelling: None,
            authored: Vec::new(),
            generated: Vec::new(),
        });
        if group.point.owner() != anchor.owner() {
            return Err(NativeProjectionError::InvalidRelation(
                "anchor group crosses native owners",
            ));
        }
        match anchor.origin() {
            NativeTargetOrigin::Authored => {
                group.authored_spelling.get_or_insert(anchor.target());
                if valid_alias(anchor.target()) {
                    if !group.authored.contains(&anchor.target()) {
                        group.authored.push(anchor.target());
                    }
                    claims
                        .entry(anchor.target().to_owned())
                        .or_default()
                        .insert(point.key());
                } else {
                    diagnostics.push(invalid_alias_diagnostic(
                        projection,
                        anchor.target(),
                        anchor.provenance(),
                    ));
                }
            }
            NativeTargetOrigin::Generated => {
                if !group.generated.contains(&anchor.target()) {
                    group.generated.push(anchor.target());
                }
            }
        }
    }
    Ok((groups, claims))
}

fn invalid_alias_diagnostic(
    projection: &NativeProseProjection,
    target: &str,
    provenance: libmandoc_rs::structured::ProvenanceKey,
) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        impact: DiagnosticImpact::None,
        code: Some("ir.invalid-fragment-alias".to_owned()),
        message: format!(
            "source-authored fragment alias '{target}' contains whitespace or control characters"
        ),
        source: source_for(projection, provenance),
    }
}

fn build_sections(
    native: &libmandoc_rs::structured::StructuredDocument,
    headings: &HashMap<NativeBlockKey, &libmandoc_rs::structured::HeadingEvidence>,
    groups: &AnchorGroups<'_>,
    claims: &AliasClaims,
    used: &mut HashSet<String>,
) -> Result<SectionAssignments, NativeProjectionError> {
    let mut sections = HashMap::new();
    let mut targets = super::super::navigation::SectionTargets::new();
    let mut consumed_points = HashSet::new();
    for block in native
        .blocks()
        .iter()
        .filter(|block| block.kind() == libmandoc_rs::structured::NativeBlockKind::Heading)
    {
        let heading =
            headings
                .get(&block.key())
                .copied()
                .ok_or(NativeProjectionError::InvalidRelation(
                    "heading block has no heading evidence",
                ))?;
        let leading_points = leading_anchor_points(native, block, groups)?;
        let base = heading.authored_phrase().map_or_else(
            || "section".to_owned(),
            |phrase| identity_base(phrase, "section", "section"),
        );
        let id = allocate_id(&base, used, claims, &leading_points);
        let aliases = section_aliases(groups, &leading_points);
        consumed_points.extend(&leading_points);
        if let Some(phrase) = heading.authored_phrase() {
            targets
                .entry(phrase.to_owned())
                .and_modify(|target| *target = None)
                .or_insert_with(|| Some(id.clone()));
        }
        sections.insert(
            block.key(),
            SectionAddress {
                id: NodeId::new(id),
                aliases,
            },
        );
    }
    Ok(SectionAssignments {
        sections,
        targets,
        consumed_points,
    })
}

fn section_aliases(
    groups: &AnchorGroups<'_>,
    leading_points: &BTreeSet<ContentPointKey>,
) -> Vec<FragmentAlias> {
    let mut aliases = Vec::new();
    for point in leading_points {
        let group = groups.get(point).expect("leading point came from groups");
        for alias in &group.authored {
            if !aliases
                .iter()
                .any(|known: &FragmentAlias| known.as_str() == *alias)
            {
                aliases.push(FragmentAlias::from(*alias));
            }
        }
    }
    aliases
}

fn build_anchors(
    projection: &NativeProseProjection,
    groups: AnchorGroups<'_>,
    consumed_points: &HashSet<ContentPointKey>,
    claims: &AliasClaims,
    used: &mut HashSet<String>,
) -> AnchorAssignments {
    let native = projection.document();
    let mut anchors = HashMap::new();
    let mut by_owner = HashMap::<_, Vec<_>>::new();
    for (point_key, group) in groups {
        if consumed_points.contains(&point_key) {
            continue;
        }
        let spelling = anchor_identity_spelling(group.authored_spelling, &group.generated);
        let base = spelling.map_or_else(
            || "anchor".to_owned(),
            |value| identity_base(value, "anchor", "anchor"),
        );
        let id = allocate_id(&base, used, claims, &BTreeSet::from([point_key]));
        let owner_source = native
            .owner(group.point.owner())
            .and_then(|owner| source_for(projection, owner.provenance()));
        anchors.insert(
            point_key,
            AnchorPlacement {
                id: NodeId::new(id),
                aliases: group
                    .authored
                    .into_iter()
                    .map(FragmentAlias::from)
                    .collect(),
                owner_source,
            },
        );
        by_owner
            .entry(group.point.owner())
            .or_default()
            .push(point_key);
    }
    AnchorAssignments { anchors, by_owner }
}

fn build_links(
    projection: &NativeProseProjection,
    section_targets: &super::super::navigation::SectionTargets,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Option<LinkTarget>>, NativeProjectionError> {
    let native = projection.document();
    let mut links = Vec::new();
    links
        .try_reserve_exact(native.links().len())
        .map_err(|_| NativeProjectionError::InvalidRelation("link address allocation"))?;
    for link in native.links() {
        links.push(match link.target() {
            NativeLinkTarget::Section(phrase) => {
                if let Some(id) =
                    super::super::navigation::resolve_section_target(section_targets, phrase)
                {
                    Some(LinkTarget::Section { id: id.into() })
                } else {
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        impact: DiagnosticImpact::None,
                        code: Some("unresolved-section-reference".to_owned()),
                        message: format!("cannot resolve section reference: {phrase}"),
                        source: source_for(projection, link.provenance()),
                    });
                    None
                }
            }
            NativeLinkTarget::External(uri) => Some(LinkTarget::External { uri: uri.clone() }),
            NativeLinkTarget::Email(address) => Some(LinkTarget::Email {
                address: address.clone(),
            }),
            NativeLinkTarget::Document(name) => Some(LinkTarget::Document {
                name: name.clone(),
                fragment: None,
            }),
            NativeLinkTarget::Manual { name, section } => Some(LinkTarget::Manual {
                name: name.clone(),
                manual_section: Some(section.clone()),
            }),
        });
    }
    Ok(links)
}

fn heading_rows(
    native: &libmandoc_rs::structured::StructuredDocument,
) -> Result<
    HashMap<NativeBlockKey, &libmandoc_rs::structured::HeadingEvidence>,
    NativeProjectionError,
> {
    let mut rows = HashMap::new();
    for heading in native.heading_evidence() {
        if rows.insert(heading.block(), heading).is_some() {
            return Err(NativeProjectionError::InvalidRelation(
                "heading block has duplicate heading evidence",
            ));
        }
    }
    Ok(rows)
}

fn leading_anchor_points(
    native: &libmandoc_rs::structured::StructuredDocument,
    block: &NativeBlock,
    groups: &BTreeMap<ContentPointKey, AnchorGroup<'_>>,
) -> Result<BTreeSet<ContentPointKey>, NativeProjectionError> {
    let root = block.root().ok_or(NativeProjectionError::InvalidRelation(
        "heading block has no content root",
    ))?;
    let mut points = BTreeSet::new();
    for (key, group) in groups {
        if group.point.root() != root || group.point.owner() != block.owner() {
            continue;
        }
        let leading = match group.point.boundary() {
            PointBoundary::BetweenAtoms { atom_boundary } => *atom_boundary == 0,
            PointBoundary::InAtom { atom, byte_offset } => {
                *byte_offset == 0
                    && native
                        .content_atom(*atom)
                        .is_some_and(|atom| atom.root() == root && atom.ordinal() == 0)
            }
        };
        if leading {
            points.insert(*key);
        }
    }
    Ok(points)
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn anchor_identity_spelling<'a>(
    authored: Option<&'a str>,
    generated: &[&'a str],
) -> Option<&'a str> {
    authored.or_else(|| generated.first().copied())
}

fn identity_base(value: &str, fallback: &str, reserved_suffix: &str) -> String {
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

fn allocate_id(
    base: &str,
    used: &mut HashSet<String>,
    claims: &HashMap<String, BTreeSet<ContentPointKey>>,
    own_points: &BTreeSet<ContentPointKey>,
) -> String {
    let mut suffix = 1_u64;
    loop {
        let candidate = if suffix == 1 {
            base.to_owned()
        } else {
            format!("{base}-{suffix}")
        };
        let claimed_elsewhere = claims
            .get(&candidate)
            .is_some_and(|owners| owners.iter().any(|owner| !own_points.contains(owner)));
        if !used.contains(&candidate) && !claimed_elsewhere {
            used.insert(candidate.clone());
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{anchor_identity_spelling, identity_base, valid_alias};

    #[test]
    fn invalid_authored_alias_still_supplies_the_canonical_identity_spelling() {
        let authored = "Mixed Target";
        assert!(!valid_alias(authored));
        assert_eq!(
            anchor_identity_spelling(Some(authored), &["generated-target"]),
            Some(authored)
        );
        assert_eq!(identity_base(authored, "anchor", "anchor"), "mixed-target");
    }
}
