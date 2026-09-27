//! Experimental, owned post-device native display output.
//!
//! The ordinary parser and renderer remain unchanged. This surface is a
//! private migration entrance until the annotated IR and consumers are ready.
#![allow(missing_docs)]

use crate::{InputFormat, SourceBundle};

/// Pinned roff.h discriminators for authored man(7) definition macros.
/// The native build asserts these values against `MAN_TP`, `MAN_TQ`,
/// `MAN_IP`, and `MAN_RS`.
#[doc(hidden)]
pub const MAN_TP_TOKEN: u32 = 382;
#[doc(hidden)]
pub const MAN_TQ_TOKEN: u32 = 383;
#[doc(hidden)]
pub const MAN_IP_TOKEN: u32 = 387;
/// Pinned plain bold macro discriminator for a TP/TQ term fallback.
#[doc(hidden)]
pub const MAN_B_TOKEN: u32 = 396;
/// Pinned `MAN_RS` discriminator; checked against the native region relation.
#[doc(hidden)]
pub const MAN_RS_TOKEN: u32 = 401;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedMetadata {
    pub macroset: u32,
    pub title: Option<String>,
    pub section: Option<String>,
    pub volume: Option<String>,
    pub operating_system: Option<String>,
    pub architecture: Option<String>,
    pub name: Option<String>,
    pub date: Option<String>,
    pub alias_target: Option<String>,
    pub has_body: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedSource {
    pub key: u32,
    pub identity_kind: u32,
    pub format: u32,
    pub coordinate_kind: u32,
    pub logical_name: String,
    pub decoded_length: u64,
    pub hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedSpan {
    pub source: u32,
    pub line_column: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedProvenance {
    pub kind: u32,
    pub authored_span: u32,
    pub generated_trigger_span: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedDiagnostic {
    pub level: u32,
    pub code: u32,
    pub message: String,
    pub span: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedLabel {
    pub owner: u32,
    pub link: u32,
    pub source: u32,
    pub head_component: u32,
    pub style: u32,
    pub role: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedRun {
    pub key: u32,
    pub column: u32,
    pub width: u32,
    pub byte_start: u64,
    pub byte_count: u64,
    pub label: AnnotatedLabel,
}

/// One surviving, direct mark selection in a final display run.
///
/// Byte offsets are relative to the run's UTF-8 text, not terminal columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedSelectionPart {
    pub run: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub join_before: AnnotatedTextJoin,
    /// Byte range in `AnnotatedDocument::join_text` for an exact native
    /// authored or formatter-generated separator.
    /// Other join kinds have a zero start and length.
    pub join_text_start: u64,
    pub join_text_len: u64,
}

/// Known native relationship to the previous part of the same selection.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotatedTextJoin {
    None = 0,
    DirectContact = 1,
    AuthoredSeparator = 2,
    HardBoundary = 3,
    Unknown = 4,
    GeneratedSeparator = 5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedRow {
    pub key: u32,
    pub first_run: u32,
    pub run_count: u32,
    pub column_count: u32,
    pub break_after: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedMark {
    pub key: u32,
    pub kind: u32,
    pub parent: u32,
    pub owner: u32,
    pub source: u32,
    pub line: u32,
    pub column: u32,
    pub token: u32,
    pub region_kind: u32,
    pub title_region: u32,
    pub body_region: u32,
    pub flags: u32,
    /// Earlier direct man definition sibling in the same native flow.
    /// Zero means no proven predecessor; this is not shared body content.
    pub preceding_owner: u32,
    /// Half-open range in `AnnotatedDocument::selection_parts`.
    pub selection_first: u32,
    pub selection_count: u32,
    /// Native final body position, independently of authored provenance.
    pub point: Option<AnnotatedDisplayPoint>,
    /// Native tbl column and offset hint, not a final display point.
    pub native_table_position: Option<(u32, u64)>,
    /// Exact target spelling for anchors, optional `deroff()` authored
    /// heading phrase for headings, or a conservative native Fl/Ev operand
    /// on an owner. The latter must still match final visible head glyphs.
    pub name: Option<String>,
    /// Decoded destination, when this native mark represents one link target.
    pub link_target: Option<AnnotatedLinkTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotatedDisplayPoint {
    /// One-based final body row and zero-based terminal-column boundary.
    RowColumn { row: u32, column: u32 },
    /// Final body end, including an entirely empty surface.
    DocumentEnd { row_count: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedLinkTarget {
    pub kind: u32,
    pub primary: String,
    pub secondary: Option<String>,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationProducer {
    Native = 1,
    Codec = 2,
    Validator = 3,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationDimension {
    Section = 1,
    OwnerBoundary = 2,
    Declaration = 3,
    Link = 4,
    Anchor = 5,
    Relation = 6,
    Source = 7,
    Join = 8,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationCheckState {
    Checked = 1,
    NotApplicable = 2,
    Unverified = 3,
    Pending = 4,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationIssueReason {
    NotObserved = 1,
    Unverified = 2,
    Rejected = 3,
    AmbiguousSurvival = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationScope {
    Document,
    Section(u32),
    Owner(u32),
    Region(u32),
    Source(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationSourcePosition {
    pub source: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationCoverageCheck {
    pub producer: AnnotationProducer,
    pub dimension: AnnotationDimension,
    pub state: AnnotationCheckState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationCoverageIssue {
    pub producer: AnnotationProducer,
    pub dimension: AnnotationDimension,
    pub reason: AnnotationIssueReason,
    pub scope: AnnotationScope,
    pub source: Option<AnnotationSourcePosition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationCoverage {
    pub checks: Vec<AnnotationCoverageCheck>,
    pub issues: Vec<AnnotationCoverageIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedDocument {
    pub root_source: u32,
    pub profile: u32,
    pub width: u32,
    /// The checked native display survived, but native mark relations were
    /// discarded. Consumers must report semantic coverage as incomplete.
    pub annotation_degraded: bool,
    pub metadata: AnnotatedMetadata,
    pub sources: Vec<AnnotatedSource>,
    pub spans: Vec<AnnotatedSpan>,
    pub provenances: Vec<AnnotatedProvenance>,
    pub diagnostics: Vec<AnnotatedDiagnostic>,
    /// The sole owned visible-body text arena. Runs refer to byte intervals.
    pub text: String,
    pub rows: Vec<AnnotatedRow>,
    pub runs: Vec<AnnotatedRun>,
    pub marks: Vec<AnnotatedMark>,
    /// Final, surviving direct owner/link selections, grouped by mark key.
    pub selection_parts: Vec<AnnotatedSelectionPart>,
    /// Shared exact authored or native-generated separator bytes for joins.
    pub join_text: String,
    pub coverage: AnnotationCoverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedError {
    pub status: u32,
    pub stage: u32,
    pub limit_kind: u32,
    pub observed: u64,
    pub allowed: u64,
}

impl std::fmt::Display for AnnotatedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "native annotated render failed (status {}, stage {})",
            self.status, self.stage
        )
    }
}

impl std::error::Error for AnnotatedError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotatedRenderer {
    width: u32,
    limits: crate::structured::StructuredLimits,
}

impl Default for AnnotatedRenderer {
    fn default() -> Self {
        Self {
            width: 78,
            limits: crate::structured::StructuredLimits::default(),
        }
    }
}

impl AnnotatedRenderer {
    /// Select a fixed native terminal width.
    ///
    /// # Errors
    /// Returns an input error when the width is outside 20–1000 columns.
    pub fn new(width: u32) -> Result<Self, AnnotatedError> {
        if !(20..=1_000).contains(&width) {
            return Err(AnnotatedError {
                status: 1,
                stage: 1,
                limit_kind: 0,
                observed: u64::from(width),
                allowed: 1_000,
            });
        }
        Ok(Self {
            width,
            ..Self::default()
        })
    }

    /// Set the maximum captured native output bytes.
    ///
    /// # Errors
    /// Returns an input error when the limit is zero.
    pub fn with_max_content_bytes(mut self, maximum: u64) -> Result<Self, AnnotatedError> {
        if maximum == 0 {
            return Err(AnnotatedError {
                status: 1,
                stage: 1,
                limit_kind: 0,
                observed: 0,
                allowed: 1,
            });
        }
        self.limits.max_content_bytes = maximum;
        Ok(self)
    }

    /// Set the shared native collector and display work budget.
    ///
    /// # Errors
    /// Returns an input error when the limit is zero.
    pub fn with_max_builder_operations(mut self, maximum: u64) -> Result<Self, AnnotatedError> {
        if maximum == 0 {
            return Err(AnnotatedError {
                status: 1,
                stage: 1,
                limit_kind: 0,
                observed: 0,
                allowed: 1,
            });
        }
        self.limits.max_builder_operations = maximum;
        Ok(self)
    }

    /// Set the shared native collector and display allocation budget.
    ///
    /// # Errors
    /// Returns an input error when the limit is zero.
    pub fn with_max_builder_allocated_bytes(
        mut self,
        maximum: u64,
    ) -> Result<Self, AnnotatedError> {
        if maximum == 0 {
            return Err(AnnotatedError {
                status: 1,
                stage: 1,
                limit_kind: 0,
                observed: 0,
                allowed: 1,
            });
        }
        self.limits.max_builder_allocated_bytes = maximum;
        Ok(self)
    }

    /// Render one authorized source bundle into an owned final-cell surface.
    ///
    /// # Errors
    /// Returns a staged native, resource, or relation error; no partial page
    /// is returned.
    pub fn render_bundle(
        &self,
        root: &str,
        bundle: &SourceBundle,
        format: InputFormat,
    ) -> Result<AnnotatedDocument, AnnotatedError> {
        crate::ffi::render_annotated(root, bundle, format, self.width, &self.limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_mark_text(page: &AnnotatedDocument, mark: &AnnotatedMark) -> String {
        let mut text = String::new();
        for part in &page.selection_parts
            [mark.selection_first as usize..(mark.selection_first + mark.selection_count) as usize]
        {
            if matches!(
                part.join_before,
                AnnotatedTextJoin::AuthoredSeparator | AnnotatedTextJoin::GeneratedSeparator
            ) {
                let start = usize::try_from(part.join_text_start).unwrap();
                let end = start + usize::try_from(part.join_text_len).unwrap();
                text.push_str(&page.join_text[start..end]);
            }
            let run = &page.runs[(part.run - 1) as usize];
            let start = usize::try_from(run.byte_start + part.start_byte).unwrap();
            let end = usize::try_from(run.byte_start + part.end_byte).unwrap();
            text.push_str(&page.text[start..end]);
        }
        text
    }

    #[test]
    fn native_head_components_follow_surviving_macro_instances() {
        // Pinned mdoc_macro.c::blk_full keeps both Fl elements in one HEAD;
        // mdoc_term.c::termp_fl_pre emits each dash inside its own frame.
        // The exact input was run with the fixed CVS -Ttree and -Tutf8 first.
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a , Fl b\nBODY\n.El\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert_eq!(
            components
                .iter()
                .map(|mark| direct_mark_text(&page, mark))
                .collect::<Vec<_>>(),
            ["-a", "-b"]
        );
        assert!(components.iter().all(|mark| mark.flags & 32 != 0));
        assert!(
            page.runs
                .iter()
                .any(|run| run.label.head_component == components[0].key)
        );
        assert!(
            page.runs
                .iter()
                .any(|run| run.label.head_component == components[1].key)
        );
    }

    #[test]
    fn native_va_dv_and_multiple_ev_keep_distinct_authored_roles() {
        // Exact input ran pinned CVS -Ttree/-Tutf8 first. mdoc_macro.c::
        // in_line() creates each Ev/Va/Dv element separately in the It HEAD;
        // mdoc_term.c assigns different initial fonts, not semantic names.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev ONE , Ev TWO\nBody.\n.It Va counter\nVariable.\n.It Dv MODE_FAST\nConstant.\n.El\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let roles = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2)
            .map(|mark| mark.flags & (32 | 64 | 128 | 256 | 2048 | 4096))
            .collect::<Vec<_>>();
        assert_eq!(roles, [64, 2048, 4096]);
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .map(|mark| {
                (
                    mark.flags & (32 | 64 | 128 | 256 | 2048 | 4096),
                    direct_mark_text(&page, mark),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            components,
            [
                (64, "ONE".to_owned()),
                (64, "TWO".to_owned()),
                (2048, "counter".to_owned()),
                (4096, "MODE_FAST".to_owned()),
            ]
        );
        assert!(page.text.contains("Body."));
        assert!(page.text.contains("Variable."));
        assert!(page.text.contains("Constant."));
    }

    #[test]
    fn native_ar_component_is_not_an_emphasis_or_owner_role() {
        // Exact input ran pinned CVS -Tutf8 -Owidth=78 before this assertion.
        // mdoc_macro.c::in_line retains Ar and Em as different ELEM nodes;
        // mdoc_term.c::termp_under_pre gives both the same final underline,
        // so only the authored Ar node can authenticate a parameter boundary.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm AddressFamily Ar address_family\nThe address family.\n.It Cm Key No prose\nA prose tail.\n.It Cm Key Em prose\nAn emphasized tail.\n.It Cm color=[yes|no\nAn unclosed value.\n.El\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let roles = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2)
            .map(|mark| mark.flags & (32 | 64 | 128 | 256 | 2048 | 4096 | 8192))
            .collect::<Vec<_>>();
        assert_eq!(roles, [128, 128, 128, 128]);
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .map(|mark| {
                (
                    mark.flags & (32 | 64 | 128 | 256 | 2048 | 4096 | 8192),
                    direct_mark_text(&page, mark),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            components,
            [
                (128, "AddressFamily".to_owned()),
                (8192, "address_family".to_owned()),
                (128, "Key".to_owned()),
                (128, "Key".to_owned()),
                (128, "color=[yes|no".to_owned()),
            ]
        );
    }

    #[test]
    fn native_empty_roles_and_generated_va_do_not_reject_readable_body() {
        // Exact input ran pinned CVS -Tutf8 first. term.c::term_word()
        // emits no glyph for \&, while mdoc_validate.c::post_rv() creates
        // a NODE_NOSRC Va for errno outside this definition HEAD.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Va \\& , Ev HOME\nVisible body.\n.It Dv \\&\nAnother body.\n.El\n.Rv\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert_eq!(components.len(), 3);
        assert_eq!(components[0].flags & 2048, 2048);
        assert_eq!(components[0].selection_count, 0);
        assert_eq!(components[1].flags & 64, 64);
        assert_eq!(direct_mark_text(&page, components[1]), "HOME");
        assert_eq!(components[2].flags & 4096, 4096);
        assert_eq!(components[2].selection_count, 0);
        assert!(components.iter().all(|mark| mark.source == 1));
        assert!(page.text.contains("Visible body."));
        assert!(page.text.contains("Another body."));
        assert!(page.text.contains("global variable"));
    }

    #[test]
    fn expanded_va_dv_roles_keep_source_identity_without_fake_coordinates() {
        // Exact input ran pinned CVS -Tutf8 first. read.c reparses the
        // user macro at its invocation SourceKey; its expanded coordinates
        // are not authored line/column positions.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.de Vars\n.It Va counter , Dv MODE_FAST\nExpanded body.\n..\n.Bl -tag -width Ds\n.Vars\n.El\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].flags & 2048, 2048);
        assert_eq!(components[1].flags & 4096, 4096);
        assert!(components.iter().all(|mark| {
            mark.source == 1 && mark.line == 0 && mark.column == 0 && mark.flags & 1 == 0
        }));
        assert_eq!(direct_mark_text(&page, components[0]), "counter");
        assert_eq!(direct_mark_text(&page, components[1]), "MODE_FAST");
        assert!(page.text.contains("Expanded body."));
    }

    #[test]
    fn native_diag_head_does_not_parse_va_spelling_as_a_role() {
        // Exact input ran pinned CVS -Ttree/-Tutf8 first. mdoc_macro.c::
        // blk_full() leaves -diag HEAD text unparsed; mdoc_term.c styles the
        // displayed label, but no Va macro instance exists in the AST.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.Bl -diag\n.It Va counter\nBody.\n.El\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        assert!(
            page.marks
                .iter()
                .any(|mark| mark.kind == 2 && mark.flags & 16 != 0)
        );
        assert!(!page.marks.iter().any(|mark| mark.kind == 6));
        assert!(page.text.contains("Va counter"));
        assert!(page.text.contains("Body."));
    }

    #[test]
    fn native_va_dv_roles_survive_font_escapes_in_their_operands() {
        // Exact input ran pinned CVS -Tutf8 first. mdoc_term.c starts Va
        // underlined and Dv roman, but term.c::term_word() executes each
        // embedded \f escape afterward; role evidence follows the macro.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Va \\fBcounter\\fP\nVariable.\n.It Dv \\fIMODE_FAST\\fP\nConstant.\n.El\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].flags & 2048, 2048);
        assert_eq!(components[1].flags & 4096, 4096);
        assert_eq!(direct_mark_text(&page, components[0]), "counter");
        assert_eq!(direct_mark_text(&page, components[1]), "MODE_FAST");
    }

    #[test]
    fn native_head_component_does_not_infer_a_second_styled_option() {
        // Pinned mdoc_macro.c::blk_full builds the second macro as Sy, not Fl.
        // The exact input was run with the fixed CVS -Ttree first.
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a , Sy -b\nBODY\n.El\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert_eq!(
            components
                .iter()
                .map(|mark| direct_mark_text(&page, mark))
                .collect::<Vec<_>>(),
            ["-a"]
        );
    }

    #[test]
    fn native_man_b_keeps_one_complete_head_component() {
        // Pinned man_macro.c::blk_imp gives TP one HEAD, and man_term.c::pre_B
        // styles a single text child; exact input ran CVS -Ttree/-Tutf8.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH OPTIONS\n.TP\n.B -a, --all\nBODY\n".to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert_eq!(
            components
                .iter()
                .map(|mark| direct_mark_text(&page, mark))
                .collect::<Vec<_>>(),
            ["-a, --all"]
        );
        assert!(components.iter().all(|mark| mark.flags & 256 != 0));
    }

    #[test]
    fn native_zero_glyph_head_components_remain_checked_instances() {
        // Exact inputs ran pinned CVS -Ttree/-Tutf8 first. man_term.c::pre_B
        // executes `\&` without a glyph, while pre_alternate() still visits
        // its empty children; term.c::term_word() leaves no visible selection.
        for (input, format) in [
            (
                b".TH T 1\n.SH OPTIONS\n.TP\n.B \\&\nDescription.\n".as_slice(),
                InputFormat::Man,
            ),
            (
                b".TH T 1\n.SH OPTIONS\n.TP\n.BR \\& \\&\nDescription.\n".as_slice(),
                InputFormat::Man,
            ),
            (
                b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Ev \\&\nDescription.\n.El\n"
                    .as_slice(),
                InputFormat::Mdoc,
            ),
        ] {
            let mut bundle = SourceBundle::new();
            bundle.insert("t.1", input.to_vec()).unwrap();
            let page = AnnotatedRenderer::default()
                .render_bundle("t.1", &bundle, format)
                .unwrap();
            let components = page
                .marks
                .iter()
                .filter(|mark| mark.kind == 6)
                .collect::<Vec<_>>();
            assert!(!components.is_empty());
            assert!(components.iter().all(|mark| mark.selection_count == 0));
            assert!(page.text.contains("Description."));
        }
    }

    #[test]
    fn native_bi_keeps_later_bold_operand_after_empty_operands() {
        // Exact input ran pinned CVS -Ttree/-Tutf8/-Thtml first.
        // man_term.c::pre_alternate() visits the third, bold child even
        // though its first two children emit nothing.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" \"\" \"--help \" FILE\nDescription.\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
        assert_ne!(owner.flags & 256, 0);
        let components = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 6)
            .collect::<Vec<_>>();
        assert!(components.iter().any(|mark| {
            mark.selection_count != 0 && direct_mark_text(&page, mark).contains("--help")
        }));
    }

    #[test]
    fn native_direct_tp_text_witness_excludes_alternating_macro_heads() {
        // Each exact input ran pinned CVS -Tutf8 first. man_macro.c::blk_imp
        // keeps a TP HEAD; man_term.c::pre_TP prints its first next-line
        // child, which can be direct text or an entire BI macro instance.
        for (input, direct_text) in [
            (
                b".TH T 1\n.SH OPTIONS\n.TP\nFILE\nDescription.\n".as_slice(),
                true,
            ),
            (
                b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" \"--fake\"\nDescription.\n".as_slice(),
                false,
            ),
            (
                b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" \"\" \"--help \" FILE\nDescription.\n"
                    .as_slice(),
                false,
            ),
        ] {
            let mut bundle = SourceBundle::new();
            bundle.insert("t.1", input.to_vec()).unwrap();
            let page = AnnotatedRenderer::default()
                .render_bundle("t.1", &bundle, InputFormat::Man)
                .unwrap();
            let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
            assert_eq!(owner.flags & 1024 != 0, direct_text);
            assert_ne!(owner.flags & 256, 0);
        }
    }

    #[test]
    fn native_tp_width_is_not_a_label_or_reading_group_boundary() {
        // Exact input ran pinned CVS -Ttree first. man_macro.c::blk_imp
        // retains the same-line width in HEAD, while man_term.c::pre_TP
        // prints only children from the first NODE_LINE onward.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH OPTIONS\n.TP 4\n.B -a\n.TQ\n.B --all\nShared description.\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let owners = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2)
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 2);
        assert!(owners.iter().all(|owner| owner.flags & 256 != 0));
        assert_eq!(owners[1].preceding_owner, owners[0].key);
        let heads = owners
            .iter()
            .map(|owner| direct_mark_text(&page, &page.marks[(owner.title_region - 1) as usize]))
            .collect::<Vec<_>>();
        assert_eq!(heads, ["-a", "--all"]);
    }

    #[test]
    fn native_bi_and_plain_ip_heads_are_candidates_without_width_names() {
        // Both exact inputs ran pinned CVS -Ttree first. man_term.c::pre_TP
        // traverses the BI macro's two styled operands; pre_IP prints only
        // the first text operand and treats the second as layout width.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH OPTIONS\n.TP\n.BI --output= FILE\nOutput description.\n".to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
        assert_ne!(owner.flags & 256, 0);
        let component = page.marks.iter().find(|mark| mark.kind == 6).unwrap();
        assert_eq!(direct_mark_text(&page, component), "--output=");

        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH OPTIONS\n.IP --alpha 4\nAlpha description.\n.IP item 4\nItem description.\n.IP 1 4\nNumeric description.\n"
                    .to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let owners = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2)
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 3);
        assert_eq!(
            owners
                .iter()
                .map(|owner| owner.flags & 16 != 0)
                .collect::<Vec<_>>(),
            [true, true, false]
        );
        assert!(owners.iter().all(|owner| owner.name.is_none()));
        let heads = owners
            .iter()
            .map(|owner| direct_mark_text(&page, &page.marks[(owner.title_region - 1) as usize]))
            .collect::<Vec<_>>();
        assert_eq!(heads, ["--alpha", "item", "1"]);
    }

    #[test]
    fn native_alternating_font_components_keep_each_visible_operand() {
        // Both exact inputs ran pinned CVS -Ttree first. man_term.c::
        // pre_alternate() emits each child directly without a nested NODE
        // event, and TERMP_NOSPACE joins the operands in final display.
        // Both intervals are structural; only final checked glyphs name an option.
        for operand in ["FILE", "-x"] {
            let mut bundle = SourceBundle::new();
            let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.BI --foo {operand}\nDescription.\n");
            bundle.insert("t.1", input.into_bytes()).unwrap();
            let page = AnnotatedRenderer::default()
                .render_bundle("t.1", &bundle, InputFormat::Man)
                .unwrap();
            let components = page
                .marks
                .iter()
                .filter(|mark| mark.kind == 6)
                .collect::<Vec<_>>();
            assert_eq!(components.len(), 2);
            assert_eq!(direct_mark_text(&page, components[0]), "--foo");
            assert_eq!(direct_mark_text(&page, components[1]), operand);
            let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
            assert_eq!(
                direct_mark_text(&page, &page.marks[(owner.title_region - 1) as usize]),
                format!("--foo{operand}")
            );
        }
    }

    #[test]
    fn native_italic_and_roman_macro_labels_keep_final_style_evidence() {
        // Both exact inputs ran pinned CVS -Ttree/-Tutf8 first. man_term.c::
        // pre_I and bare R do not create a bold macro declaration. pre_IP
        // still prints italic-only and one-letter labels as ordinary content;
        // C may retain a broad lexical candidate for the former, but the
        // final font and checked name scan must decide whether it is a name.
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".TH T 1\n.SH OPTIONS\n.TP\n.I --italic\nDescription.\n.TP\n.R --roman\nDescription.\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        assert!(
            page.marks
                .iter()
                .filter(|mark| mark.kind == 2)
                .all(|owner| owner.flags & 256 == 0)
        );

        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".TH T 1\n.SH OPTIONS\n.IP \\fI--italic\\fP 4\nDescription.\n.IP o 4\nBullet-like.\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let owners = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2)
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 2);
        assert!(owners[0].flags & 16 != 0);
        assert_eq!(owners[1].flags & (16 | 256), 0);
        assert_eq!(
            owners
                .iter()
                .map(|owner| direct_mark_text(
                    &page,
                    &page.marks[(owner.title_region - 1) as usize]
                ))
                .collect::<Vec<_>>(),
            ["--italic", "o"]
        );
    }

    #[test]
    fn native_multiword_command_component_keeps_internal_space_not_leading_padding() {
        // Exact input ran pinned CVS -Tutf8 first. term.c::term_word()
        // inserts AUTO_SPACE before each operand; mdoc_term.c renders the
        // two Cm children as one visibly spaced macro phrase.
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Cm foo bar\nbody\n.El\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let component = page.marks.iter().find(|mark| mark.kind == 6).unwrap();
        assert_eq!(direct_mark_text(&page, component), "foo bar");
        assert!(component.selection_count >= 2);

        // The exact zero-width first-operand variant also ran pinned CVS:
        // it renders only "foo", with no manufactured leading padding.
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Cm \\& foo\nbody\n.El\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let component = page.marks.iter().find(|mark| mark.kind == 6).unwrap();
        assert_eq!(direct_mark_text(&page, component), "foo");
    }

    #[test]
    fn native_soft_wrap_records_generated_separator_without_authored_provenance() {
        // Exact fixture ran pinned CVS -Ttree/-Tutf8 before this assertion.
        // term.c::term_word writes AUTO_SPACE; term_flushln consumes its
        // buffer slot at WRAP, while the Xo HEAD stays one native region.
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/roff/annotated-mdoc-xo-generated-space.1"
        ));
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", input.to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        let head = page
            .marks
            .iter()
            .find(|mark| mark.kind == 5 && mark.region_kind == 3)
            .unwrap();
        assert_eq!(
            direct_mark_text(&page, head),
            "run [-alpha] [-bravo] [-charlie] [-delta] [-echo] [-foxtrot] [-golf] [-hotel]"
        );
        let parts = &page.selection_parts
            [head.selection_first as usize..(head.selection_first + head.selection_count) as usize];
        assert!(parts.iter().any(|part| {
            part.join_before == AnnotatedTextJoin::GeneratedSeparator && part.join_text_len > 0
        }));
    }

    #[test]
    fn native_man_body_transfers_one_checked_surface() {
        // Pinned man_term.c executes the section head and body through the
        // terminal device; the exact input was checked with the CVS oracle.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("t.1", b".TH T 1\n.SH D\nbody\n".to_vec())
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        assert!(page.text.contains("body"));
        assert_eq!(page.root_source, 1);
        assert_eq!(page.sources.len(), 1);
        assert!(!page.rows.is_empty());
        assert!(page.marks.iter().any(|mark| mark.kind == 1));
    }

    #[test]
    fn native_mdoc_body_survives_header_and_footer_filtering() {
        // Pinned mdoc_term.c head/foot frames; exact bytes were checked with
        // the fixed CVS UTF-8 terminal before adding this assertion.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\nbody\n".to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        assert!(page.text.contains("body"));
        assert!(!page.text.contains("General Commands Manual"));
        assert!(!page.text.contains("September 23, 2026"));
        assert!(page.marks.iter().any(|mark| mark.kind == 1));
    }

    #[test]
    fn native_nofill_table_and_equation_are_one_display() {
        // Pinned man_term.c dispatches tbl/eqn after the previous display is
        // flushed; exact input was run through the fixed CVS UTF-8 oracle.
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", b".TH T 1\n.SH D\n.nf\nbefore\n.TS\ntab(;);\nl l.\na;b\n.TE\n.fi\n.EQ\nx sup 2\n.EN\n".to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        assert!(page.text.contains("before"));
        assert!(page.text.contains('a'));
        assert!(page.text.contains('b'));
        assert!(page.text.contains('x'));
        assert!(page.marks.iter().any(|mark| mark.region_kind == 7));
        assert!(page.marks.iter().any(|mark| mark.region_kind == 8));
    }

    #[test]
    fn native_overstrike_is_a_safe_final_surface() {
        // Pinned term.c::term_field and term_ascii.c::utf8_letter emit ordered
        // backspaces. This exact input was run with the fixed CVS -Tutf8
        // oracle; the annotated result is the normalized final-cell view.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH D\n.nf\n\\zAB\n\\o'ab'\n\\fBA\\fP\n\\z\xe7\x95\x8cX\n.fi\n".to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        assert!(!page.text.contains('\u{0008}'));
        assert!(page.text.contains('B'));
        assert!(page.text.contains('X'));
        assert!(page.runs.iter().any(|run| run.label.style & 1 != 0));
    }

    #[test]
    fn native_anchor_keeps_authored_declaration_source() {
        // Pinned tag.c::mant_tag_put_manual/tag_move_id keep the .Tg source
        // separate from its eventual carrier; the exact input was run with
        // the fixed CVS terminal before this assertion.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Tg Here\nbody\n".to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        assert!(page.marks.iter().any(|mark| {
            mark.kind == 4
                && mark.name.as_deref() == Some("Here")
                && mark.source == 1
                && mark.line == 5
        }));
    }

    #[test]
    fn macro_generated_marks_keep_source_without_authored_coordinates() {
        // Both exact inputs ran the pinned CVS -Thtml reference: generated
        // SH, TP, and UR all render. read.c::mparse_buf_r retains the input
        // source key but substitutes expanded line coordinates.
        for call in [".EE", ".EE ignored-padding-for-coordinate-check"] {
            let input = format!(
                ".TH T 1\n.de EE\n.SH OPTIONS\n.TP\n.B --macro-generated\nDescription.\n.UR https://example.test/x\nlabel\n.UE\n..\n{call}\n"
            );
            let mut bundle = SourceBundle::new();
            bundle.insert("expanded.1", input.into_bytes()).unwrap();
            let page = AnnotatedRenderer::default()
                .render_bundle("expanded.1", &bundle, InputFormat::Man)
                .expect("macro-generated marks retain source-only coordinates");
            for kind in [1, 2, 3, 6] {
                let mark = page
                    .marks
                    .iter()
                    .find(|mark| mark.kind == kind && mark.source == 1)
                    .unwrap_or_else(|| panic!("missing generated mark kind {kind} for {call}"));
                assert_eq!((mark.line, mark.column), (0, 0), "kind {kind} {call}");
                assert_eq!(mark.flags & 1, 0, "kind {kind} {call}");
            }
            assert!(page.text.contains("--macro-generated"), "{call}");
            assert!(page.text.contains("Description."), "{call}");
            assert!(page.text.contains("label"), "{call}");
        }
    }

    fn assert_native_hanging_case(
        input: &str,
        expected_candidate: bool,
        expected_continuation: bool,
        head_text: &str,
        head_line: u32,
    ) {
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", input.as_bytes().to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        assert!(page.text.contains("--git-dir"), "{input}");
        assert!(
            page.text.contains("Description.") || page.text.contains("Set repository directory."),
            "{input}"
        );
        let candidates = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2 && mark.flags & 512 != 0)
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), usize::from(expected_candidate), "{input}");
        let continuations = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 5 && mark.region_kind == 12)
            .collect::<Vec<_>>();
        assert_eq!(
            continuations.len(),
            usize::from(expected_continuation),
            "{input}"
        );
        if let Some(owner) = candidates.first() {
            assert_eq!(owner.source, 1, "{input}");
            assert_eq!(owner.line, head_line, "{input}");
            let head = &page.marks[(owner.title_region - 1) as usize];
            assert_eq!(head.region_kind, 3, "{input}");
            assert!(direct_mark_text(&page, head).contains(head_text), "{input}");
            let body = &page.marks[(owner.body_region - 1) as usize];
            assert_eq!(body.region_kind, 4, "{input}");
            assert_eq!(body.selection_count, 0, "{input}");
            if let Some(continuation) = continuations.first() {
                assert_eq!(continuation.preceding_owner, owner.key, "{input}");
                assert_eq!(continuation.parent, owner.parent, "{input}");
                assert_eq!(continuation.owner, 0, "{input}");
                assert!(
                    direct_mark_text(&page, continuation).contains("Description.")
                        || direct_mark_text(&page, continuation)
                            .contains("Set repository directory."),
                    "{input}"
                );
                assert_eq!(owner.flags & (16 | 256), 16 | 256, "{input}");
            } else {
                assert_eq!(owner.flags & (16 | 256), 0, "{input}");
            }
        }
    }

    #[test]
    fn native_paragraph_aliases_keep_hanging_presentation_boundaries() {
        // Each exact input ran pinned CVS -Ttree and -Tutf8 before these
        // assertions. man_validate.c::post_SH unwraps the first PP/P/LP;
        // all three execute man_term.c::pre_PP. man_term.c::pre_RS computes
        // the effective device indent after its BLOCK flush.
        let cases = [
            (
                ".TH T 1\n.SH OPTIONS\n.PP\n.B --git-dir\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.LP\n.B --git-dir\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.P\n.B --git-dir\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nintro\n.PP\n.B --git-dir\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                5,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nintro\n.LP\n.B --git-dir\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                5,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nintro\n.P\n.B --git-dir\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                5,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.PP\n\\fB--git-dir\\fR\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "--git-dir",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.PP\n.B --git-dir\nextra prose\n.RS 4\nDescription.\n.RE\n",
                true,
                true,
                "extra prose",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.PP\n.B --git-dir\n.RS 0\nDescription.\n.RE\n",
                true,
                false,
                "--git-dir",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.LP\n.B --git-dir\n.RS 0\nDescription.\n.RE\n",
                true,
                false,
                "--git-dir",
                4,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.P\n.B --git-dir\n.RS 0\nDescription.\n.RE\n",
                true,
                false,
                "--git-dir",
                4,
            ),
        ];
        for (input, candidate, continuation, head, line) in cases {
            assert_native_hanging_case(input, candidate, continuation, head, line);
        }
    }

    #[test]
    fn native_implicit_first_paragraph_only_hands_off_to_direct_rs() {
        // Each exact input ran pinned CVS -Ttree and -Tutf8. The first SH
        // BODY paragraph shares the section epoch; the removed .br changes
        // that epoch before B, and cannot become an inferred head.
        let cases = [
            (
                ".TH T 1\n.SH OPTIONS\n.B --git-dir=<path>\n.RS 4\nSet repository directory.\n.RE\n",
                true,
                true,
                "--git-dir",
                3,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nintro\n.B --git-dir=<path>\n.RS 4\nSet repository directory.\n.RE\n",
                true,
                true,
                "intro",
                3,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.br\n.B --git-dir=<path>\n.RS 4\nSet repository directory.\n.RE\n",
                false,
                false,
                "--git-dir",
                0,
            ),
        ];
        for (input, candidate, continuation, head, line) in cases {
            assert_native_hanging_case(input, candidate, continuation, head, line);
        }
    }

    #[test]
    fn native_retained_space_starts_another_direct_hanging_paragraph() {
        // This exact input ran pinned CVS -Ttree/-Tutf8 first. roff.c::
        // roff_node_alloc() gives the retained .sp and following declaration
        // one flow epoch; man_term.c::pre_RS() indents only the direct sibling
        // RS body. Neither the previous owner nor .sp becomes the new head.
        let input = ".TH T 1\n.SH OPTIONS\n\\fB\\-a\\fP\n.RS 4\nFirst description.\n.RE\n.sp\n\\fB\\-g\\fP \\fIGLOB\\fP, \\fB\\-\\-glob\\fP=\\fIGLOB\\fP\n.RS 4\nGlob description.\n.RE\n";
        let mut bundle = SourceBundle::new();
        bundle.insert("t.1", input.as_bytes().to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let candidates = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 2 && mark.flags & 512 != 0)
            .collect::<Vec<_>>();
        let continuations = page
            .marks
            .iter()
            .filter(|mark| mark.kind == 5 && mark.region_kind == 12)
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 2);
        assert_eq!(continuations.len(), 2);
        assert_eq!((candidates[0].line, candidates[1].line), (3, 8));
        assert_eq!(candidates[0].parent, candidates[1].parent);
        for (owner, continuation) in candidates.iter().zip(&continuations) {
            assert_eq!(continuation.preceding_owner, owner.key);
            assert_eq!(continuation.parent, owner.parent);
        }
        assert_eq!(page.text.matches("--glob").count(), 1);
        assert_eq!(page.text.matches("Glob description.").count(), 1);
    }

    #[test]
    fn native_space_candidate_stays_within_its_executed_scope() {
        // Each exact input ran pinned CVS -Ttree first. post_SH() may remove
        // a leading .sp, but a retained .sp starts one section-local epoch;
        // deleted .br still advances the epoch and cannot borrow that .sp.
        // A nested RS has a different parent; no-fill is not a paragraph.
        let cases = [
            (
                ".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n.ft B\n\\fB--foo\\fP\n.RS 4\nBody.\n.RE\n",
                1,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nIntro.\n.sp 0\n\\fB--foo\\fP\n.RS 4\nBody.\n.RE\n",
                1,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n.sp\n\\fB--foo\\fP\n.RS 4\nBody.\n.RE\n",
                1,
            ),
            (
                ".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n.br\n\\fB--foo\\fP\n.RS 4\nBody.\n.RE\n",
                0,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.sp\n\\fB--foo\\fP\n.SH NEXT\n.RS 4\nEXAMPLE\n.RE\n",
                0,
            ),
            (
                ".TH T 1\n.SH OPTIONS\n.RS 4\n.sp\n\\fB--inner\\fP\n.RS 4\nINNER\n.RE\n.RE\n",
                0,
            ),
            (".TH T 1\n.SH OPTIONS\n.sp\n\\fB--foo\\fP\n.RS 4\n.RE\n", 0),
            (
                ".TH T 1\n.SH OPTIONS\n.nf\n.sp\n\\fB--foo\\fP\n.RS 4\nEXAMPLE\n.RE\n.fi\n",
                0,
            ),
        ];
        for (input, expected) in cases {
            let mut bundle = SourceBundle::new();
            bundle.insert("t.1", input.as_bytes().to_vec()).unwrap();
            let page = AnnotatedRenderer::default()
                .render_bundle("t.1", &bundle, InputFormat::Man)
                .unwrap();
            let candidates = page
                .marks
                .iter()
                .filter(|mark| mark.kind == 2 && mark.flags & 512 != 0)
                .count();
            assert_eq!(candidates, expected, "{input}");
            assert!(!page.text.is_empty(), "{input}");
        }
    }

    #[test]
    fn native_hanging_alternating_font_keeps_both_head_operands() {
        // Both exact inputs ran pinned CVS -Tutf8 first: man_term.c::
        // pre_alternate() renders --foo in bold and FILE in italic without
        // nested child NODE events. The PP candidate is not a Definition
        // until pre_RS BODY has computed its positive device indent.
        for paragraph in [".PP\n", ""] {
            let input = format!(
                ".TH T 1\n.SH OPTIONS\n{paragraph}.BI --foo FILE\n.RS 4\nDescription.\n.RE\n"
            );
            let mut bundle = SourceBundle::new();
            bundle.insert("t.1", input.into_bytes()).unwrap();
            let page = AnnotatedRenderer::default()
                .render_bundle("t.1", &bundle, InputFormat::Man)
                .unwrap();
            let owner = page
                .marks
                .iter()
                .find(|mark| mark.kind == 2 && mark.flags & 512 != 0)
                .expect("whole PP or implicit first paragraph presentation");
            let head = &page.marks[(owner.title_region - 1) as usize];
            assert_eq!(direct_mark_text(&page, head), "--fooFILE");
            let components = page
                .marks
                .iter()
                .filter(|mark| mark.kind == 6 && mark.parent == head.key)
                .collect::<Vec<_>>();
            assert_eq!(components.len(), 2);
            assert_eq!(direct_mark_text(&page, components[0]), "--foo");
            assert_eq!(direct_mark_text(&page, components[1]), "FILE");
            assert_ne!(components[0].flags & 256, 0);
            assert_ne!(components[1].flags & 256, 0);
            assert!(page.marks.iter().any(|mark| mark.kind == 5
                && mark.region_kind == 12
                && mark.preceding_owner == owner.key));
        }
    }

    #[test]
    fn resource_failures_return_no_partial_page_and_next_call_recovers() {
        // Same exact minimal input as the pinned CVS man_term.c smoke above.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("t.1", b".TH T 1\n.SH D\nbody\n".to_vec())
            .unwrap();
        let mut mark_limited = AnnotatedRenderer::default();
        mark_limited.limits.max_transfer_objects = 1;
        for renderer in [
            AnnotatedRenderer::default()
                .with_max_content_bytes(1)
                .unwrap(),
            AnnotatedRenderer::default()
                .with_max_content_bytes(20)
                .unwrap(),
            AnnotatedRenderer::default()
                .with_max_builder_operations(1)
                .unwrap(),
            AnnotatedRenderer::default()
                .with_max_builder_allocated_bytes(1)
                .unwrap(),
            mark_limited,
        ] {
            let error = renderer
                .render_bundle("t.1", &bundle, InputFormat::Man)
                .unwrap_err();
            assert_eq!(error.status, 3, "{error:?}");
            assert!(error.limit_kind != 0, "{error:?}");
            assert!(error.observed > error.allowed, "{error:?}");
            assert!(
                AnnotatedRenderer::default()
                    .render_bundle("t.1", &bundle, InputFormat::Man)
                    .is_ok()
            );
        }
    }

    #[test]
    fn margin_mark_budget_failure_does_not_poison_the_next_session() {
        // Exact input ran pinned CVS -Tutf8 -O width=78. term.c::endline()
        // emits .mc after the field, so the generated display mark is
        // allocated during output rather than while walking an AST node.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.mc |\nsome body\n.br\n.mc\n".to_vec(),
            )
            .unwrap();
        let normal = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let margin_key = normal
            .marks
            .iter()
            .find(|mark| mark.kind == 5 && mark.region_kind == 11)
            .unwrap()
            .key;
        let mut limited = AnnotatedRenderer::default();
        limited.limits.max_transfer_objects = u64::from(margin_key - 1);
        let error = limited
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap_err();
        assert_eq!(error.status, 3, "{error:?}");
        assert!(error.observed > error.allowed, "{error:?}");
        assert!(
            AnnotatedRenderer::default()
                .render_bundle("t.1", &bundle, InputFormat::Man)
                .is_ok()
        );
    }

    #[test]
    fn independent_threads_render_without_shared_session_state() {
        // Same fixed-CVS minimal man input as the transfer test.
        let tasks = (0..4).map(|_| {
            std::thread::spawn(|| {
                let mut bundle = SourceBundle::new();
                bundle
                    .insert("t.1", b".TH T 1\n.SH D\nbody\n".to_vec())
                    .unwrap();
                AnnotatedRenderer::default()
                    .render_bundle("t.1", &bundle, InputFormat::Man)
                    .map(|page| page.text)
            })
        });
        for task in tasks {
            assert!(task.join().unwrap().unwrap().contains("body"));
        }
    }

    #[test]
    fn authored_link_space_follows_the_field_not_layout_indent() {
        // Pinned term.c::term_field defers FIELD_SKIP whitespace until
        // ascii_advance emits actual bytes; exact input was run with CVS.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "t.1",
                b".TH T 1\n.SH D\n.UR https://example.test\na b\n.UE\n".to_vec(),
            )
            .unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .unwrap();
        let start = page.text.find("a b").unwrap();
        for offset in start..start + 3 {
            let run = page
                .runs
                .iter()
                .find(|run| {
                    (run.byte_start..run.byte_start + run.byte_count).contains(&(offset as u64))
                })
                .unwrap();
            assert_ne!(run.label.link, 0, "offset {offset} lost link");
        }
    }
}
