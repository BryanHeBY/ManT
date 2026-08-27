//! Checksum-pinned, non-redistributed upstream regression inventories.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::{self, Cursor, Read},
    path::Path,
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;

const STABLE_1_14_6_ARCHIVE_SHA256: &str =
    "8bf0d570f01e70a6e124884088870cbed7537f36328d512909eb10cd53179d9c";
const STABLE_1_14_6_CASE_SET_SHA256: &str =
    "61739e43aa621dcfb57477da90d7b19cb5387bb5094e9c26249a7c35017da467";
const STABLE_1_14_6_ARCHIVE_ROOT: &str = "mandoc-1.14.6";
const STABLE_1_14_6_INPUT_COUNT: usize = 572;
const STABLE_1_14_6_EXPECTED_OUTPUT_COUNT: usize = 1_189;
const MAX_COMPRESSED_ARCHIVE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DECOMPRESSED_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TRACKED_FILE_BYTES: usize = 2 * 1024 * 1024;

/// One reference-renderer result paired with a corpus input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceOutput {
    /// Renderer or linter channel, for example `ascii` or `lint`.
    pub format: Box<str>,
    /// Archive-relative path of the exact expected bytes.
    pub archive_path: Box<str>,
    /// SHA-256 of the exact expected bytes.
    pub sha256: Box<str>,
}

/// One exact decompressed source and its available upstream oracle outputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusCase {
    /// Stable path-derived corpus-local identity without the `.in` suffix.
    pub id: Box<str>,
    /// Archive-relative path of the input bytes.
    pub input_archive_path: Box<str>,
    /// SHA-256 of the exact decompressed input bytes.
    pub source_sha256: Box<str>,
    /// Upstream renderer/linter outputs associated with this input.
    pub expected_outputs: Vec<ReferenceOutput>,
}

/// Exact source bytes selected from a checksum-verified corpus case.
///
/// The input is read only after the complete archive has passed its compressed
/// and canonical case-set checks. Expected output bytes remain out of memory;
/// callers use their checksummed paths to request a later renderer comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusCasePayload {
    /// Immutable identity and expected-output hash records for this source.
    pub case: CorpusCase,
    /// Exact unmodified decompressed parser input.
    pub source_bytes: Vec<u8>,
}

/// One exact upstream reference-renderer artifact selected from a verified case.
///
/// The bytes are retained only in the caller's returned value.  The
/// conformance package neither writes nor redistributes upstream output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceOutputPayload {
    /// Immutable identity and checksum record for the selected artifact.
    pub output: ReferenceOutput,
    /// Exact upstream output bytes whose digest matches [`Self::output`].
    pub output_bytes: Vec<u8>,
}

/// One exact source plus selected upstream renderer artifacts from a verified case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RendererCasePayload {
    /// Exact source input retained for each selected renderer comparison.
    pub source: CorpusCasePayload,
    /// Requested reference outputs in the stable archive order.
    pub outputs: Vec<ReferenceOutputPayload>,
}

/// Deterministic, in-memory inventory of a checksum-pinned corpus archive.
///
/// It contains paths and hashes, but never writes or redistributes the archive
/// payload. Later differential stages reopen the already verified archive and
/// select one input by [`CorpusCase::input_archive_path`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusInventory {
    /// Stable manifest corpus identifier.
    pub corpus_id: Box<str>,
    /// SHA-256 of the compressed archive that was inspected.
    pub archive_sha256: Box<str>,
    /// Cases in lexicographic archive-path order.
    pub cases: Vec<CorpusCase>,
    /// Number of expected renderer/linter artifacts across all cases.
    pub expected_output_count: usize,
    /// SHA-256 of the canonical sorted case-hash inventory.
    pub case_set_sha256: Box<str>,
}

/// Failure while reading or validating a corpus archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusArchiveError {
    /// Stable category appropriate for tests and automation.
    pub kind: CorpusArchiveErrorKind,
    /// Human explanation including the offending path when available.
    pub message: Box<str>,
}

