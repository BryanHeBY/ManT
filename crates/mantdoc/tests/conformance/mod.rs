#![doc = include_str!("README.md")]
#![warn(missing_docs)]

//! Checksum-pinned identities and native parser regression support.

use std::{collections::BTreeSet, fmt, path::Path};

use mantdoc::{FatalError, ParseReport, Parser, Source, SourceName};
use serde::Deserialize;
use sha2::{Digest, Sha256};

mod canonical;
mod corpus;

#[allow(unused_imports)]
pub use canonical::{
    CANONICAL_AST_SCHEMA, CANONICAL_DIAGNOSTIC_SCHEMA, CANONICAL_MDOC_OPERATING_SYSTEM,
    CanonicalDiagnostic, CanonicalDocument, CanonicalEnclosure, CanonicalFlags, CanonicalLocation,
    CanonicalMetadata, CanonicalNode, CanonicalParse, CanonicalTableCell, canonicalize_mantdoc,
};
#[allow(unused_imports)]
pub use corpus::{
    CorpusArchiveError, CorpusArchiveErrorKind, CorpusCase, CorpusCasePayload, CorpusInventory,
    ReferenceOutput, ReferenceOutputPayload, RendererCasePayload, stable_1_14_6_case,
    stable_1_14_6_inventory, stable_1_14_6_reference_output, stable_1_14_6_renderer_case,
};

const M3_EXECUTION_MANIFEST: &str = include_str!("data/m3-execution.toml");

/// Repository-relative path of the native M3 execution gate.
pub const M3_EXECUTION_MANIFEST_PATH: &str =
    "crates/mantdoc/tests/conformance/data/m3-execution.toml";

/// Number of checksum-pinned traditional man(7) inputs in mandoc 1.14.6.
pub const M4_STABLE_MAN_CASE_COUNT: usize = 99;

/// Number of checksum-pinned semantic mdoc(7) inputs in mandoc 1.14.6.
pub const M5_STABLE_MDOC_CASE_COUNT: usize = 276;

/// Number of checksum-pinned tbl(7) and eqn(7) inputs in mandoc 1.14.6.
pub const M6_STABLE_PREPROCESS_CASE_COUNT: usize = 58;

/// One validated native-parser result from the M3 execution gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M3ExecutionCaseResult {
    /// Checksum-pinned upstream case identity.
    pub case_id: Box<str>,
    /// SHA-256 of the exact decompressed source passed to the native parser.
    pub source_sha256: Box<str>,
    /// Nodes in the bounded immutable document.
    pub ast_nodes: usize,
    /// Number of recoverable parser findings.
    pub diagnostic_count: usize,
    /// Native roff expansion and reparse work counter.
    pub expansion_steps: usize,
    /// Whether deterministic recovery limits yielded a prefix document.
    pub truncated: bool,
}

/// Failure while loading or checking the native M3 execution gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum M3ExecutionGateError {
    /// The checked-in gate manifest has an invalid schema or expectation.
    Manifest {
        /// Precise schema or invariant failure.
        message: Box<str>,
    },
    /// The pinned upstream archive or selected case could not be validated.
    Corpus(CorpusArchiveError),
    /// The backend configuration or exact source no longer matches its case identity.
    CaseValidation(CaseValidationError),
    /// The native parser could not construct a coherent report for one gate case.
    Fatal {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Typed native parser failure.
        error: FatalError,
    },
    /// A bounded parser outcome differs from the reviewed gate expectation.
    Mismatch {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Report field that diverged.
        field: &'static str,
        /// Checked-in reviewed expectation.
        expected: Box<str>,
        /// Observed native parser outcome.
        actual: Box<str>,
    },
}

impl fmt::Display for M3ExecutionGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest { message } => {
                write!(formatter, "invalid M3 execution manifest: {message}")
            }
            Self::Corpus(error) => {
                write!(formatter, "M3 execution corpus validation failed: {error}")
            }
            Self::CaseValidation(error) => {
                write!(formatter, "M3 execution case validation failed: {error}")
            }
            Self::Fatal { case_id, error } => {
                write!(
                    formatter,
                    "M3 execution case {case_id} failed fatally: {error}"
                )
            }
            Self::Mismatch {
                case_id,
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "M3 execution case {case_id} mismatched {field}: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for M3ExecutionGateError {}

/// Aggregate result from parsing the complete stable man(7) regression lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M4ManSmokeResult {
    /// Number of exact upstream man inputs checked by this run.
    pub case_count: usize,
    /// Number of cases with reviewed recoverable diagnostics.
    pub diagnostic_case_count: usize,
}

/// Failure while running the complete native M4 man smoke gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum M4ManSmokeGateError {
    /// The pinned archive did not satisfy the corpus inventory contract.
    Corpus(CorpusArchiveError),
    /// A selected case or parser configuration did not preserve its identity.
    CaseValidation(CaseValidationError),
    /// A man input failed before producing a bounded parser report.
    Fatal {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Typed native parser failure.
        error: FatalError,
    },
    /// A stable man input unexpectedly exhausted a parser resource bound.
    Truncated {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
    },
    /// A case emitted a diagnostic sequence outside the reviewed recovery set.
    Diagnostics {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Reviewed diagnostic code sequence.
        expected: Box<str>,
        /// Observed diagnostic code sequence.
        actual: Box<str>,
    },
    /// The archive inventory no longer contains the frozen number of man inputs.
    CaseCount {
        /// Observed number of man inputs.
        actual: usize,
    },
}

impl fmt::Display for M4ManSmokeGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corpus(error) => write!(formatter, "M4 man corpus validation failed: {error}"),
            Self::CaseValidation(error) => {
                write!(formatter, "M4 man case validation failed: {error}")
            }
            Self::Fatal { case_id, error } => {
                write!(formatter, "M4 man case {case_id} failed fatally: {error}")
            }
            Self::Truncated { case_id } => {
                write!(formatter, "M4 man case {case_id} unexpectedly truncated")
            }
            Self::Diagnostics {
                case_id,
                expected,
                actual,
            } => write!(
                formatter,
                "M4 man case {case_id} emitted unexpected diagnostics: expected {expected}, got {actual}"
            ),
            Self::CaseCount { actual } => write!(
                formatter,
                "M4 man corpus case count changed: expected {M4_STABLE_MAN_CASE_COUNT}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for M4ManSmokeGateError {}

/// Aggregate result from parsing the complete stable mdoc(7) regression lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M5MdocSmokeResult {
    /// Number of exact upstream mdoc inputs checked by this run or shard.
    pub case_count: usize,
    /// Number of checked cases with reviewed recoverable diagnostics.
    pub diagnostic_case_count: usize,
}

/// Failure while running the complete native M5 mdoc smoke gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum M5MdocSmokeGateError {
    /// The pinned archive did not satisfy the corpus inventory contract.
    Corpus(CorpusArchiveError),
    /// A selected case or parser configuration did not preserve its identity.
    CaseValidation(CaseValidationError),
    /// An mdoc input failed before producing a bounded parser report.
    Fatal {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Typed native parser failure.
        error: FatalError,
    },
    /// A stable mdoc input unexpectedly exhausted a parser resource bound.
    Truncated {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
    },
    /// A case emitted a diagnostic sequence outside the reviewed recovery set.
    Diagnostics {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Reviewed diagnostic code sequence.
        expected: Box<str>,
        /// Observed diagnostic code sequence.
        actual: Box<str>,
    },
    /// The archive inventory no longer contains the frozen number of mdoc inputs.
    CaseCount {
        /// Observed number of mdoc inputs.
        actual: usize,
    },
    /// The requested deterministic corpus shard was outside its partition.
    InvalidShard {
        /// Zero-based requested shard index.
        shard_index: usize,
        /// Total number of shards in the partition.
        shard_count: usize,
    },
}

impl fmt::Display for M5MdocSmokeGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corpus(error) => write!(formatter, "M5 mdoc corpus validation failed: {error}"),
            Self::CaseValidation(error) => {
                write!(formatter, "M5 mdoc case validation failed: {error}")
            }
            Self::Fatal { case_id, error } => {
                write!(formatter, "M5 mdoc case {case_id} failed fatally: {error}")
            }
            Self::Truncated { case_id } => {
                write!(formatter, "M5 mdoc case {case_id} unexpectedly truncated")
            }
            Self::Diagnostics {
                case_id,
                expected,
                actual,
            } => write!(
                formatter,
                "M5 mdoc case {case_id} emitted unexpected diagnostics: expected {expected}, got {actual}"
            ),
            Self::CaseCount { actual } => write!(
                formatter,
                "M5 mdoc corpus case count changed: expected {M5_STABLE_MDOC_CASE_COUNT}, got {actual}"
            ),
            Self::InvalidShard {
                shard_index,
                shard_count,
            } => write!(
                formatter,
                "M5 mdoc shard {shard_index}/{shard_count} is outside the zero-based partition"
            ),
        }
    }
}

impl std::error::Error for M5MdocSmokeGateError {}

/// Aggregate result from parsing the stable tbl(7) and eqn(7) regression lanes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M6PreprocessSmokeResult {
    /// Number of exact upstream tbl/eqn inputs checked by this run.
    pub case_count: usize,
    /// Number of cases with reviewed recoverable diagnostics.
    pub diagnostic_case_count: usize,
}

/// Failure while running the initial native M6 tbl/eqn smoke gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum M6PreprocessSmokeGateError {
    /// The pinned archive did not satisfy the corpus inventory contract.
    Corpus(CorpusArchiveError),
    /// A selected case or parser configuration did not preserve its identity.
    CaseValidation(CaseValidationError),
    /// A tbl/eqn input failed before producing a bounded parser report.
    Fatal {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Typed native parser failure.
        error: FatalError,
    },
    /// A stable tbl/eqn input unexpectedly exhausted a parser resource bound.
    Truncated {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
    },
    /// A case emitted a diagnostic sequence outside the reviewed recovery set.
    Diagnostics {
        /// Checksum-pinned upstream case identity.
        case_id: Box<str>,
        /// Reviewed diagnostic code sequence.
        expected: Box<str>,
        /// Observed diagnostic code sequence.
        actual: Box<str>,
    },
    /// The archive inventory no longer contains the frozen number of tbl/eqn inputs.
    CaseCount {
        /// Observed number of tbl/eqn inputs.
        actual: usize,
    },
}

