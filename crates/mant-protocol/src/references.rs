//! Bounded reference inventories, independent from readable outline nodes.

pub use mant_ir::ReferenceTargetType;
use mant_ir::{
    ContentLocation, ContentProjection, ContentReveal, DocumentAddress, LinkOccurrenceKey,
    LinkTarget, OutputSlice, ReferenceScanReport, ReferenceScanStop, SourceKey, SourceSpan,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// Whether reference facts are omitted, counted, or materialized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceProjectionMode {
    /// Do not traverse reference content.
    None,
    /// Count references without cloning labels or positions.
    #[default]
    Summary,
    /// Return a bounded occurrence page as well as counts.
    All,
}

/// Independent reference selection policy; entry filtering never changes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceProjection {
    /// Projection mode.
    #[serde(default)]
    pub mode: ReferenceProjectionMode,
    /// Original typed target kinds, defaulting to document/manual.
    #[serde(default = "default_target_types")]
    pub target_types: Vec<ReferenceTargetType>,
    /// Zero-based selected occurrence offset, not distinct-target offset.
    #[serde(default)]
    pub offset: u32,
    /// Maximum materialized occurrences, between 1 and 1,000.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_target_types() -> Vec<ReferenceTargetType> {
    vec![ReferenceTargetType::Document, ReferenceTargetType::Manual]
}
const fn default_limit() -> u32 {
    100
}

impl Default for ReferenceProjection {
    fn default() -> Self {
        Self {
            mode: ReferenceProjectionMode::Summary,
            target_types: default_target_types(),
            offset: 0,
            limit: 100,
        }
    }
}

impl ReferenceProjection {
    /// Validate a bounded request before scanning. An empty type set deliberately selects none.
    ///
    /// # Errors
    /// Rejects excessive or repeated kinds and invalid page limits.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.limit == 0 || self.limit > 1000 {
            return Err("reference limit must be between 1 and 1000");
        }
        if self.target_types.len() > 5 {
            return Err("at most five reference target types are accepted");
        }
        for (index, kind) in self.target_types.iter().enumerate() {
            if self.target_types[..index].contains(kind) {
                return Err("reference target types must not repeat");
            }
        }
        Ok(())
    }
}

/// Why bounded traversal could not inspect all selected source content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceLimitReason {
    /// Shared work-unit ceiling.
    Steps,
    /// Structural/inline nesting ceiling.
    Depth,
    /// Inspected target/label/form byte ceiling.
    Bytes,
    /// A position cannot be represented within its encoded-size ceiling.
    Position,
    /// Selected source root is invalid for the loaded snapshot.
    InvalidRoot,
    /// Consumer deliberately stopped the traversal.
    ConsumerStop,
}

impl From<ReferenceScanStop> for ReferenceLimitReason {
    fn from(value: ReferenceScanStop) -> Self {
        match value {
            ReferenceScanStop::Steps => Self::Steps,
            ReferenceScanStop::Depth => Self::Depth,
            ReferenceScanStop::Bytes => Self::Bytes,
            ReferenceScanStop::Position => Self::Position,
            ReferenceScanStop::InvalidRoot => Self::InvalidRoot,
            ReferenceScanStop::Visitor => Self::ConsumerStop,
        }
    }
}

/// Coverage is independent of target-set precision and returned-page truncation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReferenceCoverageStatus {
    /// Every selected content root was traversed.
    Complete {},
    /// Traversal stopped at a resource or structural boundary.
    Limited {
        /// First limit reached.
        reason: ReferenceLimitReason,
    },
    /// Reference projection was disabled.
    NotScanned {},
}

/// Accounted work and actual content coverage for one scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceCoverage {
    /// Charged traversal and optional validation steps.
    pub steps: u32,
    /// Charged target, label and form inspection bytes.
    pub bytes: u32,
    /// Whether all selected source content was inspected.
    pub status: ReferenceCoverageStatus,
}

impl ReferenceCoverage {
    /// Convert a bounded IR scan report without inventing complete coverage.
    #[must_use]
    pub fn from_report(report: ReferenceScanReport) -> Self {
        Self {
            steps: u32::try_from(report.steps).unwrap_or(u32::MAX),
            bytes: u32::try_from(report.bytes).unwrap_or(u32::MAX),
            status: report
                .stopped
                .map_or(ReferenceCoverageStatus::Complete {}, |reason| {
                    ReferenceCoverageStatus::Limited {
                        reason: reason.into(),
                    }
                }),
        }
    }
}