/// Stable categories for corpus archive failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorpusArchiveErrorKind {
    /// The local archive could not be opened or read.
    Read,
    /// The compressed archive exceeded a fixed defensive byte limit.
    CompressedSizeLimit,
    /// The compressed archive did not match its declared SHA-256 identity.
    ArchiveSha256Mismatch,
    /// Canonical per-case hashes did not match the frozen case-set identity.
    CaseSetSha256Mismatch,
    /// The requested corpus-local case identifier was not present in the archive.
    CaseNotFound,
    /// A selected source member differed from its verified inventory hash.
    CaseSha256Mismatch,
    /// Gzip or tar decoding failed, including the decompressed-size limit.
    Decode,
    /// An archive member path or file type violates corpus policy.
    ArchiveLayout,
    /// A tracked regression file exceeded its per-file byte limit.
    FileSizeLimit,
    /// The inventory did not match the declared input or output count.
    CorpusCountMismatch,
    /// An expected output does not have exactly one matching input.
    OrphanedOutput,
    /// An expected output format is outside this lane's frozen contract.
    UnknownOutputFormat,
}

impl fmt::Display for CorpusArchiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CorpusArchiveError {}

/// Inspect the stable mandoc 1.14.6 regression archive after verifying it.
///
/// This is a read-only operation. It intentionally does not download, unpack,
/// or copy upstream files into the repository. It validates the compressed
/// archive identity first, then records the hashes of all 572 parser inputs and
/// their 1,189 declared renderer/linter outputs in deterministic path order.
///
/// # Errors
///
/// Returns a typed error when the archive is unavailable, malformed, too large,
/// has an unexpected layout, or differs from the M0 frozen corpus contract.
pub fn stable_1_14_6_inventory(
    archive_path: impl AsRef<Path>,
) -> Result<CorpusInventory, CorpusArchiveError> {
    let bytes = read_verified_stable_1_14_6_archive(archive_path.as_ref())?;
    let inventory = inventory_from_archive_bytes(
        &bytes,
        STABLE_1_14_6_ARCHIVE_ROOT,
        "mandoc-stable-1.14.6",
        STABLE_1_14_6_INPUT_COUNT,
        STABLE_1_14_6_EXPECTED_OUTPUT_COUNT,
        &["ascii", "markdown", "lint", "html", "tag", "utf8"],
    )?;
    if inventory.case_set_sha256.as_ref() != STABLE_1_14_6_CASE_SET_SHA256 {
        return Err(error(
            CorpusArchiveErrorKind::CaseSetSha256Mismatch,
            format!(
                "mandoc 1.14.6 case-set SHA-256 mismatch: expected {STABLE_1_14_6_CASE_SET_SHA256}, got {}",
                inventory.case_set_sha256
            ),
        ));
    }
    Ok(inventory)
}

/// Read one exact stable mandoc 1.14.6 regression input after full verification.
///
/// `case_id` is the lexicographic corpus-local identity such as
/// `regress/mdoc/Dd/basic`, without the `.in` suffix. The archive is first
/// inventoried in full; this second pass extracts only the selected input and
/// verifies its bytes against that inventory before returning them.
///
/// # Errors
///
/// Returns the same typed validation errors as [`stable_1_14_6_inventory`], a
/// `CaseNotFound` error for an unknown case, or `CaseSha256Mismatch` if a source
/// changes between inventory and selection.
pub fn stable_1_14_6_case(
    archive_path: impl AsRef<Path>,
    case_id: &str,
) -> Result<CorpusCasePayload, CorpusArchiveError> {
    let archive_path = archive_path.as_ref();
    let inventory = stable_1_14_6_inventory(archive_path)?;
    let case = inventory
        .cases
        .iter()
        .find(|case| case.id.as_ref() == case_id)
        .cloned()
        .ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::CaseNotFound,
                format!("mandoc 1.14.6 has no regression case {case_id:?}"),
            )
        })?;
    let archive_bytes = read_verified_stable_1_14_6_archive(archive_path)?;
    let source_bytes = read_member_from_archive_bytes(
        &archive_bytes,
        STABLE_1_14_6_ARCHIVE_ROOT,
        &case.input_archive_path,
    )?;
    let actual_sha256 = sha256_hex(&source_bytes);
    if actual_sha256 != case.source_sha256.as_ref() {
        return Err(error(
            CorpusArchiveErrorKind::CaseSha256Mismatch,
            format!(
                "selected source SHA-256 mismatch for {}: expected {}, got {actual_sha256}",
                case.input_archive_path, case.source_sha256
            ),
        ));
    }
    Ok(CorpusCasePayload { case, source_bytes })
}