impl fmt::Display for M6PreprocessSmokeGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corpus(error) => {
                write!(formatter, "M6 tbl/eqn corpus validation failed: {error}")
            }
            Self::CaseValidation(error) => {
                write!(formatter, "M6 tbl/eqn case validation failed: {error}")
            }
            Self::Fatal { case_id, error } => {
                write!(
                    formatter,
                    "M6 tbl/eqn case {case_id} failed fatally: {error}"
                )
            }
            Self::Truncated { case_id } => {
                write!(
                    formatter,
                    "M6 tbl/eqn case {case_id} unexpectedly truncated"
                )
            }
            Self::Diagnostics {
                case_id,
                expected,
                actual,
            } => write!(
                formatter,
                "M6 tbl/eqn case {case_id} emitted unexpected diagnostics: expected {expected}, got {actual}"
            ),
            Self::CaseCount { actual } => write!(
                formatter,
                "M6 tbl/eqn corpus case count changed: expected {M6_STABLE_PREPROCESS_CASE_COUNT}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for M6PreprocessSmokeGateError {}

#[derive(Deserialize)]
struct M3ExecutionManifest {
    schema: String,
    corpus_id: String,
    #[serde(rename = "case")]
    cases: Vec<M3ExecutionExpectation>,
}

#[derive(Deserialize)]
struct M3ExecutionExpectation {
    id: String,
    source_sha256: String,
    ast_nodes: usize,
    expansion_steps: usize,
    truncated: bool,
    #[serde(default)]
    diagnostics: Vec<M3ExecutionDiagnosticExpectation>,
}

#[derive(Deserialize)]
struct M3ExecutionDiagnosticExpectation {
    code: String,
    start: u32,
    end: u32,
}

/// Stable identity of one exact decompressed parser input/configuration pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseIdentity {
    /// Manifest corpus lane such as `mandoc-stable-1.14.6`.
    pub corpus_id: Box<str>,
    /// Corpus-local case identifier.
    pub case_id: Box<str>,
    /// Logical root used by the parser/resolver.
    pub logical_root: Box<str>,
    /// SHA-256 of exactly the decompressed source bytes.
    pub decompressed_source_sha256: Box<str>,
    /// Hash of syntax, limits, recovery, and resolver configuration.
    pub parser_config_fingerprint: Box<str>,
    /// Include graph hash when resolver behavior contributed to this case.
    pub source_graph_hash: Option<Box<str>>,
}

/// Exact bytes and identity sent to a checked native parser configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseInput {
    /// Hash-keyed conformance identity.
    pub identity: CaseIdentity,
    /// Caller-facing logical source name.
    pub source_name: SourceName,
    /// Unmodified decompressed input bytes.
    pub bytes: Vec<u8>,
}

/// Construct a hash-bound conformance input from one verified stable upstream case.
///
/// The selected source has already passed the archive and per-case checks in
/// [`stable_1_14_6_case`]. This function adds the exact parser configuration
/// identity required before a named backend may run it.
///
/// # Errors
///
/// Returns the typed corpus validation error from [`stable_1_14_6_case`] when
/// the archive or requested case is not trustworthy.
pub fn stable_1_14_6_case_input(
    archive_path: impl AsRef<Path>,
    case_id: &str,
    config: &mantdoc::ParserConfig,
) -> Result<CaseInput, CorpusArchiveError> {
    Ok(case_input_from_payload(
        "mandoc-stable-1.14.6",
        stable_1_14_6_case(archive_path, case_id)?,
        config,
    ))
}

/// A case's declared identity does not match the exact data or configuration
/// presented to the configured parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaseValidationError {
    /// The source bytes differ from the decompressed-source hash in the case identity.
    SourceSha256Mismatch {
        /// Hash declared by the checked-in or verified corpus identity.
        expected: Box<str>,
        /// Hash calculated from the actual byte slice about to be parsed.
        actual: Box<str>,
    },
    /// The backend configuration differs from the case's declared configuration.
    ParserConfigFingerprintMismatch {
        /// Backend whose supplied configuration did not match.
        backend: &'static str,
        /// Fingerprint declared by the case identity.
        expected: Box<str>,
        /// Fingerprint calculated by the backend.
        actual: Box<str>,
    },
}

impl std::fmt::Display for CaseValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceSha256Mismatch { expected, actual } => write!(
                formatter,
                "case source SHA-256 mismatch: expected {expected}, got {actual}"
            ),
            Self::ParserConfigFingerprintMismatch {
                backend,
                expected,
                actual,
            } => write!(
                formatter,
                "{backend} parser configuration fingerprint mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for CaseValidationError {}

/// One parser implementation that can run a conformance case.
pub trait ParseBackend {
    /// Stable backend label included in every comparison record.
    fn name(&self) -> &'static str;

    /// Return the canonical fingerprint of this backend's parser configuration.
    fn parser_config_fingerprint(&self) -> &str;

    /// Parse exactly the supplied byte source.
    ///
    /// # Errors
    ///
    /// Returns the parser's fatal, bounded-session error without losing its
    /// stable category.
    fn parse(&self, source: Source<'_>) -> Result<ParseReport, FatalError>;
}

/// Adapter around the native parser configuration used by archive-backed tests.
#[derive(Clone, Debug)]
pub struct MantdocBackend {
    parser: Parser,
    parser_config_fingerprint: Box<str>,
}

impl MantdocBackend {
    /// Construct an adapter around one immutable parser configuration.
    #[must_use]
    pub fn new(parser: Parser) -> Self {
        let parser_config_fingerprint = parser_config_fingerprint(parser.config());
        Self {
            parser,
            parser_config_fingerprint,
        }
    }

    /// Borrow the immutable native parser configuration used by this backend.
    #[must_use]
    pub fn parser_config(&self) -> &mantdoc::ParserConfig {
        self.parser.config()
    }
}

impl Default for MantdocBackend {
    fn default() -> Self {
        Self::new(Parser::default())
    }
}

impl ParseBackend for MantdocBackend {
    fn name(&self) -> &'static str {
        "mantdoc"
    }

    fn parser_config_fingerprint(&self) -> &str {
        &self.parser_config_fingerprint
    }

    fn parse(&self, source: Source<'_>) -> Result<ParseReport, FatalError> {
        self.parser.parse(source)
    }
}

/// Output from a single named backend for one exact case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendRun {
    /// Backend label selected by the harness.
    pub backend: &'static str,
    /// Exact input identity.
    pub case: CaseIdentity,
    /// Complete parser outcome; later stages canonicalize it by comparison layer.
    pub outcome: Result<ParseReport, FatalError>,
}

/// Run one hash-verified case through one explicitly named backend.
///
/// # Errors
///
/// Returns [`CaseValidationError`] before parsing when source bytes or parser
/// configuration do not equal the values declared in `case.identity`.
pub fn run_case(
    backend: &impl ParseBackend,
    case: &CaseInput,
) -> Result<BackendRun, CaseValidationError> {
    validate_case(backend, case)?;
    Ok(BackendRun {
        backend: backend.name(),
        case: case.identity.clone(),
        outcome: backend.parse(Source::new(&case.source_name, &case.bytes)),
    })
}

/// Run every reviewed native roff-execution case from the checksum-pinned
/// mandoc 1.14.6 archive.
///
/// The runner first validates the checked-in manifest, archive identity,
/// decompressed source hashes, and native parser configuration.  It then
/// requires every node count, work count, truncation state, and diagnostic
/// code/span sequence to equal the M3 expectation.  This deliberately checks
/// execution behavior only; canonical parser and renderer checks are separate.
///
/// # Errors
///
/// Returns [`M3ExecutionGateError`] if the manifest is invalid, the archive is
/// not the pinned upstream payload, or any native parser result diverges from
/// its reviewed execution expectation.
pub fn run_m3_execution_gate(
    archive_path: impl AsRef<Path>,
) -> Result<Vec<M3ExecutionCaseResult>, M3ExecutionGateError> {
    run_m3_execution(archive_path.as_ref(), true)
}

/// Produce the checked M3 execution inputs' current report counters without
/// comparing them to the checked-in node-count expectation.
///
/// This maintainer-facing inspection helper still validates the manifest,
/// archive, source identity, parser configuration, and fatal parser outcome.
/// It is used when deliberately rebasing a lower-level execution baseline
/// after an approved later structural phase; CI must use
/// [`run_m3_execution_gate`] instead.
///
/// # Errors
///
/// Returns the same identity and fatal-outcome errors as the M3 gate.
pub fn inspect_m3_execution_reports(
    archive_path: impl AsRef<Path>,
) -> Result<Vec<M3ExecutionCaseResult>, M3ExecutionGateError> {
    run_m3_execution(archive_path.as_ref(), false)
}

fn run_m3_execution(
    archive_path: &Path,
    assert_expectations: bool,
) -> Result<Vec<M3ExecutionCaseResult>, M3ExecutionGateError> {
    let manifest = parse_m3_execution_manifest()?;
    let backend = m3_roff_backend();
    let mut results = Vec::with_capacity(manifest.cases.len());

    for expected in manifest.cases {
        let input = stable_1_14_6_case_input(archive_path, &expected.id, backend.parser_config())
            .map_err(M3ExecutionGateError::Corpus)?;
        if input.identity.decompressed_source_sha256.as_ref() != expected.source_sha256 {
            return Err(mismatch(
                &expected.id,
                "source_sha256",
                &expected.source_sha256,
                &input.identity.decompressed_source_sha256,
            ));
        }

        let run = run_case(&backend, &input).map_err(M3ExecutionGateError::CaseValidation)?;
        let report = run.outcome.map_err(|error| M3ExecutionGateError::Fatal {
            case_id: expected.id.clone().into(),
            error,
        })?;
        let result = M3ExecutionCaseResult {
            case_id: expected.id.clone().into(),
            source_sha256: expected.source_sha256.clone().into(),
            ast_nodes: report.document.node_count(),
            diagnostic_count: report.diagnostics.len(),
            expansion_steps: report.statistics.expansion_steps,
            truncated: report.statistics.truncated,
        };
        if assert_expectations {
            assert_m3_execution_expectation(&expected, &report, &result)?;
        }
        results.push(result);
    }

    Ok(results)
}