/// Precision of an occurrence or distinct-target count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReferenceCount {
    /// A complete count of the selected population.
    Exact {
        /// Exact count.
        value: u64,
    },
    /// A proven lower bound; unseen targets are not guessed.
    LowerBound {
        /// Number certainly observed.
        value: u64,
    },
    /// Counting was not attempted.
    Unknown {
        /// Why no count was established.
        reason: ReferenceUnknownReason,
    },
}

/// Why a requested quantity has no observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceUnknownReason {
    /// The caller selected references=none.
    Disabled,
    /// No authoritative document content was available for scanning.
    NotScanned,
    /// The caller supplied an invalid projection policy.
    InvalidPolicy,
}

/// A return-page cap, not a scan-coverage claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReferencePageLimit {
    /// Requested occurrence limit.
    Records,
    /// Total retained labels, positions, targets and metadata budget.
    MaterializationBytes,
    /// Exact position cannot fit its independent bound.
    Position,
    /// Traversal or optional association work was exhausted.
    Scan,
}

/// A bounded page over original link occurrences in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferencePage {
    /// Requested selected-occurrence offset.
    pub offset: u32,
    /// Number of retained records.
    pub returned: u32,
    /// Why additional requested records were not retained, if applicable.
    pub limited: Option<ReferencePageLimit>,
    /// Offered only when a fresh bounded scan has proven it can reach another occurrence.
    pub next_offset: Option<u32>,
}

/// Validation state of optional original form bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReferenceAssociation {
    /// No attached semantic owner or no recorded forms.
    Unrecorded {},
    /// Entire original form set was validated; indices refer to that owner.
    Valid {
        /// Exact semantic owner whose full form set was validated. This can be
        /// an ancestor of the nearest ordinary content item in `record.owner`.
        owner: ContentReveal,
        /// Zero-based original form indices sharing this occurrence.
        forms: Vec<u32>,
    },
    /// Invalid bindings are not partially projected.
    Invalid {},
    /// Association inspection hit a shared operation limit.
    Limited {},
}

/// Fragment facts permitted before a target document is loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UnloadedFragment {
    /// Original target did not specify a fragment.
    Absent {},
    /// Original fragment is retained in the target, but has not been checked.
    Unchecked {},
}

/// Fragment outcome in an actually loaded target snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LoadedFragment {
    /// Original target did not specify a fragment.
    Absent {},
    /// Exactly one logical content destination was found.
    Valid {
        /// Precise reveal destination, not a fabricated readable subtree.
        reveal: ContentReveal,
    },
    /// A complete target scan found no destination.
    Missing {},
    /// More than one logical destination matches.
    Ambiguous {},
    /// Limits prevented a complete destination check; this is not absence.
    Limited {},
}

/// Staged resolution; locating an address never implies loading or validating it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReferenceResolution {
    /// External/email targets are not probed.
    NotApplicable {},
    /// Catalog lookup is required, such as a manual without a section.
    NotQueried {
        /// Only unloaded fragment facts may be stated.
        fragment: UnloadedFragment,
    },
    /// A direct-file source has no registered namespace for relative resolution.
    MissingContext {
        /// Original fragment remains unchecked.
        fragment: UnloadedFragment,
    },
    /// Namespace rules derive this logical address; existence is not established.
    LogicalAddress {
        /// Namespace-confined logical destination.
        address: DocumentAddress,
        /// No target document has been loaded.
        fragment: UnloadedFragment,
    },
    /// The target is the already loaded source document; no extra I/O occurred.
    Loaded {
        /// Logical address when the source is registered; never a host path.
        address: Option<DocumentAddress>,
        /// Result based on that exact loaded snapshot.
        fragment: LoadedFragment,
    },
    /// Namespace/address grammar does not permit this logical reference.
    Restricted {},
}

/// One occurrence, not a grouped or deduplicated navigation row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceRecord {
    /// Document-local logical occurrence in this inventory's content projection.
    pub occurrence: LinkOccurrenceKey,
    /// Snapshot-local location of the original `Inline::Link`.
    pub origin: ContentLocation,
    /// Explicit containing readable source subtree. This is not the occurrence
    /// itself and never follows the reference's destination.
    pub source_read: crate::ContentSelector,
    /// Nearest original content item, when present.
    pub owner: Option<ContentReveal>,
    /// Bounded presentation preview; never authoritative link content.
    pub label_preview: String,
    /// Whether the UTF-8-safe presentation preview was bounded.
    pub label_preview_truncated: bool,
    /// Atomic validation outcome of original form bindings.
    pub association: ReferenceAssociation,
    /// Facts established without loading other documents or probing external URIs.
    pub resolution: ReferenceResolution,
}