/// Read one exact stable upstream renderer artifact after full archive verification.
///
/// `case_id` identifies the source without its `.in` suffix. `format` is one
/// of the declared upstream channels such as `ascii`, `utf8`, or `html`.
/// The selected artifact is verified against the checksum recorded in the
/// canonical inventory before it is returned.
///
/// # Errors
///
/// Returns a typed archive error when the corpus is unavailable, invalid, the
/// requested case/format is absent, or the selected output differs from its
/// checked-in upstream digest.
pub fn stable_1_14_6_reference_output(
    archive_path: impl AsRef<Path>,
    case_id: &str,
    format: &str,
) -> Result<ReferenceOutputPayload, CorpusArchiveError> {
    let archive_path = archive_path.as_ref();
    let inventory = stable_1_14_6_inventory(archive_path)?;
    let case = inventory
        .cases
        .iter()
        .find(|case| case.id.as_ref() == case_id)
        .ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::CaseNotFound,
                format!("mandoc 1.14.6 has no regression case {case_id:?}"),
            )
        })?;
    let output = case
        .expected_outputs
        .iter()
        .find(|output| output.format.as_ref() == format)
        .cloned()
        .ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::CaseNotFound,
                format!("mandoc 1.14.6 case {case_id:?} has no {format:?} reference output"),
            )
        })?;
    let archive_bytes = read_verified_stable_1_14_6_archive(archive_path)?;
    let output_bytes = read_member_from_archive_bytes(
        &archive_bytes,
        STABLE_1_14_6_ARCHIVE_ROOT,
        &output.archive_path,
    )?;
    let observed = sha256_hex(&output_bytes);
    if observed != output.sha256.as_ref() {
        return Err(error(
            CorpusArchiveErrorKind::CaseSha256Mismatch,
            format!(
                "mandoc 1.14.6 reference output {:?} checksum mismatch: expected {}, got {observed}",
                output.archive_path, output.sha256
            ),
        ));
    }
    Ok(ReferenceOutputPayload {
        output,
        output_bytes,
    })
}