/// Parse every checksum-pinned stable man(7) regression input with the native
/// backend.
///
/// This is intentionally a parser and recovery gate, not an AST parity claim:
/// it requires a finite report for all 99 stable inputs and fixes every
/// diagnostic sequence emitted by the current native baseline. Canonical
/// parser and renderer checks remain separate release gates.
///
/// # Errors
///
/// Returns [`M4ManSmokeGateError`] when archive identity, input identity,
/// parser completion, truncation state, or the reviewed diagnostic set differs.
pub fn run_m4_man_smoke_gate(
    archive_path: impl AsRef<Path>,
) -> Result<M4ManSmokeResult, M4ManSmokeGateError> {
    let archive_path = archive_path.as_ref();
    let inventory = stable_1_14_6_inventory(archive_path).map_err(M4ManSmokeGateError::Corpus)?;
    let cases = inventory
        .cases
        .into_iter()
        .filter(|case| case.id.starts_with("regress/man/"))
        .collect::<Vec<_>>();
    if cases.len() != M4_STABLE_MAN_CASE_COUNT {
        return Err(M4ManSmokeGateError::CaseCount {
            actual: cases.len(),
        });
    }

    let backend = MantdocBackend::default();
    let mut diagnostic_case_count = 0;
    for case in cases {
        let input = stable_1_14_6_case_input(archive_path, &case.id, backend.parser_config())
            .map_err(M4ManSmokeGateError::Corpus)?;
        let run = run_case(&backend, &input).map_err(M4ManSmokeGateError::CaseValidation)?;
        let report = run.outcome.map_err(|error| M4ManSmokeGateError::Fatal {
            case_id: case.id.clone(),
            error,
        })?;
        if report.statistics.truncated {
            return Err(M4ManSmokeGateError::Truncated { case_id: case.id });
        }
        let expected = m4_expected_diagnostic_codes(&case.id);
        let actual = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(M4ManSmokeGateError::Diagnostics {
                case_id: case.id,
                expected: expected.join(",").into(),
                actual: actual.join(",").into(),
            });
        }
        diagnostic_case_count += usize::from(!actual.is_empty());
    }

    Ok(M4ManSmokeResult {
        case_count: M4_STABLE_MAN_CASE_COUNT,
        diagnostic_case_count,
    })
}

#[allow(clippy::too_many_lines)] // The exhaustive stable-case contract is intentionally one auditable mapping.
fn m4_expected_diagnostic_codes(case_id: &str) -> &'static [&'static str] {
    match case_id {
        "regress/man/B/args" | "regress/man/SH/broken" | "regress/man/SS/broken" => {
            &["man.line-scope-broken"]
        }
        "regress/man/TP/broken" => &[
            "man.line-scope-broken",
            "man.blank-line-scope",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
        ],
        "regress/man/TP/eof" => &["man.line-scope-broken"],
        "regress/man/B/blank" => &["man.blank-line-scope"],
        "regress/man/TH/case" => &["man.title-not-uppercase"],
        "regress/man/BI/emptyargs" => &["input.trailing-whitespace"],
        "regress/man/BI/literal"
        | "regress/man/RS/breaking"
        | "regress/man/TH/baddate"
        | "regress/man/TH/longdate"
        | "regress/man/TH/onlyyear" => &["man.title-date-unparseable"],
        "regress/man/TH/emptydate" => &["man.title-date-missing"],
        "regress/man/TH/noTH" => &["man.title-missing", "man.title-date-missing"],
        "regress/man/TH/noarg" => &[
            "man.title-missing",
            "man.title-section-missing",
            "man.title-date-missing",
        ],
        "regress/man/TH/onearg" | "regress/man/TH/twoargs" => {
            &["man.title-section-missing", "man.title-date-missing"]
        }
        "regress/man/TH/nobody" => &["man.no-document-body"],
        "regress/man/TH/sixargs" | "regress/man/PD/args" => &["man.excess-arguments"],
        "regress/man/nf/args" => &["man.all-arguments", "man.all-arguments"],
        "regress/man/EX/nested" | "regress/man/nf/dupe" => {
            &["man.redundant-fill-mode", "man.redundant-fill-mode"]
        }
        "regress/man/EX/spacing" => &[
            "man.redundant-fill-mode",
            "man.redundant-fill-mode",
            "man.redundant-fill-mode",
            "man.redundant-fill-mode",
        ],
        "regress/man/HP/break" => &["man.redundant-fill-mode"],
        "regress/man/IP/empty" => &["man.empty-paragraph", "man.empty-paragraph"],
        "regress/man/blank/afterSH" | "regress/man/blank/afterSS" => &[
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
        ],
        "regress/man/TP/double" => &[
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.empty-paragraph",
            "man.empty-paragraph",
        ],
        "regress/man/TS/break" => &[
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
        ],
        "regress/man/OP/args" => &["man.missing-option", "man.excess-arguments"],
        "regress/man/PP/args" => &[
            "man.all-arguments",
            "man.all-arguments",
            "man.all-arguments",
        ],
        "regress/man/PP/empty" => &[
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
        ],
        "regress/man/blank/line" => &[
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
            "man.empty-paragraph",
        ],
        "regress/man/RS/broken" | "regress/man/SH/empty_before" => &["man.empty-paragraph"],
        "regress/man/RS/empty" => &["man.empty-block"],
        "regress/man/MT/noME" | "regress/man/UR/noUE" => {
            &["man.unmatched-close", "man.unclosed-block"]
        }
        "regress/man/MT/args" | "regress/man/UR/args" => &[
            "man.excess-arguments",
            "man.excess-arguments",
            "man.empty-block",
            "man.missing-resource",
            "man.empty-block",
        ],
        "regress/man/RS/REarg" => &[
            "man.excess-arguments",
            "man.excess-arguments",
            "man.excess-arguments",
            "man.excess-arguments",
            "man.excess-arguments",
            "man.excess-arguments",
            "man.fewer-indents",
        ],
        "regress/man/RS/noRE" => &["man.unclosed-block"],
        "regress/man/RS/lonelyRE" => &[
            "man.unmatched-close",
            "man.unmatched-close",
            "man.unmatched-close",
        ],
        "regress/man/SH/broken_eline" | "regress/man/SS/broken_eline" => {
            &["man.line-scope-broken", "man.line-scope-broken"]
        }
        "regress/man/SH/noarg" | "regress/man/SS/noarg" => &[
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.unmatched-close",
            "man.line-scope-broken",
            "man.line-scope-broken",
            "man.unmatched-close",
            "man.blank-line-scope",
            "man.redundant-fill-mode",
        ],
        "regress/man/TP/fill" => &["input.line-too-long"],
        _ => &[],
    }
}

/// Parse every checksum-pinned stable mdoc(7) regression input with the
/// native backend.
///
/// This is deliberately a finite-parser and scanner-recovery gate, not an AST
/// parity claim. It fixes the 276-input corpus identity and the 53 reviewed
/// scanner- or scope-recovery diagnostic sequences while M5 grows macro validation.
/// Canonical legacy AST/IR and renderer comparison remain M7/M8/M9 work.
///
/// # Errors
///
/// Returns [`M5MdocSmokeGateError`] when archive identity, input identity,
/// parser completion, truncation state, or the reviewed diagnostic set differs.
pub fn run_m5_mdoc_smoke_gate(
    archive_path: impl AsRef<Path>,
) -> Result<M5MdocSmokeResult, M5MdocSmokeGateError> {
    run_m5_mdoc_smoke_shard(archive_path, 0, 1)
}

/// Run one deterministic zero-based partition of the M5 mdoc smoke gate.
///
/// Each shard independently verifies the pinned archive and the complete mdoc
/// inventory before selecting `case_index % shard_count == shard_index`.
/// Therefore independently launched shards can be summed without changing the
/// semantic gate or depending on filesystem enumeration order.
///
/// # Errors
///
/// Returns [`M5MdocSmokeGateError::InvalidShard`] for an invalid partition and
/// otherwise reports the same archive, parser, or recovery mismatch as the
/// complete gate.
pub fn run_m5_mdoc_smoke_shard(
    archive_path: impl AsRef<Path>,
    shard_index: usize,
    shard_count: usize,
) -> Result<M5MdocSmokeResult, M5MdocSmokeGateError> {
    if shard_count == 0 || shard_index >= shard_count {
        return Err(M5MdocSmokeGateError::InvalidShard {
            shard_index,
            shard_count,
        });
    }
    let archive_path = archive_path.as_ref();
    let inventory = stable_1_14_6_inventory(archive_path).map_err(M5MdocSmokeGateError::Corpus)?;
    let cases = inventory
        .cases
        .into_iter()
        .filter(|case| case.id.starts_with("regress/mdoc/"))
        .collect::<Vec<_>>();
    if cases.len() != M5_STABLE_MDOC_CASE_COUNT {
        return Err(M5MdocSmokeGateError::CaseCount {
            actual: cases.len(),
        });
    }

    let backend = MantdocBackend::default();
    let mut case_count = 0;
    let mut diagnostic_case_count = 0;
    let selected_cases = cases
        .into_iter()
        .enumerate()
        .filter_map(|(case_index, case)| (case_index % shard_count == shard_index).then_some(case));
    for case in selected_cases {
        case_count += 1;
        let input = stable_1_14_6_case_input(archive_path, &case.id, backend.parser_config())
            .map_err(M5MdocSmokeGateError::Corpus)?;
        let run = run_case(&backend, &input).map_err(M5MdocSmokeGateError::CaseValidation)?;
        let report = run.outcome.map_err(|error| M5MdocSmokeGateError::Fatal {
            case_id: case.id.clone(),
            error,
        })?;
        if report.statistics.truncated {
            return Err(M5MdocSmokeGateError::Truncated { case_id: case.id });
        }
        let expected = m5_expected_diagnostic_codes(&case.id);
        let actual = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(M5MdocSmokeGateError::Diagnostics {
                case_id: case.id,
                expected: expected.join(",").into(),
                actual: actual.join(",").into(),
            });
        }
        diagnostic_case_count += usize::from(!actual.is_empty());
    }

    Ok(M5MdocSmokeResult {
        case_count,
        diagnostic_case_count,
    })
}

/// Ordered legacy sequence shared by the full punctuation matrices for the
/// variadic tag-style `Em`, `Li`, and `Sy` macros.
const MDOC_TAG_PUNCT_DIAGNOSTICS: &[&str] = &[
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.empty-macro",
    "mdoc.trailing-delimiter-spacing",
];