/// Checked origin of one Fixed link, without pretending it is a Flow inline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedReferenceOrigin {
    /// Dense native occurrence key in the loaded Fixed document.
    pub link: NonZeroU32,
    /// Native section in which the occurrence began.
    pub section: Option<NonZeroU32>,
    /// Native owner in which the occurrence began.
    pub owner: Option<NonZeroU32>,
    /// First surviving final-run slice, using explicitly byte-based storage coordinates.
    pub first_slice: Option<OutputSlice>,
    /// Authored declaration position when the native execution proved one.
    pub source: Option<SourceSpan>,
    /// Authorized source identity even when no authored position can be proved.
    pub source_key: Option<SourceKey>,
}

/// One bounded Fixed reference; its typed target and preview come from the
/// validated native surface, not a reconstructed Flow content store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedReferenceRecord {
    /// Exact occurrence origin in the loaded Fixed snapshot.
    pub origin: FixedReferenceOrigin,
    /// A readable section/root selector, never the destination of this link.
    pub source_read: crate::ContentSelector,
    /// Original typed target, retained without probing another document.
    pub target: LinkTarget,
    /// Bounded presentation preview, not authoritative body storage.
    pub label_preview: String,
    /// Whether the native label was longer than this preview.
    pub label_preview_truncated: bool,
    /// Offline address and fragment state, without target-document I/O.
    pub resolution: ReferenceResolution,
}

/// Independent reference output embedded alongside an outline's content tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceInventory {
    /// Closed response-local store resolving every retained occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_projection: Option<ContentProjection>,
    /// Effective request policy.
    pub policy: ReferenceProjection,
    /// Source content scan coverage.
    pub coverage: ReferenceCoverage,
    /// Separate local-destination validation pass, sharing the operation budget.
    /// A limited target pass does not make an already exact occurrence count inexact.
    pub target_coverage: Option<ReferenceCoverage>,
    /// Occurrences within source root and selected target types.
    pub occurrences: ReferenceCount,
    /// Distinct complete typed targets, including fragments.
    pub targets: ReferenceCount,
    /// Occurrence-based return window.
    pub page: ReferencePage,
    /// Bounded records, empty for summary/none.
    pub records: Vec<ReferenceRecord>,
    /// Bounded Fixed occurrences. Exactly one of this and `records` is populated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_records: Vec<FixedReferenceRecord>,
}

#[derive(Deserialize)]
#[serde(
    remote = "ReferenceInventory",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ReferenceInventoryWire {
    #[serde(default)]
    pub content_projection: Option<ContentProjection>,
    pub policy: ReferenceProjection,
    pub coverage: ReferenceCoverage,
    pub target_coverage: Option<ReferenceCoverage>,
    pub occurrences: ReferenceCount,
    pub targets: ReferenceCount,
    pub page: ReferencePage,
    pub records: Vec<ReferenceRecord>,
    #[serde(default)]
    pub fixed_records: Vec<FixedReferenceRecord>,
}

impl<'de> Deserialize<'de> for ReferenceInventory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ReferenceInventoryWire::deserialize(deserializer)?;
        if !value.records.is_empty() && value.content_projection.is_none() {
            return Err(serde::de::Error::custom(
                "retained reference records require a content projection",
            ));
        }
        if !value.fixed_records.is_empty()
            && (!value.records.is_empty() || value.content_projection.is_some())
        {
            return Err(serde::de::Error::custom(
                "Fixed references cannot borrow a Flow content projection",
            ));
        }
        if value.page.returned as usize
            != value
                .records
                .len()
                .saturating_add(value.fixed_records.len())
        {
            return Err(serde::de::Error::custom(
                "reference page count does not match retained records",
            ));
        }
        let mut previous_link = 0;
        for record in &value.fixed_records {
            if record.origin.link.get() <= previous_link || record.label_preview.len() > 4096 {
                return Err(serde::de::Error::custom(
                    "Fixed reference keys must increase and previews must be bounded",
                ));
            }
            previous_link = record.origin.link.get();
            if record.origin.source.is_some() && record.origin.source_key.is_some() {
                return Err(serde::de::Error::custom(
                    "Fixed reference source and source key must be exclusive",
                ));
            }
            if record
                .origin
                .first_slice
                .is_some_and(|slice| slice.start_byte >= slice.end_byte)
            {
                return Err(serde::de::Error::custom(
                    "empty Fixed reference origin slice",
                ));
            }
        }
        if let Some(projection) = &value.content_projection {
            for record in &value.records {
                if projection.content_store.link(record.occurrence).is_none() {
                    return Err(serde::de::Error::custom(
                        "reference occurrence does not resolve in its content projection",
                    ));
                }
            }
        }
        Ok(value)
    }
}