/// Read one stable source and selected renderer artifacts with one archive pass.
///
/// The source and every requested output are selected only after the complete
/// archive inventory has passed validation. This is the efficient M9 path for
/// comparing multiple formats of one source: it avoids separately validating
/// and decompressing the same archive for each output.
///
/// # Errors
///
/// Returns a typed archive error if the corpus, case, or selected outputs are
/// unavailable or do not match their frozen digests.
pub fn stable_1_14_6_renderer_case(
    archive_path: impl AsRef<Path>,
    case_id: &str,
    formats: &[&str],
) -> Result<RendererCasePayload, CorpusArchiveError> {
    let archive_path = archive_path.as_ref();
    let inventory = stable_1_14_6_inventory(archive_path)?;
    let case = inventory
        .cases
        .iter()
        .find(|case| case.id.as_ref() == case_id)
        .cloned()
        .ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::CaseNotFound,
                format!("mandoc 1.14.6 has no regression case {case_id:?}"),
            )
        })?;
    let requested_formats = formats.iter().copied().collect::<BTreeSet<_>>();
    let outputs = case
        .expected_outputs
        .iter()
        .filter(|output| requested_formats.contains(output.format.as_ref()))
        .cloned()
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return Err(error(
            CorpusArchiveErrorKind::CaseNotFound,
            format!("mandoc 1.14.6 case {case_id:?} has no selected reference output"),
        ));
    }
    let mut requested_paths = Vec::with_capacity(outputs.len() + 1);
    requested_paths.push(case.input_archive_path.as_ref());
    requested_paths.extend(outputs.iter().map(|output| output.archive_path.as_ref()));
    let archive_bytes = read_verified_stable_1_14_6_archive(archive_path)?;
    let mut members = read_members_from_archive_bytes(
        &archive_bytes,
        STABLE_1_14_6_ARCHIVE_ROOT,
        &requested_paths,
    )?;
    let source_bytes = members
        .remove(case.input_archive_path.as_ref())
        .ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::CaseNotFound,
                format!(
                    "archive is missing selected member {:?}",
                    case.input_archive_path
                ),
            )
        })?;
    let observed_source = sha256_hex(&source_bytes);
    if observed_source != case.source_sha256.as_ref() {
        return Err(error(
            CorpusArchiveErrorKind::CaseSha256Mismatch,
            format!(
                "mandoc 1.14.6 case {case_id:?} checksum mismatch: expected {}, got {observed_source}",
                case.source_sha256
            ),
        ));
    }
    let outputs = outputs
        .into_iter()
        .map(|output| {
            let output_bytes = members.remove(output.archive_path.as_ref()).ok_or_else(|| {
                error(
                    CorpusArchiveErrorKind::CaseNotFound,
                    format!("archive is missing selected member {:?}", output.archive_path),
                )
            })?;
            let observed = sha256_hex(&output_bytes);
            if observed != output.sha256.as_ref() {
                return Err(error(
                    CorpusArchiveErrorKind::CaseSha256Mismatch,
                    format!(
                        "mandoc 1.14.6 reference output {:?} checksum mismatch: expected {}, got {observed}",
                        output.archive_path, output.sha256
                    ),
                ));
            }
            Ok(ReferenceOutputPayload {
                output,
                output_bytes,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RendererCasePayload {
        source: CorpusCasePayload { case, source_bytes },
        outputs,
    })
}

fn read_verified_stable_1_14_6_archive(path: &Path) -> Result<Vec<u8>, CorpusArchiveError> {
    let bytes = read_archive(path)?;
    let actual_sha256 = sha256_hex(&bytes);
    if actual_sha256 != STABLE_1_14_6_ARCHIVE_SHA256 {
        return Err(error(
            CorpusArchiveErrorKind::ArchiveSha256Mismatch,
            format!(
                "mandoc 1.14.6 archive SHA-256 mismatch: expected {STABLE_1_14_6_ARCHIVE_SHA256}, got {actual_sha256}"
            ),
        ));
    }
    Ok(bytes)
}

fn read_archive(path: &Path) -> Result<Vec<u8>, CorpusArchiveError> {
    let file = File::open(path).map_err(|issue| {
        error(
            CorpusArchiveErrorKind::Read,
            format!("cannot read corpus archive {}: {issue}", path.display()),
        )
    })?;
    let mut reader = file.take((MAX_COMPRESSED_ARCHIVE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|issue| {
        error(
            CorpusArchiveErrorKind::Read,
            format!("cannot read corpus archive {}: {issue}", path.display()),
        )
    })?;
    if bytes.len() > MAX_COMPRESSED_ARCHIVE_BYTES {
        return Err(error(
            CorpusArchiveErrorKind::CompressedSizeLimit,
            format!(
                "corpus archive {} exceeds the {MAX_COMPRESSED_ARCHIVE_BYTES}-byte compressed limit",
                path.display()
            ),
        ));
    }
    Ok(bytes)
}

fn inventory_from_archive_bytes(
    archive_bytes: &[u8],
    archive_root: &str,
    corpus_id: &str,
    expected_input_count: usize,
    expected_output_count: usize,
    allowed_formats: &[&str],
) -> Result<CorpusInventory, CorpusArchiveError> {
    let decoder = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(ByteLimitedReader::new(
        decoder,
        MAX_DECOMPRESSED_ARCHIVE_BYTES,
    ));
    let tracked_files = read_tracked_files(&mut archive, archive_root)?;
    let (cases, observed_output_count) = build_cases(&tracked_files, allowed_formats)?;

    if cases.len() != expected_input_count || observed_output_count != expected_output_count {
        return Err(error(
            CorpusArchiveErrorKind::CorpusCountMismatch,
            format!(
                "{corpus_id} inventory count mismatch: expected {expected_input_count} inputs and {expected_output_count} outputs, found {} inputs and {observed_output_count} outputs",
                cases.len()
            ),
        ));
    }
    Ok(CorpusInventory {
        corpus_id: corpus_id.into(),
        archive_sha256: sha256_hex(archive_bytes).into(),
        case_set_sha256: canonical_case_set_sha256(&cases).into(),
        cases,
        expected_output_count: observed_output_count,
    })
}

fn read_tracked_files<R: Read>(
    archive: &mut Archive<R>,
    archive_root: &str,
) -> Result<BTreeMap<Box<str>, Vec<u8>>, CorpusArchiveError> {
    let mut tracked_files = BTreeMap::<Box<str>, Vec<u8>>::new();
    for entry in archive.entries().map_err(decode_error)? {
        let mut entry = entry.map_err(decode_error)?;
        let path = entry.path().map_err(decode_error)?;
        let path = path.to_str().ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::ArchiveLayout,
                "archive has a non-UTF-8 member path",
            )
        })?;
        let relative = archive_relative_path(path, archive_root)?.to_owned();
        if !is_relevant_regression_file(&relative) {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(error(
                CorpusArchiveErrorKind::ArchiveLayout,
                format!("tracked regression member is not a regular file: {relative}"),
            ));
        }
        let declared_size = usize::try_from(entry.size()).map_err(|_| {
            error(
                CorpusArchiveErrorKind::FileSizeLimit,
                format!("tracked regression member has an unrepresentable size: {relative}"),
            )
        })?;
        if declared_size > MAX_TRACKED_FILE_BYTES {
            return Err(error(
                CorpusArchiveErrorKind::FileSizeLimit,
                format!("tracked regression member exceeds byte limit: {relative}"),
            ));
        }
        let mut bytes = Vec::with_capacity(declared_size);
        entry.read_to_end(&mut bytes).map_err(decode_error)?;
        if bytes.len() != declared_size {
            return Err(error(
                CorpusArchiveErrorKind::Decode,
                format!("tracked regression member changed size while reading: {relative}"),
            ));
        }
        if tracked_files
            .insert(relative.clone().into(), bytes)
            .is_some()
        {
            return Err(error(
                CorpusArchiveErrorKind::ArchiveLayout,
                format!("archive contains duplicate tracked regression member: {relative}"),
            ));
        }
    }
    Ok(tracked_files)
}