#[allow(clippy::too_many_lines)] // The explicit source-order fixture ledger is easier to audit as one table.
#[allow(clippy::match_same_arms)] // One arm per upstream case keeps the compatibility ledger reviewable.
fn m5_expected_diagnostic_codes(case_id: &str) -> &'static [&'static str] {
    match case_id {
        "regress/mdoc/Ad/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/An/break" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.duplicate-argument",
            "mdoc.arguments",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.duplicate-argument",
            "mdoc.arguments",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
        ],
        "regress/mdoc/At/invalid" => &["mdoc.unknown-at-version"],
        "regress/mdoc/Aq/empty"
        | "regress/mdoc/Ar/punct"
        | "regress/mdoc/Brq/empty"
        | "regress/mdoc/Op/punct"
        | "regress/mdoc/Qq/empty"
        | "regress/mdoc/Sq/empty" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Bk/synopsis" => &["mdoc.trailing-delimiter-spacing", "mdoc.empty-block"],
        "regress/mdoc/Bk/broken" => &["mdoc.broken-block", "mdoc.empty-block"],
        "regress/mdoc/Bk/badarg" => &[
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.empty-block",
        ],
        "regress/mdoc/Bk/lines" => &["mdoc.arguments"],
        "regress/mdoc/Bl/column" => &[
            "mdoc.empty-macro",
            "mdoc.arguments",
            "mdoc.empty-macro",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.empty-macro",
            "mdoc.arguments",
        ],
        "regress/mdoc/Bl/column_nogroff" => &["mdoc.arguments", "mdoc.arguments"],
        "regress/mdoc/Bl/diag" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.arguments",
        ],
        "regress/mdoc/Bl/empty" => &[
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
        ],
        "regress/mdoc/Bl/emptyhead" => &[
            "mdoc.empty-list-item",
            "mdoc.empty-list-item",
            "mdoc.empty-list-item",
            "mdoc.empty-list-item",
        ],
        "regress/mdoc/Bl/emptyitem" => &[
            "mdoc.arguments",
            "mdoc.empty-list-item",
            "mdoc.empty-list-item",
            "mdoc.arguments",
            "mdoc.empty-list-item",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.empty-list-item",
        ],
        "regress/mdoc/Bl/emptytag" => &["mdoc.empty-list-item"],
        "regress/mdoc/Bl/item" => &["mdoc.arguments", "mdoc.arguments"],
        "regress/mdoc/Bl/noIt" => &[
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.empty-block",
        ],
        "regress/mdoc/Bl/tag" => &["mdoc.arguments"],
        "regress/mdoc/Bx/args" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/blank/line" => &[
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
        ],
        "regress/mdoc/blank/list" => &[
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-moved-out-of-list",
            "mdoc.paragraph-moved-out-of-list",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-moved-out-of-list",
            "mdoc.paragraph-moved-out-of-list",
            "mdoc.paragraph-before-block",
        ],
        "regress/mdoc/blank/transp" => &[
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "input.blank-line-in-filled-text",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
        ],
        "regress/mdoc/Db/args" => &["mdoc.obsolete", "mdoc.obsolete", "mdoc.obsolete"],
        "regress/mdoc/Cd/noarg" => &["mdoc.empty-macro"],
        "regress/mdoc/Cd/punct" => &["mdoc.empty-macro", "mdoc.empty-macro"],
        "regress/mdoc/Cm/noarg" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.trailing-delimiter-spacing",
        ],
        "regress/mdoc/Cm/punct" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.title-not-uppercase",
        ],
        "regress/mdoc/Dd/badarg" | "regress/mdoc/Dd/long" => &["mdoc.date-unparseable"],
        "regress/mdoc/Dd/manarg" => &["mdoc.date-legacy"],
        "regress/mdoc/Dd/noarg" => &["mdoc.date-missing"],
        "regress/mdoc/Dd/order" => &["mdoc.prologue-order"],
        "regress/mdoc/Dd/late" => &["mdoc.late-prologue"],
        "regress/mdoc/Dd/dupe" => &["mdoc.duplicate-prologue", "mdoc.duplicate-prologue"],
        "regress/mdoc/Dt/dupe" => &["mdoc.duplicate-prologue", "mdoc.late-title"],
        "regress/mdoc/Dt/case" => &["mdoc.title-not-uppercase"],
        "regress/mdoc/Dt/badsec" => &["mdoc.title-section-unknown"],
        "regress/mdoc/Dt/fourargs" => &["mdoc.arguments"],
        "regress/mdoc/Dt/late" => &["mdoc.late-title", "mdoc.title-missing"],
        "regress/mdoc/Dt/missing" => &["mdoc.title-missing"],
        "regress/mdoc/Dt/noarg" => &["mdoc.title-missing", "mdoc.title-section-missing"],
        "regress/mdoc/Dt/nosec" => &["mdoc.title-section-missing"],
        "regress/mdoc/Dt/order" => &["mdoc.prologue-order"],
        "regress/mdoc/Dt/nobody" => &["mdoc.no-document-body"],
        "regress/mdoc/blank/comment" => &["input.bad-comment-style"],
        "regress/mdoc/Dv/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Em/noarg" => &["mdoc.empty-macro"],
        "regress/mdoc/Em/punct" | "regress/mdoc/Li/punct" | "regress/mdoc/Sy/punct" => {
            MDOC_TAG_PUNCT_DIAGNOSTICS
        }
        "regress/mdoc/No/punct" => MDOC_TAG_PUNCT_DIAGNOSTICS,
        "regress/mdoc/Ns/position" => &["mdoc.no-space-macro", "mdoc.no-space-macro"],
        "regress/mdoc/Sm/badarg" => &["mdoc.boolean-argument", "mdoc.boolean-argument"],
        "regress/mdoc/Sm/twoarg" => &["mdoc.boolean-argument"],
        "regress/mdoc/St/badargs" => &["mdoc.empty-macro", "mdoc.unknown-standard"],
        "regress/mdoc/St/call" => &["mdoc.empty-macro"],
        "regress/mdoc/Sx/noarg" => &["mdoc.empty-macro"],
        "regress/mdoc/Va/punct" => &["mdoc.empty-macro", "mdoc.empty-macro"],
        "regress/mdoc/Va/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Vt/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Xr/args" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.reference-section-missing",
            "mdoc.reference-section-missing",
            "mdoc.trailing-delimiter-spacing",
        ],
        "regress/mdoc/Rs/allch" => &[
            "mdoc.reference-content",
            "mdoc.reference-content",
            "mdoc.reference-content",
            "mdoc.reference-content",
            "mdoc.reference-content",
            "mdoc.reference-content",
        ],
        "regress/mdoc/Rs/break" => &["mdoc.duplicate-section"],
        "regress/mdoc/Rs/empty" => &["mdoc.empty-reference-block", "mdoc.empty-reference-block"],
        "regress/mdoc/Rs/transp" => &[
            "mdoc.reference-content",
            "mdoc.reference-content",
            "mdoc.reference-content",
            "mdoc.reference-content",
        ],
        "regress/mdoc/Nd/noarg" => &["mdoc.description-missing"],
        "regress/mdoc/Lk/noarg" => &[
            "mdoc.empty-macro",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
        ],
        "regress/mdoc/Er/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Ev/font" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Er/tag" => &["mdoc.unexpected-section"],
        "regress/mdoc/Ev/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Ic/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Ms/noarg" => &["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Sy/noarg" => &["mdoc.empty-macro"],
        "regress/mdoc/Os/dupe" => &[
            "mdoc.operating-system-explicit",
            "mdoc.mdocdate-found",
            "mdoc.prologue-order",
            "mdoc.duplicate-prologue",
            "mdoc.operating-system-explicit",
            "mdoc.mdocdate-found",
            "mdoc.duplicate-prologue",
            "mdoc.operating-system-explicit",
            "mdoc.rcs-id-missing",
        ],
        "regress/mdoc/Os/late" => &["mdoc.late-operating-system"],
        "regress/mdoc/Os/long" => &["mdoc.operating-system-explicit"],
        "regress/mdoc/Os/missing" => &["mdoc.operating-system-missing"],
        "regress/mdoc/Pf/spacing" => &[
            "mdoc.prefix-without-following",
            "mdoc.prefix-without-following",
            "mdoc.prefix-without-following",
        ],
        "regress/mdoc/Pa/punct" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Tn/font" => &["mdoc.useless-macro"],
        "regress/mdoc/Tn/noarg" => &["mdoc.empty-macro", "mdoc.useless-macro"],
        "regress/mdoc/Tg/warn" => &[
            "mdoc.empty-macro",
            "mdoc.arguments",
            "mdoc.invalid-tag",
            "mdoc.invalid-tag",
            "mdoc.invalid-tag",
            "mdoc.empty-macro",
        ],
        "regress/mdoc/Ux/spacing" => &["mdoc.useless-macro", "mdoc.useless-macro"],
        "regress/mdoc/Ux/punct" => &[
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
        ],
        "regress/mdoc/Ud/arg" => &[
            "mdoc.useless-macro",
            "mdoc.useless-macro",
            "mdoc.useless-macro",
            "mdoc.arguments",
            "mdoc.useless-macro",
            "mdoc.arguments",
            "mdoc.useless-macro",
            "mdoc.arguments",
            "mdoc.useless-macro",
            "mdoc.arguments",
        ],
        "regress/mdoc/Ex/nostd" | "regress/mdoc/Rv/nostd" => &[
            "mdoc.standard-selector-missing",
            "mdoc.standard-selector-missing",
            "mdoc.standard-selector-missing",
        ],
        "regress/mdoc/Fd/empty" => &["mdoc.empty-macro", "mdoc.empty-macro"],
        "regress/mdoc/Fl/parsed" => &["mdoc.trailing-delimiter"],
        "regress/mdoc/Fl/punct" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Fl/spacing" => &["mdoc.obsolete", "mdoc.obsolete"],
        "regress/mdoc/Fo/noarg" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.arguments",
        ],
        "regress/mdoc/Fo/nohead" => &["mdoc.function-name-missing"],
        "regress/mdoc/Fo/obsolete" => &["mdoc.obsolete", "mdoc.obsolete"],
        "regress/mdoc/Fo/punct" => &[
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.trailing-delimiter-spacing",
        ],
        "regress/mdoc/Fo/section" => &["mdoc.authors-missing"],
        "regress/mdoc/Fo/warn" => &[
            "mdoc.function-name-parenthesis",
            "mdoc.function-argument-comma",
            "mdoc.function-name-parenthesis",
            "mdoc.function-name-parenthesis",
            "mdoc.function-name-parenthesis",
            "mdoc.function-name-parenthesis",
        ],
        "regress/mdoc/In/noarg" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.trailing-delimiter-spacing",
        ],
        "regress/mdoc/Lb/badargs" => &[
            "mdoc.empty-macro",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.unknown-library",
            "mdoc.trailing-delimiter-spacing",
            "mdoc.unknown-library",
        ],
        "regress/mdoc/Lb/break" => &["mdoc.unknown-library", "mdoc.unknown-library"],
        "regress/mdoc/Lb/eos" => &["mdoc.unknown-library"],
        "regress/mdoc/Mt/simple" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Nd/broken" => &[
            "mdoc.badly-nested-block",
            "mdoc.name-section-content",
            "mdoc.name-section-content",
            "mdoc.name-section-name-missing",
            "mdoc.name-section-description-missing",
            "mdoc.description-outside-name",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.description-outside-name",
        ],
        "regress/mdoc/Nd/par" => &[
            "mdoc.trailing-delimiter",
            "mdoc.description-outside-name",
            "mdoc.trailing-delimiter",
        ],
        "regress/mdoc/Ex/noname" => &["mdoc.name-missing", "mdoc.exit-name-missing"],
        "regress/mdoc/Ic/punct" => &[
            "mdoc.empty-macro",
            "mdoc.empty-macro",
            "mdoc.title-not-uppercase",
        ],
        "regress/mdoc/Nm/badNAME" => &["mdoc.name-missing", "mdoc.name-section-content"],
        "regress/mdoc/Nm/badNAMEuse" => &["mdoc.name-missing", "mdoc.name-section-content"],
        "regress/mdoc/Nm/broken" => &["mdoc.badly-nested-block", "mdoc.content-outside-list"],
        "regress/mdoc/Nm/punct" => &["mdoc.trailing-delimiter-spacing"],
        "regress/mdoc/Op/broken" => &["mdoc.badly-nested-block", "mdoc.badly-nested-block"],
        "regress/mdoc/Op/break" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
        ],
        "regress/mdoc/Nm/emptyNAME" => &["mdoc.name-missing"],
        "regress/mdoc/Rs/three_authors" => &["mdoc.authors-missing"],
        "regress/mdoc/Sh/badNAME" => &[
            "mdoc.name-section-content",
            "mdoc.name-section-description-missing",
        ],
        "regress/mdoc/Sh/emptyNAME" => &[
            "mdoc.name-section-name-missing",
            "mdoc.name-section-description-missing",
        ],
        "regress/mdoc/Sh/orderNAME" => &[
            "mdoc.name-section-description-not-last",
            "mdoc.name-section-name-missing",
        ],
        "regress/mdoc/Sh/punctNAME" => &[
            "mdoc.name-section-comma-missing",
            "mdoc.name-section-content",
            "mdoc.name-section-comma-missing",
            "mdoc.name-section-content",
        ],
        "regress/mdoc/break/twice" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.content-outside-list",
        ],
        "regress/mdoc/break/brokenbreaker" => &[
            "mdoc.badly-nested-block",
            "mdoc.unmatched-close",
            "mdoc.badly-nested-block",
        ],
        "regress/mdoc/break/tail" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
        ],
        "regress/mdoc/break/two" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
        ],
        "regress/mdoc/Nm/emptyNAMEuse" => &["mdoc.name-missing"],
        "regress/mdoc/Rv/noname" => &["mdoc.name-missing"],
        "regress/mdoc/Rs/args" => &["mdoc.arguments", "mdoc.arguments"],
        "regress/mdoc/Sh/empty" => &["mdoc.broken-block"],
        "regress/mdoc/Sh/first" => &["mdoc.first-section-not-name"],
        "regress/mdoc/Sh/nohead" => &["mdoc.empty-macro", "mdoc.empty-macro"],
        "regress/mdoc/Sh/parborder" => &[
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
        ],
        "regress/mdoc/Sh/order" => &[
            "mdoc.section-order",
            "mdoc.duplicate-section",
            "mdoc.unexpected-section",
        ],
        "regress/mdoc/Sh/tag" => &[
            "mdoc.duplicate-section",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
        ],
        "regress/mdoc/break/notopen" => &["mdoc.unmatched-close"],
        "regress/mdoc/Bl/inset" => &["mdoc.arguments"],
        "regress/mdoc/Bd/beforeNAME" | "regress/mdoc/Sh/before" | "regress/mdoc/Sh/subbefore" => {
            &["mdoc.content-before-section"]
        }
        "regress/mdoc/Bd/break"
        | "regress/mdoc/Bd/broken"
        | "regress/mdoc/Bf/break"
        | "regress/mdoc/Bf/broken" => &["mdoc.badly-nested-block"],
        "regress/mdoc/Bd/empty" => &[
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
            "mdoc.empty-block",
        ],
        "regress/mdoc/Bl/bareIt" => &[
            "mdoc.item-outside-list",
            "mdoc.item-outside-list",
            "mdoc.paragraph-before-block",
        ],
        "regress/mdoc/Bl/bareTa" => &[
            "mdoc.column-outside-list",
            "mdoc.column-outside-list",
            "mdoc.column-outside-list",
            "mdoc.item-outside-list",
            "mdoc.arguments",
        ],
        "regress/mdoc/Bl/break" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.item-outside-list",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.item-outside-list",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.broken-block",
            "mdoc.badly-nested-block",
            "mdoc.column-outside-list",
            "mdoc.badly-nested-block",
            "mdoc.broken-block",
            "mdoc.badly-nested-block",
            "mdoc.unclosed-block",
            "mdoc.unclosed-block",
            "mdoc.empty-list-item",
            "mdoc.arguments",
        ],
        "regress/mdoc/Bl/breakingIt" => &[
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.broken-block",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
            "mdoc.content-outside-list",
        ],
        "regress/mdoc/Bl/broken" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.broken-block",
        ],
        "regress/mdoc/Bl/unclosed" | "regress/mdoc/Nm/break" => &["mdoc.broken-block"],
        "regress/mdoc/Bd/centered" | "regress/mdoc/Sh/parbefore" => {
            &["mdoc.paragraph-before-block"]
        }
        "regress/mdoc/Bl/multitype" => &["mdoc.duplicate-argument"],
        "regress/mdoc/Bl/notype" | "regress/mdoc/Bf/multiargs" => &[
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
        ],
        "regress/mdoc/Bl/offset" => &["mdoc.empty-argument", "mdoc.empty-argument"],
        "regress/mdoc/Bl/badargs" => &[
            "mdoc.arguments",
            "mdoc.empty-argument",
            "mdoc.empty-argument",
            "mdoc.empty-argument",
            "mdoc.empty-argument",
            "mdoc.empty-argument",
            "mdoc.empty-argument",
            "mdoc.duplicate-argument",
            "mdoc.duplicate-argument",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.duplicate-argument",
            "mdoc.duplicate-argument",
            "mdoc.duplicate-argument",
        ],
        // The only recovery in `Eo/empty` is the final unmatched `.Ec`.
        // Keep it explicit: the preceding three `.Ec` calls are paired with
        // inline `Eo` scopes opened after `No`/`Ns`.
        "regress/mdoc/Eo/empty" => &["mdoc.unmatched-close"],
        "regress/mdoc/Eo/break" => &[
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
            "mdoc.badly-nested-block",
        ],
        "regress/mdoc/Eo/unclosed" | "regress/mdoc/Bd/unclosed" => &["mdoc.unclosed-block"],
        // `Es` and `En` are accepted legacy mdoc macros, but mandoc reports
        // each use as obsolete.  Preserve all six source-order warnings.
        "regress/mdoc/Eo/obsolete" => &[
            "mdoc.obsolete",
            "mdoc.obsolete",
            "mdoc.obsolete",
            "mdoc.obsolete",
            "mdoc.obsolete",
            "mdoc.obsolete",
        ],
        "regress/mdoc/Pp/arg" => &["mdoc.arguments", "mdoc.arguments", "mdoc.arguments"],
        "regress/mdoc/D1/spacing" | "regress/mdoc/Dl/spacing" => &["mdoc.empty-block"],
        "regress/mdoc/Bd/blank" => &[
            "input.trailing-whitespace",
            "input.trailing-whitespace",
            "input.trailing-whitespace",
            "input.trailing-whitespace",
            "input.trailing-whitespace",
            "input.trailing-whitespace",
            "input.trailing-whitespace",
            "mdoc.empty-block",
        ],
        "regress/mdoc/Bd/badargs" => &[
            "mdoc.arguments",
            "mdoc.arguments",
            "mdoc.empty-argument",
            "mdoc.duplicate-argument",
            "mdoc.duplicate-argument",
            "mdoc.missing-display-type",
            "mdoc.duplicate-display-type",
            "mdoc.duplicate-display-type",
            "mdoc.unsupported-display-file",
            "mdoc.unsupported-display-file",
            "mdoc.unsupported-display-file",
            "mdoc.display-without-arguments",
        ],
        "regress/mdoc/Bd/offset-empty" => &["mdoc.empty-argument"],
        "regress/mdoc/Bd/nested" | "regress/mdoc/Bd/offset-neg" => {
            &["mdoc.nested-display", "mdoc.nested-display"]
        }
        "regress/mdoc/Bf/badargs" => &[
            "mdoc.arguments",
            "mdoc.missing-font-type",
            "mdoc.unknown-font-type",
        ],
        _ => &[],
    }
}