impl ReferenceInventory {
    /// Empty, explicitly unscanned result, including tldr-only responses.
    #[must_use]
    pub fn not_scanned(policy: ReferenceProjection) -> Self {
        let reason = if policy.mode == ReferenceProjectionMode::None {
            ReferenceUnknownReason::Disabled
        } else {
            ReferenceUnknownReason::NotScanned
        };
        Self {
            content_projection: None,
            page: ReferencePage {
                offset: policy.offset,
                returned: 0,
                limited: None,
                next_offset: None,
            },
            policy,
            coverage: ReferenceCoverage {
                steps: 0,
                bytes: 0,
                status: ReferenceCoverageStatus::NotScanned {},
            },
            target_coverage: None,
            occurrences: ReferenceCount::Unknown { reason },
            targets: ReferenceCount::Unknown { reason },
            records: Vec::new(),
            fixed_records: Vec::new(),
        }
    }
}

impl Default for ReferenceInventory {
    fn default() -> Self {
        Self::not_scanned(ReferenceProjection::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_is_bounded_and_resolution_stages_are_closed() {
        assert!(ReferenceProjection::default().validate().is_ok());
        assert!(
            ReferenceProjection {
                limit: 1001,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ReferenceProjection {
                target_types: vec![ReferenceTargetType::Local; 2],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        for json in [
            r#"{"kind":"logical-address","address":{"kind":"markdown","path":"x","origin":{"kind":"documents"}},"fragment":{"kind":"valid","reveal":{"kind":"document"}}}"#,
            r#"{"kind":"not-applicable","resolved":true}"#,
            r#"{"kind":"loaded","address":null,"fragment":{"kind":"absent","reveal":{"kind":"document"}}}"#,
        ] {
            assert!(
                serde_json::from_str::<ReferenceResolution>(json).is_err(),
                "{json}"
            );
        }
    }

    #[test]
    fn count_precision_and_loaded_fragment_wire_have_no_contradictory_variants() {
        for value in [
            r#"{"kind":"exact","value":1,"reason":"disabled"}"#,
            r#"{"kind":"lower-bound"}"#,
            r#"{"kind":"unknown","reason":"disabled","value":0}"#,
        ] {
            assert!(
                serde_json::from_str::<ReferenceCount>(value).is_err(),
                "{value}"
            );
        }
        for value in [
            r#"{"kind":"missing","reveal":{"kind":"document"}}"#,
            r#"{"kind":"valid"}"#,
            r#"{"kind":"ambiguous","address":null}"#,
        ] {
            assert!(
                serde_json::from_str::<LoadedFragment>(value).is_err(),
                "{value}"
            );
        }
        let valid = ReferenceResolution::LogicalAddress {
            address: DocumentAddress::parse_catalog_path("documents/topic").unwrap(),
            fragment: UnloadedFragment::Unchecked {},
        };
        let wire = serde_json::to_string(&valid).unwrap();
        assert_eq!(
            serde_json::from_str::<ReferenceResolution>(&wire).unwrap(),
            valid
        );
    }

    #[test]
    fn retained_records_require_resolvable_occurrences() {
        let mut store = mant_ir::ContentStoreBuilder::new();
        let owner = store.push_owner(
            mant_ir::ContentOwnerKind::Document,
            mant_ir::Provenance::Unknown,
        );
        let root = store.push_root(
            owner,
            mant_ir::ContentRootKind::Body,
            mant_ir::Provenance::Unknown,
        );
        let occurrence = store.push_link(
            owner,
            mant_ir::LinkTarget::External {
                uri: "https://example.test".to_owned(),
            },
            None,
            mant_ir::Provenance::Unknown,
        );
        let _label = store.push_text(
            root,
            "authoritative label".to_owned(),
            None,
            mant_ir::ContentStyle::default(),
            None,
            Some(occurrence),
            mant_ir::Provenance::Unknown,
        );
        let mut inventory = ReferenceInventory {
            content_projection: Some(ContentProjection {
                content_store: store.finish(),
            }),
            ..ReferenceInventory::default()
        };
        inventory.page.returned = 1;
        inventory.records.push(ReferenceRecord {
            occurrence,
            origin: ContentLocation::DocumentHeading { path: vec![0] },
            source_read: crate::ContentSelector::path("root"),
            owner: None,
            label_preview: "authoritative…".to_owned(),
            label_preview_truncated: true,
            association: ReferenceAssociation::Unrecorded {},
            resolution: ReferenceResolution::NotApplicable {},
        });

        let encoded = serde_json::to_value(&inventory).expect("reference inventory");
        let restored = serde_json::from_value::<ReferenceInventory>(encoded.clone())
            .expect("closed occurrence");
        assert_eq!(restored.records[0].occurrence, occurrence);
        let projection = restored
            .content_projection
            .as_ref()
            .expect("record projection");
        assert_eq!(
            projection
                .content()
                .occurrence_plain_text(occurrence)
                .expect("complete label"),
            "authoritative label"
        );
        assert!(matches!(
            &projection
                .content_store
                .link(occurrence)
                .expect("complete target")
                .target,
            mant_ir::LinkTarget::External { uri } if uri == "https://example.test"
        ));

        let mut missing = encoded.clone();
        missing
            .as_object_mut()
            .expect("inventory object")
            .remove("contentProjection");
        assert!(serde_json::from_value::<ReferenceInventory>(missing).is_err());

        let mut dangling = encoded;
        dangling["records"][0]["occurrence"] = 2.into();
        assert!(serde_json::from_value::<ReferenceInventory>(dangling).is_err());
    }

    #[test]
    fn fixed_records_are_detached_from_flow_and_preserve_byte_coordinate_kind() {
        let mut inventory = ReferenceInventory::not_scanned(ReferenceProjection::default());
        inventory.page.returned = 1;
        inventory.fixed_records.push(FixedReferenceRecord {
            origin: FixedReferenceOrigin {
                link: NonZeroU32::new(1).unwrap(),
                section: None,
                owner: None,
                first_slice: Some(OutputSlice {
                    run: NonZeroU32::new(2).unwrap(),
                    start_byte: 1,
                    end_byte: 3,
                }),
                source: None,
                source_key: None,
            },
            source_read: crate::ContentSelector::path("root"),
            target: LinkTarget::External {
                uri: "https://example.test".into(),
            },
            label_preview: "xy".into(),
            label_preview_truncated: false,
            resolution: ReferenceResolution::NotApplicable {},
        });
        let encoded = serde_json::to_value(&inventory).unwrap();
        let restored = serde_json::from_value::<ReferenceInventory>(encoded.clone()).unwrap();
        assert_eq!(
            restored.fixed_records[0]
                .origin
                .first_slice
                .unwrap()
                .start_byte,
            1
        );

        let mut missing = encoded.clone();
        missing["page"]["returned"] = 0.into();
        assert!(serde_json::from_value::<ReferenceInventory>(missing).is_err());

        let mut empty_slice = encoded.clone();
        empty_slice["fixedRecords"][0]["origin"]["firstSlice"]["endByte"] = 1.into();
        assert!(serde_json::from_value::<ReferenceInventory>(empty_slice).is_err());

        let mut duplicate = encoded.clone();
        duplicate["fixedRecords"] = serde_json::json!([
            encoded["fixedRecords"][0].clone(),
            encoded["fixedRecords"][0].clone()
        ]);
        duplicate["page"]["returned"] = 2.into();
        assert!(serde_json::from_value::<ReferenceInventory>(duplicate).is_err());

        let mut conflicting_source = encoded.clone();
        conflicting_source["fixedRecords"][0]["origin"]["source"] =
            serde_json::to_value(SourceSpan {
                source: SourceKey::FIRST,
                byte_range: None,
                line: 1,
                column: 1,
                end_line: None,
                end_column: None,
            })
            .unwrap();
        conflicting_source["fixedRecords"][0]["origin"]["sourceKey"] =
            serde_json::to_value(SourceKey::FIRST).unwrap();
        assert!(serde_json::from_value::<ReferenceInventory>(conflicting_source).is_err());

        let mut mixed = encoded;
        mixed["contentProjection"] = serde_json::json!({"contentStore": {}});
        assert!(serde_json::from_value::<ReferenceInventory>(mixed).is_err());
    }
}