fn read_member_from_archive_bytes(
    archive_bytes: &[u8],
    archive_root: &str,
    requested_relative_path: &str,
) -> Result<Vec<u8>, CorpusArchiveError> {
    let decoder = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(ByteLimitedReader::new(
        decoder,
        MAX_DECOMPRESSED_ARCHIVE_BYTES,
    ));
    for entry in archive.entries().map_err(decode_error)? {
        let mut entry = entry.map_err(decode_error)?;
        let path = entry.path().map_err(decode_error)?;
        let path = path.to_str().ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::ArchiveLayout,
                "archive has a non-UTF-8 member path",
            )
        })?;
        let relative = archive_relative_path(path, archive_root)?.to_owned();
        if relative != requested_relative_path {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(error(
                CorpusArchiveErrorKind::ArchiveLayout,
                format!("selected member is not a regular file: {relative}"),
            ));
        }
        let declared_size = usize::try_from(entry.size()).map_err(|_| {
            error(
                CorpusArchiveErrorKind::FileSizeLimit,
                format!("selected member has an unrepresentable size: {relative}"),
            )
        })?;
        if declared_size > MAX_TRACKED_FILE_BYTES {
            return Err(error(
                CorpusArchiveErrorKind::FileSizeLimit,
                format!("selected member exceeds byte limit: {relative}"),
            ));
        }
        let mut bytes = Vec::with_capacity(declared_size);
        entry.read_to_end(&mut bytes).map_err(decode_error)?;
        if bytes.len() != declared_size {
            return Err(error(
                CorpusArchiveErrorKind::Decode,
                format!("selected member changed size while reading: {relative}"),
            ));
        }
        return Ok(bytes);
    }
    Err(error(
        CorpusArchiveErrorKind::CaseNotFound,
        format!("archive is missing selected member {requested_relative_path:?}"),
    ))
}