/// Parse every checksum-pinned stable tbl(7) and eqn(7) regression input with
/// the native backend.
///
/// This is a finite-preprocessor and scanner-recovery gate, not a full table
/// or equation parity claim. It locks the 58-input corpus identity and two
/// reviewed recovery sequences while M6 adds format, span, text-block, and
/// grammar semantics.
///
/// # Errors
///
/// Returns [`M6PreprocessSmokeGateError`] when archive identity, input
/// identity, parser completion, truncation state, or the reviewed diagnostic
/// set differs.
pub fn run_m6_preprocess_smoke_gate(
    archive_path: impl AsRef<Path>,
) -> Result<M6PreprocessSmokeResult, M6PreprocessSmokeGateError> {
    let archive_path = archive_path.as_ref();
    let inventory =
        stable_1_14_6_inventory(archive_path).map_err(M6PreprocessSmokeGateError::Corpus)?;
    let cases = inventory
        .cases
        .into_iter()
        .filter(|case| case.id.starts_with("regress/tbl/") || case.id.starts_with("regress/eqn/"))
        .collect::<Vec<_>>();
    if cases.len() != M6_STABLE_PREPROCESS_CASE_COUNT {
        return Err(M6PreprocessSmokeGateError::CaseCount {
            actual: cases.len(),
        });
    }

    let backend = MantdocBackend::default();
    let mut diagnostic_case_count = 0;
    for case in cases {
        let input = stable_1_14_6_case_input(archive_path, &case.id, backend.parser_config())
            .map_err(M6PreprocessSmokeGateError::Corpus)?;
        let run = run_case(&backend, &input).map_err(M6PreprocessSmokeGateError::CaseValidation)?;
        let report = run
            .outcome
            .map_err(|error| M6PreprocessSmokeGateError::Fatal {
                case_id: case.id.clone(),
                error,
            })?;
        if report.statistics.truncated {
            return Err(M6PreprocessSmokeGateError::Truncated { case_id: case.id });
        }
        let expected = m6_expected_diagnostic_codes(&case.id);
        let actual = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(M6PreprocessSmokeGateError::Diagnostics {
                case_id: case.id,
                expected: expected.join(",").into(),
                actual: actual.join(",").into(),
            });
        }
        diagnostic_case_count += usize::from(!actual.is_empty());
    }

    Ok(M6PreprocessSmokeResult {
        case_count: M6_STABLE_PREPROCESS_CASE_COUNT,
        diagnostic_case_count,
    })
}

