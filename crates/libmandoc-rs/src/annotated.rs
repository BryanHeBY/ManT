//! Experimental, owned post-device native display output.
//!
//! The ordinary parser and renderer remain unchanged. This surface is a
//! private migration entrance until the annotated IR and consumers are ready.
#![allow(missing_docs)]

use crate::{InputFormat, SourceBundle};

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
    /// Byte range in `AnnotatedDocument::join_text` for an authored separator.
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
    /// Half-open range in `AnnotatedDocument::selection_parts`.
    pub selection_first: u32,
    pub selection_count: u32,
    /// Native final body position, independently of authored provenance.
    pub point: Option<AnnotatedDisplayPoint>,
    /// Native tbl column and offset hint, not a final display point.
    pub native_table_position: Option<(u32, u64)>,
    /// Exact target spelling for anchors, optional `deroff()` authored
    /// heading phrase for headings; absent for all other mark kinds.
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
    /// Shared exact authored-separator bytes for selection joins.
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