/// Select several already-validated members during one tar traversal.
fn read_members_from_archive_bytes(
    archive_bytes: &[u8],
    archive_root: &str,
    requested_relative_paths: &[&str],
) -> Result<BTreeMap<Box<str>, Vec<u8>>, CorpusArchiveError> {
    let requested = requested_relative_paths
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let decoder = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(ByteLimitedReader::new(
        decoder,
        MAX_DECOMPRESSED_ARCHIVE_BYTES,
    ));
    let mut selected = BTreeMap::new();
    for entry in archive.entries().map_err(decode_error)? {
        let mut entry = entry.map_err(decode_error)?;
        let path = entry.path().map_err(decode_error)?;
        let path = path.to_str().ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::ArchiveLayout,
                "archive has a non-UTF-8 member path",
            )
        })?;
        let relative = archive_relative_path(path, archive_root)?.to_owned();
        if !requested.contains(relative.as_str()) {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(error(
                CorpusArchiveErrorKind::ArchiveLayout,
                format!("selected member is not a regular file: {relative}"),
            ));
        }
        let declared_size = usize::try_from(entry.size()).map_err(|_| {
            error(
                CorpusArchiveErrorKind::FileSizeLimit,
                format!("selected member has an unrepresentable size: {relative}"),
            )
        })?;
        if declared_size > MAX_TRACKED_FILE_BYTES {
            return Err(error(
                CorpusArchiveErrorKind::FileSizeLimit,
                format!("selected member exceeds byte limit: {relative}"),
            ));
        }
        let mut bytes = Vec::with_capacity(declared_size);
        entry.read_to_end(&mut bytes).map_err(decode_error)?;
        if bytes.len() != declared_size {
            return Err(error(
                CorpusArchiveErrorKind::Decode,
                format!("tracked regression member changed size while reading: {relative}"),
            ));
        }
        if selected.insert(relative.clone().into(), bytes).is_some() {
            return Err(error(
                CorpusArchiveErrorKind::ArchiveLayout,
                format!("archive contains duplicate selected regression member: {relative}"),
            ));
        }
    }
    for requested in requested {
        if !selected.contains_key(requested) {
            return Err(error(
                CorpusArchiveErrorKind::CaseNotFound,
                format!("archive is missing selected member {requested:?}"),
            ));
        }
    }
    Ok(selected)
}