#[allow(clippy::match_same_arms)] // One arm per upstream case keeps the compatibility ledger reviewable.
fn m6_expected_diagnostic_codes(case_id: &str) -> &'static [&'static str] {
    match case_id {
        "regress/tbl/data/block_unclosed" => {
            &["tbl.unclosed-text-block", "tbl.unclosed-text-block"]
        }
        "regress/tbl/data/block_width" => &[
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
        ],
        "regress/tbl/layout/center" => &[
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
        ],
        "regress/tbl/layout/spacing" | "regress/tbl/layout/span" => &[
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
        ],
        "regress/tbl/mod/font" => &[
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
            "input.tab-in-filled-text",
        ],
        "regress/tbl/mod/badfont" => &["tbl.unknown-font", "tbl.unknown-font", "tbl.unknown-font"],
        "regress/tbl/mod/font-eol" => &["tbl.unknown-font"],
        "regress/tbl/layout/spacing-nogroff" => &["tbl.excessive-spacing"],
        "regress/tbl/layout/badspan" => &["tbl.leading-span"],
        "regress/tbl/layout/empty" => &["tbl.empty-layout", "tbl.empty-layout"],
        "regress/tbl/opt/invalid" => &[
            "tbl.option-argument",
            "tbl.option-argument-size",
            "tbl.option-character",
            "tbl.unknown-option",
            "tbl.eqn-delimiter-option",
        ],
        "regress/tbl/data/empty" => &["tbl.no-data"],
        "regress/tbl/data/insert" => &["tbl.spanned-data", "tbl.spanned-data"],
        "regress/tbl/layout/complex" => &[
            "tbl.vertical-bar",
            "tbl.vertical-bar",
            "tbl.spanned-data",
            "tbl.leading-down",
            "tbl.spanned-data",
        ],
        "regress/tbl/macro/man" => &["tbl.extra-data-cells", "tbl.macro"],
        "regress/tbl/macro/nested" => &["tbl.macro"],
        "regress/tbl/mod/width" => &["tbl.macro"],
        "regress/eqn/define/infinite" => &[
            "eqn.recursive-definition",
            "eqn.recursive-definition",
            "eqn.recursive-definition",
            "eqn.recursive-definition",
        ],
        "regress/eqn/define/invalid" => &[
            "eqn.empty-request",
            "eqn.empty-request",
            "eqn.empty-request",
            "eqn.empty-request",
            "eqn.empty-request",
        ],
        "regress/eqn/over/noarg" => &["eqn.missing-box"],
        _ => &[],
    }
}

fn m3_roff_backend() -> MantdocBackend {
    MantdocBackend::new(Parser::new(mantdoc::ParserConfig {
        syntax: mantdoc::Syntax::Roff,
        ..mantdoc::ParserConfig::default()
    }))
}

fn parse_m3_execution_manifest() -> Result<M3ExecutionManifest, M3ExecutionGateError> {
    let manifest: M3ExecutionManifest =
        toml::from_str(M3_EXECUTION_MANIFEST).map_err(|error| M3ExecutionGateError::Manifest {
            message: error.to_string().into(),
        })?;
    if manifest.schema != "mantdoc.m3-execution/v1" {
        return Err(manifest_error(format!(
            "schema must be mantdoc.m3-execution/v1, got {:?}",
            manifest.schema
        )));
    }
    if manifest.corpus_id != "mandoc-stable-1.14.6" {
        return Err(manifest_error(format!(
            "corpus_id must be mandoc-stable-1.14.6, got {:?}",
            manifest.corpus_id
        )));
    }
    if manifest.cases.is_empty() {
        return Err(manifest_error("at least one case is required"));
    }

    let mut case_ids = BTreeSet::new();
    for expected in &manifest.cases {
        if !case_ids.insert(&expected.id) {
            return Err(manifest_error(format!(
                "case id {:?} appears more than once",
                expected.id
            )));
        }
        if expected.id.is_empty() || !expected.id.starts_with("regress/roff/") {
            return Err(manifest_error(format!(
                "case id {:?} is not a stable roff regression path",
                expected.id
            )));
        }
        if !is_sha256(&expected.source_sha256) {
            return Err(manifest_error(format!(
                "case {:?} has invalid source_sha256",
                expected.id
            )));
        }
        for diagnostic in &expected.diagnostics {
            if diagnostic.code.is_empty() {
                return Err(manifest_error(format!(
                    "case {:?} has an empty diagnostic code",
                    expected.id
                )));
            }
            if diagnostic.end < diagnostic.start {
                return Err(manifest_error(format!(
                    "case {:?} has a reversed diagnostic span {}-{}",
                    expected.id, diagnostic.start, diagnostic.end
                )));
            }
        }
    }
    Ok(manifest)
}

fn assert_m3_execution_expectation(
    expected: &M3ExecutionExpectation,
    report: &ParseReport,
    actual: &M3ExecutionCaseResult,
) -> Result<(), M3ExecutionGateError> {
    if actual.ast_nodes != expected.ast_nodes {
        return Err(mismatch(
            &expected.id,
            "ast_nodes",
            expected.ast_nodes,
            actual.ast_nodes,
        ));
    }
    if actual.expansion_steps != expected.expansion_steps {
        return Err(mismatch(
            &expected.id,
            "expansion_steps",
            expected.expansion_steps,
            actual.expansion_steps,
        ));
    }
    if actual.truncated != expected.truncated {
        return Err(mismatch(
            &expected.id,
            "truncated",
            expected.truncated,
            actual.truncated,
        ));
    }

    let expected_diagnostics = format_expected_diagnostics(&expected.diagnostics);
    let actual_diagnostics = format_actual_diagnostics(&report.diagnostics);
    if expected_diagnostics != actual_diagnostics {
        return Err(mismatch(
            &expected.id,
            "diagnostics",
            expected_diagnostics,
            actual_diagnostics,
        ));
    }
    Ok(())
}

fn format_expected_diagnostics(diagnostics: &[M3ExecutionDiagnosticExpectation]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}@{}-{}",
                diagnostic.code, diagnostic.start, diagnostic.end
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn format_actual_diagnostics(diagnostics: &[mantdoc::Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| match &diagnostic.primary {
            Some(span) => format!("{}@{}-{}", diagnostic.code, span.start, span.end),
            None => format!("{}@none", diagnostic.code),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn mismatch(
    case_id: &str,
    field: &'static str,
    expected: impl fmt::Display,
    actual: impl fmt::Display,
) -> M3ExecutionGateError {
    M3ExecutionGateError::Mismatch {
        case_id: case_id.into(),
        field,
        expected: expected.to_string().into(),
        actual: actual.to_string().into(),
    }
}

fn manifest_error(message: impl Into<Box<str>>) -> M3ExecutionGateError {
    M3ExecutionGateError::Manifest {
        message: message.into(),
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| {
            byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte.is_ascii_hexdigit())
        })
}

fn validate_case(backend: &impl ParseBackend, case: &CaseInput) -> Result<(), CaseValidationError> {
    let actual_source_sha256 = sha256_hex(&case.bytes);
    if actual_source_sha256 != case.identity.decompressed_source_sha256.as_ref() {
        return Err(CaseValidationError::SourceSha256Mismatch {
            expected: case.identity.decompressed_source_sha256.clone(),
            actual: actual_source_sha256.into(),
        });
    }
    let actual_config_fingerprint = backend.parser_config_fingerprint();
    if actual_config_fingerprint != case.identity.parser_config_fingerprint.as_ref() {
        return Err(CaseValidationError::ParserConfigFingerprintMismatch {
            backend: backend.name(),
            expected: case.identity.parser_config_fingerprint.clone(),
            actual: actual_config_fingerprint.into(),
        });
    }
    Ok(())
}

fn case_input_from_payload(
    corpus_id: &str,
    payload: CorpusCasePayload,
    config: &mantdoc::ParserConfig,
) -> CaseInput {
    let source_name = SourceName::new(&payload.case.input_archive_path)
        .expect("checksum-verified archive paths are non-empty and NUL-free");
    CaseInput {
        identity: CaseIdentity {
            corpus_id: corpus_id.into(),
            case_id: payload.case.id,
            logical_root: source_name.as_str().into(),
            decompressed_source_sha256: payload.case.source_sha256,
            parser_config_fingerprint: parser_config_fingerprint(config),
            source_graph_hash: None,
        },
        source_name,
        bytes: payload.source_bytes,
    }
}

fn parser_config_fingerprint(config: &mantdoc::ParserConfig) -> Box<str> {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "schema", "mantdoc.parser-config/v1");
    hash_field(
        &mut hasher,
        "syntax",
        match config.syntax {
            mantdoc::Syntax::Auto => "auto",
            mantdoc::Syntax::Roff => "roff",
            mantdoc::Syntax::Man => "man",
            mantdoc::Syntax::Mdoc => "mdoc",
        },
    );
    hash_option(
        &mut hasher,
        "operating_system",
        config.operating_system.as_deref(),
    );
    hash_field(
        &mut hasher,
        "recovery",
        match config.recovery {
            mantdoc::RecoveryMode::BestEffort => "best-effort",
            mantdoc::RecoveryMode::Strict => "strict",
        },
    );
    for (name, value) in limit_fields(&config.limits) {
        hash_field(&mut hasher, name, &value.to_string());
    }
    sha256_hex(&hasher.finalize()).into()
}

fn limit_fields(limits: &mantdoc::Limits) -> [(&'static str, usize); 31] {
    [
        ("max_root_source_bytes", limits.max_root_source_bytes),
        ("max_total_source_bytes", limits.max_total_source_bytes),
        ("max_sources", limits.max_sources),
        ("max_source_lines", limits.max_source_lines),
        ("max_include_depth", limits.max_include_depth),
        ("max_line_bytes", limits.max_line_bytes),
        ("max_expanded_line_bytes", limits.max_expanded_line_bytes),
        ("max_line_expansion_steps", limits.max_line_expansion_steps),
        ("max_expansion_steps", limits.max_expansion_steps),
        ("max_macro_depth", limits.max_macro_depth),
        ("max_arguments", limits.max_arguments),
        ("max_argument_bytes", limits.max_argument_bytes),
        ("max_loop_iterations", limits.max_loop_iterations),
        (
            "max_total_loop_iterations",
            limits.max_total_loop_iterations,
        ),
        ("max_definitions", limits.max_definitions),
        ("max_definition_bytes", limits.max_definition_bytes),
        ("max_nodes", limits.max_nodes),
        ("max_child_edges", limits.max_child_edges),
        ("max_text_bytes", limits.max_text_bytes),
        ("max_tree_depth", limits.max_tree_depth),
        ("max_table_rows", limits.max_table_rows),
        ("max_table_columns", limits.max_table_columns),
        ("max_table_cells", limits.max_table_cells),
        ("max_table_span", limits.max_table_span),
        ("max_table_text_bytes", limits.max_table_text_bytes),
        ("max_equation_tokens", limits.max_equation_tokens),
        ("max_equation_depth", limits.max_equation_depth),
        ("max_equation_definitions", limits.max_equation_definitions),
        (
            "max_equation_expansion_steps",
            limits.max_equation_expansion_steps,
        ),
        ("max_diagnostics", limits.max_diagnostics),
        ("max_render_output_bytes", limits.max_render_output_bytes),
    ]
}

fn hash_option(hasher: &mut Sha256, key: &str, value: Option<&str>) {
    hash_field(
        hasher,
        &format!("{key}.present"),
        if value.is_some() { "true" } else { "false" },
    );
    if let Some(value) = value {
        hash_field(hasher, key, value);
    }
}

fn hash_field(hasher: &mut Sha256, key: &str, value: &str) {
    hasher.update((key.len() as u64).to_be_bytes());
    hasher.update(key.as_bytes());
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{
        CaseIdentity, CaseInput, CaseValidationError, CorpusCase, CorpusCasePayload,
        M3_EXECUTION_MANIFEST_PATH, M4_STABLE_MAN_CASE_COUNT, M5_STABLE_MDOC_CASE_COUNT,
        M6_STABLE_PREPROCESS_CASE_COUNT, MDOC_TAG_PUNCT_DIAGNOSTICS, MantdocBackend,
        case_input_from_payload, m4_expected_diagnostic_codes, m5_expected_diagnostic_codes,
        m6_expected_diagnostic_codes, parse_m3_execution_manifest, parser_config_fingerprint,
        run_case, sha256_hex,
    };

    #[test]
    fn harness_keeps_its_native_test_data_with_the_parser() {
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/conformance/data/m3-execution.toml")
                .is_file()
        );
    }

    #[test]
    fn m3_execution_gate_manifest_is_valid_and_has_the_reviewed_case_count() {
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/conformance/data/m3-execution.toml")
                .is_file()
        );
        assert_eq!(
            M3_EXECUTION_MANIFEST_PATH,
            "crates/mantdoc/tests/conformance/data/m3-execution.toml"
        );
        let manifest = parse_m3_execution_manifest().expect("checked-in M3 execution manifest");
        assert_eq!(manifest.cases.len(), 29);
        assert!(
            manifest
                .cases
                .iter()
                .any(|case| case.id == "regress/roff/cond/close" && case.truncated)
        );
        assert!(
            manifest
                .cases
                .iter()
                .any(|case| { case.id == "regress/roff/de/indir" && case.diagnostics.len() == 4 })
        );
    }

    #[test]
    fn m4_man_smoke_contract_names_every_reviewed_recovery_sequence() {
        assert_eq!(M4_STABLE_MAN_CASE_COUNT, 99);
        for case_id in [
            "regress/man/IP/longhead",
            "regress/man/SH/longarg",
            "regress/man/TP/longhead",
        ] {
            assert!(m4_expected_diagnostic_codes(case_id).is_empty());
        }
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/TH/case"),
            ["man.title-not-uppercase"]
        );
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/BI/emptyargs"),
            ["input.trailing-whitespace"]
        );
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/UR/noUE"),
            ["man.unmatched-close", "man.unclosed-block"]
        );
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/RS/lonelyRE"),
            [
                "man.unmatched-close",
                "man.unmatched-close",
                "man.unmatched-close",
            ]
        );
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/RS/REarg"),
            [
                "man.excess-arguments",
                "man.excess-arguments",
                "man.excess-arguments",
                "man.excess-arguments",
                "man.excess-arguments",
                "man.excess-arguments",
                "man.fewer-indents",
            ]
        );
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/RS/noRE"),
            ["man.unclosed-block"]
        );
        for case_id in ["regress/man/SH/noarg", "regress/man/SS/noarg"] {
            assert_eq!(
                m4_expected_diagnostic_codes(case_id),
                [
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.unmatched-close",
                    "man.line-scope-broken",
                    "man.line-scope-broken",
                    "man.unmatched-close",
                    "man.blank-line-scope",
                    "man.redundant-fill-mode",
                ]
            );
        }
        assert_eq!(
            m4_expected_diagnostic_codes("regress/man/PP/empty"),
            [
                "man.empty-paragraph",
                "man.empty-paragraph",
                "man.empty-paragraph",
                "man.empty-paragraph",
            ]
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // The audited diagnostic ledger is intentionally explicit.
    fn m5_mdoc_smoke_contract_names_every_reviewed_recovery_sequence() {
        assert_eq!(M5_STABLE_MDOC_CASE_COUNT, 276);
        for case_id in [
            "regress/mdoc/Op/arg",
            "regress/mdoc/Ns/arg",
            "regress/mdoc/Li/arg",
            "regress/mdoc/Fd/arg",
            "regress/mdoc/Bl/esc",
        ] {
            assert!(m5_expected_diagnostic_codes(case_id).is_empty());
        }
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/blank/comment"),
            ["input.bad-comment-style"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ad/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/At/invalid"),
            ["mdoc.unknown-at-version"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/blank/line"),
            [
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/blank/transp"),
            [
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "input.blank-line-in-filled-text",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/blank/list"),
            [
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-moved-out-of-list",
                "mdoc.paragraph-moved-out-of-list",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-moved-out-of-list",
                "mdoc.paragraph-moved-out-of-list",
                "mdoc.paragraph-before-block",
            ]
        );
        assert!(m5_expected_diagnostic_codes("regress/mdoc/Fo/break").is_empty());
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/empty"),
            [
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
                "mdoc.empty-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/emptyhead"),
            [
                "mdoc.empty-list-item",
                "mdoc.empty-list-item",
                "mdoc.empty-list-item",
                "mdoc.empty-list-item",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/emptyitem"),
            [
                "mdoc.arguments",
                "mdoc.empty-list-item",
                "mdoc.empty-list-item",
                "mdoc.arguments",
                "mdoc.empty-list-item",
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.empty-list-item",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/emptytag"),
            ["mdoc.empty-list-item"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/item"),
            ["mdoc.arguments", "mdoc.arguments"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/noIt"),
            [
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.empty-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bl/tag"),
            ["mdoc.arguments"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bx/args"),
            ["mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Db/args"),
            ["mdoc.obsolete", "mdoc.obsolete", "mdoc.obsolete"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Cd/noarg"),
            ["mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Cd/punct"),
            ["mdoc.empty-macro", "mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Cm/noarg"),
            [
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.trailing-delimiter-spacing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Cm/punct"),
            [
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.title-not-uppercase",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dd/badarg"),
            ["mdoc.date-unparseable"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dd/manarg"),
            ["mdoc.date-legacy"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dd/noarg"),
            ["mdoc.date-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dd/order"),
            ["mdoc.prologue-order"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dd/late"),
            ["mdoc.late-prologue"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/case"),
            ["mdoc.title-not-uppercase"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/badsec"),
            ["mdoc.title-section-unknown"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dd/dupe"),
            ["mdoc.duplicate-prologue", "mdoc.duplicate-prologue"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/dupe"),
            ["mdoc.duplicate-prologue", "mdoc.late-title"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/fourargs"),
            ["mdoc.arguments"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/late"),
            ["mdoc.late-title", "mdoc.title-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/missing"),
            ["mdoc.title-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/noarg"),
            ["mdoc.title-missing", "mdoc.title-section-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/nosec"),
            ["mdoc.title-section-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/order"),
            ["mdoc.prologue-order"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dt/nobody"),
            ["mdoc.no-document-body"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Os/dupe"),
            [
                "mdoc.operating-system-explicit",
                "mdoc.mdocdate-found",
                "mdoc.prologue-order",
                "mdoc.duplicate-prologue",
                "mdoc.operating-system-explicit",
                "mdoc.mdocdate-found",
                "mdoc.duplicate-prologue",
                "mdoc.operating-system-explicit",
                "mdoc.rcs-id-missing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dv/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Em/noarg"),
            ["mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Em/punct"),
            MDOC_TAG_PUNCT_DIAGNOSTICS
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Li/punct"),
            MDOC_TAG_PUNCT_DIAGNOSTICS
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/No/punct"),
            MDOC_TAG_PUNCT_DIAGNOSTICS
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ns/position"),
            ["mdoc.no-space-macro", "mdoc.no-space-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sm/badarg"),
            ["mdoc.boolean-argument", "mdoc.boolean-argument"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sm/twoarg"),
            ["mdoc.boolean-argument"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/St/badargs"),
            ["mdoc.empty-macro", "mdoc.unknown-standard"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/St/call"),
            ["mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sx/noarg"),
            ["mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Va/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Va/punct"),
            ["mdoc.empty-macro", "mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Vt/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Tg/warn"),
            [
                "mdoc.empty-macro",
                "mdoc.arguments",
                "mdoc.invalid-tag",
                "mdoc.invalid-tag",
                "mdoc.invalid-tag",
                "mdoc.empty-macro",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Xr/args"),
            [
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.trailing-delimiter-spacing",
                "mdoc.reference-section-missing",
                "mdoc.reference-section-missing",
                "mdoc.trailing-delimiter-spacing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ux/punct"),
            [
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rs/allch"),
            [
                "mdoc.reference-content",
                "mdoc.reference-content",
                "mdoc.reference-content",
                "mdoc.reference-content",
                "mdoc.reference-content",
                "mdoc.reference-content",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rs/break"),
            ["mdoc.duplicate-section"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rs/empty"),
            ["mdoc.empty-reference-block", "mdoc.empty-reference-block"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rs/transp"),
            [
                "mdoc.reference-content",
                "mdoc.reference-content",
                "mdoc.reference-content",
                "mdoc.reference-content",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Lk/noarg"),
            [
                "mdoc.empty-macro",
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nd/noarg"),
            ["mdoc.description-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Er/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ev/font"),
            ["mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ev/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ic/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ms/noarg"),
            ["mdoc.empty-macro", "mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sy/noarg"),
            ["mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sy/punct"),
            MDOC_TAG_PUNCT_DIAGNOSTICS
        );
        for case_id in ["regress/mdoc/Ex/nostd", "regress/mdoc/Rv/nostd"] {
            assert_eq!(
                m5_expected_diagnostic_codes(case_id),
                [
                    "mdoc.standard-selector-missing",
                    "mdoc.standard-selector-missing",
                    "mdoc.standard-selector-missing",
                ],
                "{case_id}"
            );
        }
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fd/empty"),
            ["mdoc.empty-macro", "mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fl/parsed"),
            ["mdoc.trailing-delimiter"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fl/punct"),
            ["mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fl/spacing"),
            ["mdoc.obsolete", "mdoc.obsolete"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fo/noarg"),
            [
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.arguments",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fo/nohead"),
            ["mdoc.function-name-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fo/obsolete"),
            ["mdoc.obsolete", "mdoc.obsolete"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fo/punct"),
            [
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
                "mdoc.trailing-delimiter-spacing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fo/section"),
            ["mdoc.authors-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Fo/warn"),
            [
                "mdoc.function-name-parenthesis",
                "mdoc.function-argument-comma",
                "mdoc.function-name-parenthesis",
                "mdoc.function-name-parenthesis",
                "mdoc.function-name-parenthesis",
                "mdoc.function-name-parenthesis",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/In/noarg"),
            [
                "mdoc.empty-macro",
                "mdoc.empty-macro",
                "mdoc.trailing-delimiter-spacing",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Mt/simple"),
            ["mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nd/broken"),
            [
                "mdoc.badly-nested-block",
                "mdoc.name-section-content",
                "mdoc.name-section-content",
                "mdoc.name-section-name-missing",
                "mdoc.name-section-description-missing",
                "mdoc.description-outside-name",
                "mdoc.content-outside-list",
                "mdoc.content-outside-list",
                "mdoc.description-outside-name",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nd/par"),
            [
                "mdoc.trailing-delimiter",
                "mdoc.description-outside-name",
                "mdoc.trailing-delimiter",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Ex/noname"),
            ["mdoc.name-missing", "mdoc.exit-name-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nm/badNAME"),
            ["mdoc.name-missing", "mdoc.name-section-content"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rs/three_authors"),
            ["mdoc.authors-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/badNAME"),
            [
                "mdoc.name-section-content",
                "mdoc.name-section-description-missing"
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/emptyNAME"),
            [
                "mdoc.name-section-name-missing",
                "mdoc.name-section-description-missing"
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/orderNAME"),
            [
                "mdoc.name-section-description-not-last",
                "mdoc.name-section-name-missing"
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/punctNAME"),
            [
                "mdoc.name-section-comma-missing",
                "mdoc.name-section-content",
                "mdoc.name-section-comma-missing",
                "mdoc.name-section-content"
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/break/twice"),
            [
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.content-outside-list",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/break/brokenbreaker"),
            [
                "mdoc.badly-nested-block",
                "mdoc.unmatched-close",
                "mdoc.badly-nested-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/break/tail"),
            [
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/break/two"),
            [
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nm/badNAMEuse"),
            ["mdoc.name-missing", "mdoc.name-section-content"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nm/broken"),
            ["mdoc.badly-nested-block", "mdoc.content-outside-list"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Op/broken"),
            ["mdoc.badly-nested-block", "mdoc.badly-nested-block"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Op/break"),
            [
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
                "mdoc.badly-nested-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nm/emptyNAME"),
            ["mdoc.name-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Nm/emptyNAMEuse"),
            ["mdoc.name-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rv/noname"),
            ["mdoc.name-missing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Rs/args"),
            ["mdoc.arguments", "mdoc.arguments"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/empty"),
            ["mdoc.broken-block"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/first"),
            ["mdoc.first-section-not-name"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/nohead"),
            ["mdoc.empty-macro", "mdoc.empty-macro"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Sh/parborder"),
            [
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
                "mdoc.paragraph-before-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/break/notopen"),
            ["mdoc.unmatched-close"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Aq/empty"),
            ["mdoc.trailing-delimiter-spacing"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bk/synopsis"),
            ["mdoc.trailing-delimiter-spacing", "mdoc.empty-block"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bk/broken"),
            ["mdoc.broken-block", "mdoc.empty-block"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bk/badarg"),
            [
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.empty-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bk/lines"),
            ["mdoc.arguments"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bd/badargs"),
            [
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.empty-argument",
                "mdoc.duplicate-argument",
                "mdoc.duplicate-argument",
                "mdoc.missing-display-type",
                "mdoc.duplicate-display-type",
                "mdoc.duplicate-display-type",
                "mdoc.unsupported-display-file",
                "mdoc.unsupported-display-file",
                "mdoc.unsupported-display-file",
                "mdoc.display-without-arguments",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bd/offset-empty"),
            ["mdoc.empty-argument"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bd/nested"),
            ["mdoc.nested-display", "mdoc.nested-display"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bf/badargs"),
            [
                "mdoc.arguments",
                "mdoc.missing-font-type",
                "mdoc.unknown-font-type",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bf/multiargs"),
            [
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.arguments",
                "mdoc.arguments",
            ]
        );
        assert!(m5_expected_diagnostic_codes("regress/mdoc/Eo/arg").is_empty());
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Eo/empty"),
            ["mdoc.unmatched-close"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Eo/unclosed"),
            ["mdoc.unclosed-block"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Eo/obsolete"),
            [
                "mdoc.obsolete",
                "mdoc.obsolete",
                "mdoc.obsolete",
                "mdoc.obsolete",
                "mdoc.obsolete",
                "mdoc.obsolete",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Pp/arg"),
            ["mdoc.arguments", "mdoc.arguments", "mdoc.arguments"]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Bd/blank"),
            [
                "input.trailing-whitespace",
                "input.trailing-whitespace",
                "input.trailing-whitespace",
                "input.trailing-whitespace",
                "input.trailing-whitespace",
                "input.trailing-whitespace",
                "input.trailing-whitespace",
                "mdoc.empty-block",
            ]
        );
        assert_eq!(
            m5_expected_diagnostic_codes("regress/mdoc/Dl/spacing"),
            ["mdoc.empty-block"]
        );
        assert!(m5_expected_diagnostic_codes("regress/mdoc/Bl/bullet").is_empty());
    }

    #[test]
    fn m6_preprocess_smoke_contract_names_the_reviewed_recovery_sequence() {
        assert_eq!(M6_STABLE_PREPROCESS_CASE_COUNT, 58);
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/layout/center"),
            [
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/layout/spacing"),
            [
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/layout/span"),
            [
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/mod/font"),
            [
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/data/block_width"),
            [
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
                "input.tab-in-filled-text",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/layout/empty"),
            ["tbl.empty-layout", "tbl.empty-layout"]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/layout/spacing-nogroff"),
            ["tbl.excessive-spacing"]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/macro/nested"),
            ["tbl.macro"]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/tbl/opt/invalid"),
            [
                "tbl.option-argument",
                "tbl.option-argument-size",
                "tbl.option-character",
                "tbl.unknown-option",
                "tbl.eqn-delimiter-option",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/eqn/define/infinite"),
            [
                "eqn.recursive-definition",
                "eqn.recursive-definition",
                "eqn.recursive-definition",
                "eqn.recursive-definition",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/eqn/define/invalid"),
            [
                "eqn.empty-request",
                "eqn.empty-request",
                "eqn.empty-request",
                "eqn.empty-request",
                "eqn.empty-request",
            ]
        );
        assert_eq!(
            m6_expected_diagnostic_codes("regress/eqn/over/noarg"),
            ["eqn.missing-box"]
        );
        assert!(m6_expected_diagnostic_codes("regress/eqn/empty").is_empty());
    }

    #[test]
    fn named_backend_runs_one_exact_byte_case() {
        let backend = MantdocBackend::default();
        let bytes = b".TH PLAIN 1\n".to_vec();
        let case = CaseInput {
            identity: CaseIdentity {
                corpus_id: "m1".into(),
                case_id: "plain-bytes".into(),
                logical_root: "".into(),
                decompressed_source_sha256: sha256_hex(&bytes).into(),
                parser_config_fingerprint: parser_config_fingerprint(backend.parser.config()),
                source_graph_hash: None,
            },
            source_name: mantdoc::SourceName::new("plain.1").unwrap(),
            bytes,
        };
        let run = run_case(&backend, &case).expect("identity must match selected backend");
        assert_eq!(run.backend, "mantdoc");
        let report = run
            .outcome
            .expect("M2 scanner accepts a plain control line");
        assert!(report.diagnostics.is_empty());
        // root + `.TH` element + its two argument text nodes
        assert_eq!(report.document.node_count(), 4);
    }

    #[test]
    fn changed_case_bytes_are_rejected_before_the_backend_runs() {
        let backend = MantdocBackend::default();
        let case = CaseInput {
            identity: CaseIdentity {
                corpus_id: "m1".into(),
                case_id: "mutated-bytes".into(),
                logical_root: "".into(),
                decompressed_source_sha256: "00".into(),
                parser_config_fingerprint: parser_config_fingerprint(backend.parser.config()),
                source_graph_hash: None,
            },
            source_name: mantdoc::SourceName::new("mutated.1").unwrap(),
            bytes: b".TH MUTATED 1\n".to_vec(),
        };
        assert!(matches!(
            run_case(&backend, &case),
            Err(CaseValidationError::SourceSha256Mismatch { .. })
        ));
    }

    #[test]
    fn changed_backend_configuration_is_rejected_before_the_backend_runs() {
        let backend = MantdocBackend::default();
        let bytes = b".TH CONFIG 1\n".to_vec();
        let case = CaseInput {
            identity: CaseIdentity {
                corpus_id: "m1".into(),
                case_id: "changed-config".into(),
                logical_root: "".into(),
                decompressed_source_sha256: sha256_hex(&bytes).into(),
                parser_config_fingerprint: "00".into(),
                source_graph_hash: None,
            },
            source_name: mantdoc::SourceName::new("config.1").unwrap(),
            bytes,
        };
        assert!(matches!(
            run_case(&backend, &case),
            Err(CaseValidationError::ParserConfigFingerprintMismatch { .. })
        ));
    }

    #[test]
    fn parser_configuration_fingerprint_encodes_options_and_all_limit_names() {
        let absent = mantdoc::ParserConfig::default();
        let mut present = absent.clone();
        present.operating_system = Some("<none>".into());
        assert_ne!(
            parser_config_fingerprint(&absent),
            parser_config_fingerprint(&present),
            "an option value must not collide with the absent option state"
        );

        let mut changed_limit = absent.clone();
        changed_limit.limits.max_equation_expansion_steps += 1;
        assert_ne!(
            parser_config_fingerprint(&absent),
            parser_config_fingerprint(&changed_limit),
            "less common resource limits are part of an exact case identity"
        );
    }

    #[test]
    fn verified_payload_becomes_a_case_input_that_a_named_backend_can_run() {
        let bytes = b".TH PAYLOAD 1\n".to_vec();
        let payload = CorpusCasePayload {
            case: CorpusCase {
                id: "regress/man/TH/payload".into(),
                input_archive_path: "regress/man/TH/payload.in".into(),
                source_sha256: sha256_hex(&bytes).into(),
                expected_outputs: Vec::new(),
            },
            source_bytes: bytes,
        };
        let backend = MantdocBackend::default();
        let case = case_input_from_payload("fixture", payload, backend.parser.config());
        assert_eq!(case.identity.corpus_id.as_ref(), "fixture");
        assert_eq!(case.identity.case_id.as_ref(), "regress/man/TH/payload");
        assert_eq!(case.source_name.as_str(), "regress/man/TH/payload.in");
        assert!(run_case(&backend, &case).is_ok());
    }
}