fn build_cases(
    tracked_files: &BTreeMap<Box<str>, Vec<u8>>,
    allowed_formats: &[&str],
) -> Result<(Vec<CorpusCase>, usize), CorpusArchiveError> {
    let mut cases = Vec::new();
    let mut observed_output_count = 0;
    for (input_path, input_bytes) in tracked_files {
        if !input_path.ends_with(".in") {
            continue;
        }
        let id = input_path.trim_end_matches(".in");
        let output_prefix = format!("{id}.out_");
        let mut expected_outputs = Vec::new();
        for (output_path, output_bytes) in tracked_files.iter().filter(|(path, _)| {
            path.strip_prefix(&output_prefix)
                .is_some_and(|format| !format.contains('/'))
        }) {
            let format = output_path
                .strip_prefix(&output_prefix)
                .expect("filter retains paths with the requested output prefix");
            if format.contains('/') {
                break;
            }
            if !allowed_formats.contains(&format) {
                return Err(error(
                    CorpusArchiveErrorKind::UnknownOutputFormat,
                    format!("unexpected upstream output format {format:?} for {id}"),
                ));
            }
            expected_outputs.push(ReferenceOutput {
                format: format.into(),
                archive_path: output_path.clone(),
                sha256: sha256_hex(output_bytes).into(),
            });
        }
        observed_output_count += expected_outputs.len();
        cases.push(CorpusCase {
            id: id.into(),
            input_archive_path: input_path.clone(),
            source_sha256: sha256_hex(input_bytes).into(),
            expected_outputs,
        });
    }

    for output_path in tracked_files.keys().filter(|path| path.contains(".out_")) {
        let (stem, _) = output_path.rsplit_once(".out_").ok_or_else(|| {
            error(
                CorpusArchiveErrorKind::ArchiveLayout,
                format!("invalid expected output path: {output_path}"),
            )
        })?;
        let input_path = format!("{stem}.in");
        if !tracked_files.contains_key(input_path.as_str()) {
            return Err(error(
                CorpusArchiveErrorKind::OrphanedOutput,
                format!("upstream output has no matching input: {output_path}"),
            ));
        }
    }
    Ok((cases, observed_output_count))
}

fn archive_relative_path<'a>(
    path: &'a str,
    archive_root: &str,
) -> Result<&'a str, CorpusArchiveError> {
    if path == archive_root {
        return Ok("");
    }
    let Some(relative) = path
        .strip_prefix(archive_root)
        .and_then(|path| path.strip_prefix('/'))
    else {
        return Err(error(
            CorpusArchiveErrorKind::ArchiveLayout,
            format!("archive member is outside expected root {archive_root:?}: {path}"),
        ));
    };
    let safe = !relative.is_empty()
        && !relative.starts_with('/')
        && !relative.contains('\\')
        && relative
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..");
    safe.then_some(relative).ok_or_else(|| {
        error(
            CorpusArchiveErrorKind::ArchiveLayout,
            format!("archive member has unsafe relative path: {path}"),
        )
    })
}

fn is_relevant_regression_file(path: &str) -> bool {
    const LANES: [&str; 6] = ["mdoc", "roff", "man", "tbl", "char", "eqn"];
    let Some((_, rest)) = path.split_once("regress/") else {
        return false;
    };
    let Some((lane, _)) = rest.split_once('/') else {
        return false;
    };
    LANES.contains(&lane)
        && (Path::new(path)
            .extension()
            .is_some_and(|extension| extension == "in")
            || path.contains(".out_"))
}

fn canonical_case_set_sha256(cases: &[CorpusCase]) -> String {
    let mut hasher = Sha256::new();
    for case in cases {
        hash_field(&mut hasher, "case", &case.id);
        hash_field(&mut hasher, "input", &case.input_archive_path);
        hash_field(&mut hasher, "source", &case.source_sha256);
        for output in &case.expected_outputs {
            hash_field(&mut hasher, "format", &output.format);
            hash_field(&mut hasher, "output", &output.archive_path);
            hash_field(&mut hasher, "output-sha256", &output.sha256);
        }
    }
    hex_encode(&hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, key: &str, value: &str) {
    hasher.update((key.len() as u64).to_be_bytes());
    hasher.update(key.as_bytes());
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

fn decode_error(issue: impl fmt::Display) -> CorpusArchiveError {
    error(
        CorpusArchiveErrorKind::Decode,
        format!("cannot decode checksum-verified corpus archive: {issue}"),
    )
}

fn error(kind: CorpusArchiveErrorKind, message: impl Into<Box<str>>) -> CorpusArchiveError {
    CorpusArchiveError {
        kind,
        message: message.into(),
    }
}

struct ByteLimitedReader<R> {
    reader: R,
    remaining: usize,
}

impl<R> ByteLimitedReader<R> {
    const fn new(reader: R, limit: usize) -> Self {
        Self {
            reader,
            remaining: limit,
        }
    }
}

impl<R: Read> Read for ByteLimitedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decompressed corpus archive exceeds the configured byte limit",
            ));
        }
        let readable = buffer.len().min(self.remaining);
        let read = self.reader.read(&mut buffer[..readable])?;
        self.remaining -= read;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use flate2::{Compression, write::GzEncoder};
    use tar::{Builder, Header};

    use super::{
        CorpusArchiveErrorKind, canonical_case_set_sha256, inventory_from_archive_bytes,
        read_member_from_archive_bytes, read_members_from_archive_bytes, sha256_hex,
    };

    #[test]
    fn inventory_is_sorted_and_pairs_each_output_with_its_input() {
        let archive = test_archive(&[
            ("fixture/regress/man/z.in", b".TH Z 1\n"),
            ("fixture/regress/man/z.out_ascii", b"Z\n"),
            ("fixture/regress/man/a.in", b".TH A 1\n"),
            ("fixture/regress/man/a.out_lint", b""),
        ]);
        let inventory =
            inventory_from_archive_bytes(&archive, "fixture", "fixture", 2, 2, &["ascii", "lint"])
                .expect("valid tiny corpus");
        assert_eq!(inventory.cases[0].id.as_ref(), "regress/man/a");
        assert_eq!(
            inventory.cases[1].expected_outputs[0].format.as_ref(),
            "ascii"
        );
        assert_eq!(
            inventory.case_set_sha256.as_ref(),
            canonical_case_set_sha256(&inventory.cases)
        );
        assert_eq!(inventory.archive_sha256.as_ref(), sha256_hex(&archive));
    }

    #[test]
    fn orphaned_output_is_rejected() {
        let archive = test_archive(&[("fixture/regress/roff/orphan.out_ascii", b"orphan\n")]);
        let error = inventory_from_archive_bytes(&archive, "fixture", "fixture", 0, 0, &["ascii"])
            .expect_err("output without input is invalid");
        assert_eq!(error.kind, CorpusArchiveErrorKind::OrphanedOutput);
    }

    #[test]
    fn selected_member_is_exact_and_never_uses_a_host_path() {
        let archive = test_archive(&[("fixture/regress/man/basic.in", b".TH BASIC 1\n")]);
        let source = read_member_from_archive_bytes(&archive, "fixture", "regress/man/basic.in")
            .expect("member in checksum-verified fixture archive");
        assert_eq!(source, b".TH BASIC 1\n");
        let error = read_member_from_archive_bytes(&archive, "fixture", "regress/man/missing.in")
            .expect_err("missing case cannot fall back to the filesystem");
        assert_eq!(error.kind, CorpusArchiveErrorKind::CaseNotFound);
    }

    #[test]
    fn selected_members_share_one_verified_tar_pass() {
        let archive = test_archive(&[
            ("fixture/regress/man/basic.in", b".TH BASIC 1\n"),
            ("fixture/regress/man/basic.out_ascii", b"BASIC\n"),
            ("fixture/regress/man/basic.out_html", b"BASIC\n"),
        ]);
        let selected = read_members_from_archive_bytes(
            &archive,
            "fixture",
            &[
                "regress/man/basic.in",
                "regress/man/basic.out_ascii",
                "regress/man/basic.out_html",
            ],
        )
        .expect("all requested fixture members are selected in one traversal");
        assert_eq!(selected.len(), 3);
        assert_eq!(selected["regress/man/basic.in"], b".TH BASIC 1\n");
        assert_eq!(selected["regress/man/basic.out_ascii"], b"BASIC\n");
        assert_eq!(selected["regress/man/basic.out_html"], b"BASIC\n");
    }

    fn test_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        for (path, bytes) in files {
            let mut header = Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, *path, *bytes)
                .expect("append tiny test member");
        }
        let encoder = builder.into_inner().expect("finish tar archive");
        encoder.finish().expect("finish gzip archive")
    }
}
