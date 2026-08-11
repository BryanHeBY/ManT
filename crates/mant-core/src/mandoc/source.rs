//! Owns bounded manual-source I/O and constrained `.so` alias resolution.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Cursor, Read},
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

use flate2::read::MultiGzDecoder;

use crate::ManualPage;

use super::error::ManualError;

/// Upper bound on both the stored and decoded form of one manual source chain.
///
/// The loader enforces the limit while reading instead of trusting file
/// metadata, so special files and high-ratio compressed inputs remain bounded.
pub const MAX_MANUAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SO_REDIRECTS: usize = 16;

pub(super) struct LoadedManualSource {
    pub(super) source: Vec<u8>,
}

#[derive(Debug)]
pub(super) struct ResolvedManualSource {
    pub(super) source: Vec<u8>,
    pub(super) alias_target: Option<String>,
}

#[derive(Clone, Copy)]
struct ManualBudget {
    stored_remaining: u64,
    decoded_remaining: u64,
}

impl ManualBudget {
    const fn new(limit: u64) -> Self {
        Self {
            stored_remaining: limit,
            decoded_remaining: limit,
        }
    }

    fn charge_stored(&mut self, path: &Path, amount: usize) -> Result<(), ManualError> {
        self.stored_remaining = remaining_budget(path, self.stored_remaining, amount, "stored")?;
        Ok(())
    }

    fn charge_decoded(&mut self, path: &Path, amount: usize) -> Result<(), ManualError> {
        self.decoded_remaining = remaining_budget(path, self.decoded_remaining, amount, "decoded")?;
        Ok(())
    }
}

pub(super) fn load_manual_source(path: &Path) -> Result<LoadedManualSource, ManualError> {
    load_manual_source_with_budget(path, &mut ManualBudget::new(MAX_MANUAL_BYTES))
}

fn load_manual_source_with_budget(
    path: &Path,
    budget: &mut ManualBudget,
) -> Result<LoadedManualSource, ManualError> {
    let file = File::open(path).map_err(|error| source_error(path, &error))?;
    let stored =
        read_capped(file, budget.stored_remaining).map_err(|error| source_error(path, &error))?;
    budget.charge_stored(path, stored.len())?;

    if stored.starts_with(&[0x1f, 0x8b]) || path.extension().is_some_and(|value| value == "gz") {
        let source = read_capped(
            MultiGzDecoder::new(Cursor::new(stored)),
            budget.decoded_remaining,
        )
        .map_err(|error| decompression_error(path, "gzip", &error))?;
        budget.charge_decoded(path, source.len())?;
        return Ok(LoadedManualSource { source });
    }
    if stored.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
        || path.extension().is_some_and(|value| value == "zst")
    {
        let decoder = zstd::stream::read::Decoder::new(Cursor::new(stored))
            .map_err(|error| decompression_error(path, "zstd", &error))?;
        let source = read_capped(decoder, budget.decoded_remaining)
            .map_err(|error| decompression_error(path, "zstd", &error))?;
        budget.charge_decoded(path, source.len())?;
        return Ok(LoadedManualSource { source });
    }
    budget.charge_decoded(path, stored.len())?;
    Ok(LoadedManualSource { source: stored })
}

pub(super) fn resolve_manual_redirects(
    page: &ManualPage,
) -> Result<ResolvedManualSource, ManualError> {
    resolve_manual_redirects_with_budget(page, ManualBudget::new(MAX_MANUAL_BYTES))
}

fn resolve_manual_redirects_with_budget(
    page: &ManualPage,
    mut budget: ManualBudget,
) -> Result<ResolvedManualSource, ManualError> {
    let manual_root = fs::canonicalize(&page.manual_root)
        .map_err(|error| source_error(&page.manual_root, &error))?;
    let mut current = page.path.clone();
    let mut visited = HashSet::new();
    let mut redirects = 0_usize;
    let mut alias_target = None;

    loop {
        let identity =
            fs::canonicalize(&current).map_err(|error| source_error(&current, &error))?;
        if !visited.insert(identity) {
            return Err(ManualError::redirect(
                &current,
                "manual .so redirect cycle detected",
            ));
        }

        let loaded = load_manual_source_with_budget(&current, &mut budget)?;

        let Some(target) = redirect_target(&current, &loaded.source)? else {
            return Ok(ResolvedManualSource {
                source: loaded.source,
                alias_target,
            });
        };
        if redirects == MAX_SO_REDIRECTS {
            return Err(ManualError::redirect(
                &current,
                format!("manual .so redirect depth exceeds {MAX_SO_REDIRECTS}"),
            ));
        }
        alias_target.get_or_insert_with(|| String::from_utf8_lossy(&target).into_owned());
        current = resolve_redirect_target(&current, &manual_root, &target)?;
        redirects += 1;
    }
}

fn remaining_budget(
    path: &Path,
    remaining: u64,
    amount: usize,
    form: &str,
) -> Result<u64, ManualError> {
    let amount = u64::try_from(amount)
        .map_err(|_| ManualError::limit(path, "manual byte budget overflow"))?;
    remaining.checked_sub(amount).ok_or_else(|| {
        ManualError::limit(
            path,
            format!("manual .so chain exceeds the {MAX_MANUAL_BYTES}-byte {form} input limit"),
        )
    })
}

fn redirect_target(path: &Path, source: &[u8]) -> Result<Option<Vec<u8>>, ManualError> {
    let mut target = None;
    let mut has_other_content = false;

    for raw_line in source.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if trim_ascii(line).is_empty() || is_roff_comment(line) {
            continue;
        }
        let Some(payload) = so_request_payload(line) else {
            has_other_content = true;
            continue;
        };
        if target.is_some() {
            return Err(unsupported_so_error(path));
        }
        target = Some(parse_so_target(path, payload)?);
    }

    match (target, has_other_content) {
        (None, _) => Ok(None),
        (Some(_), true) => Err(unsupported_so_error(path)),
        (Some(target), false) => Ok(Some(target)),
    }
}

fn so_request_payload(line: &[u8]) -> Option<&[u8]> {
    let payload = line
        .strip_prefix(b".so")
        .or_else(|| line.strip_prefix(b"'so"))?;
    (payload.is_empty() || payload[0].is_ascii_whitespace()).then_some(payload)
}

fn parse_so_target(path: &Path, payload: &[u8]) -> Result<Vec<u8>, ManualError> {
    let payload = trim_ascii(payload);
    let target_end = payload
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(payload.len());
    let target = &payload[..target_end];
    let trailing = trim_ascii(&payload[target_end..]);
    if target.is_empty()
        || target.contains(&0)
        || (!trailing.is_empty() && !trailing.starts_with(b"\\\"") && !trailing.starts_with(b"\\#"))
    {
        return Err(ManualError::redirect(
            path,
            "manual .so redirect must contain exactly one target path",
        ));
    }
    Ok(target.to_vec())
}

fn is_roff_comment(line: &[u8]) -> bool {
    [b".\\\"".as_slice(), b"'\\\"", b".\\#", b"'\\#"]
        .into_iter()
        .any(|prefix| line.starts_with(prefix))
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn unsupported_so_error(path: &Path) -> ManualError {
    ManualError::redirect(path, "only redirect-only .so manual pages are supported")
}

fn resolve_redirect_target(
    source_path: &Path,
    manual_root: &Path,
    target: &[u8],
) -> Result<PathBuf, ManualError> {
    #[cfg(unix)]
    let target_path = PathBuf::from(OsStr::from_bytes(target));
    #[cfg(windows)]
    let target_path = redirect_path(source_path, target)?;
    if target_path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManualError::unsafe_path(
            source_path,
            format!(
                "manual .so target '{}' is not a safe relative path",
                target_path.to_string_lossy()
            ),
        ));
    }

    let mut bases = vec![manual_root.join(&target_path)];
    if target_path.components().count() == 1
        && let Some(source_directory) = source_path.parent()
    {
        bases.push(source_directory.join(&target_path));
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for base in bases {
        for candidate in compression_candidates(&base) {
            if seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
        }
    }

    for candidate in candidates {
        match fs::canonicalize(&candidate) {
            Ok(canonical) if canonical.starts_with(manual_root) => return Ok(canonical),
            Ok(_) => {
                return Err(ManualError::unsafe_path(
                    &candidate,
                    "manual .so target resolves outside the manual root",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(source_error(&candidate, &error));
            }
        }
    }

    Err(ManualError::read(
        source_path,
        format!(
            "could not resolve manual .so target '{}'",
            target_path.to_string_lossy()
        ),
    ))
}

#[cfg(windows)]
fn redirect_path(source_path: &Path, target: &[u8]) -> Result<PathBuf, ManualError> {
    std::str::from_utf8(target)
        .map(PathBuf::from)
        .map_err(|_| ManualError::unsafe_path(source_path, "manual .so target must be UTF-8"))
}

fn compression_candidates(path: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    if !path
        .extension()
        .is_some_and(|extension| extension == "gz" || extension == "zst")
    {
        for suffix in [".gz", ".zst"] {
            let mut compressed = path.as_os_str().to_os_string();
            compressed.push(suffix);
            candidates.push(compressed.into());
        }
    }
    candidates
}

fn read_capped(reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    crate::bounded::read_bytes(reader, limit, "manual source")
}

fn source_error(path: &Path, error: &io::Error) -> ManualError {
    if crate::bounded::is_limit_exceeded(error) {
        ManualError::limit(path, error.to_string())
    } else {
        ManualError::read(path, error.to_string())
    }
}

fn decompression_error(path: &Path, format: &str, error: &io::Error) -> ManualError {
    if crate::bounded::is_limit_exceeded(error) {
        ManualError::limit(path, error.to_string())
    } else {
        ManualError::decompression(
            path,
            format!("could not decompress {format} manual source: {error}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        process,
    };

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use crate::{ManualErrorKind, ManualPage};
    use flate2::{Compression as GzipCompression, write::GzEncoder};

    use super::super::{parse_manual_page, parse_manual_source};
    use super::{
        MAX_MANUAL_BYTES, MAX_SO_REDIRECTS, ManualBudget, read_capped,
        resolve_manual_redirects_with_budget,
    };

    #[test]
    fn decodes_gzip_and_zstd_before_calling_libmandoc() {
        let source = b".TH COMPRESSED 1\n.SH NAME\ncompressed \\- decoded by mant\n";
        let base = std::env::temp_dir().join(format!("mant-compressed-{}", process::id()));
        fs::create_dir_all(&base).expect("create compressed fixture directory");

        let gzip_path = base.join("gzip.1");
        let mut gzip = GzEncoder::new(Vec::new(), GzipCompression::fast());
        gzip.write_all(source).expect("encode gzip fixture");
        fs::write(&gzip_path, gzip.finish().expect("finish gzip fixture"))
            .expect("write gzip fixture");

        let zstd_path = base.join("zstd.1");
        fs::write(
            &zstd_path,
            zstd::stream::encode_all(source.as_slice(), 1).expect("encode zstd fixture"),
        )
        .expect("write zstd fixture");

        for path in [&gzip_path, &zstd_path] {
            let document = parse_manual_source(path).expect("decode compressed manual by magic");
            assert_eq!(document.meta.title.as_deref(), Some("COMPRESSED"));
        }
        fs::remove_dir_all(base).expect("remove compressed fixtures");
    }

    #[test]
    fn indexed_pages_expand_redirects_against_their_explicit_root() {
        let root = std::env::temp_dir().join(format!("mant-indexed-so-{}", process::id()));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create manual section");
        fs::write(
            man1.join("target.1"),
            ".TH INDEXED-TARGET 1\n.SH NAME\ntarget \\- explicit include root\n",
        )
        .expect("write redirect target");
        let alias = man1.join("alias.1.gz");
        let mut gzip = GzEncoder::new(Vec::new(), GzipCompression::fast());
        gzip.write_all(b".so target.1\n")
            .expect("encode redirect stub");
        fs::write(&alias, gzip.finish().expect("finish redirect stub"))
            .expect("write redirect stub");

        let document = parse_manual_page(&ManualPage {
            name: "alias".to_owned(),
            section: "1".to_owned(),
            path: alias,
            manual_root: root.clone(),
        })
        .expect("load indexed redirect");
        fs::remove_dir_all(root).expect("remove indexed redirect fixture");

        assert_eq!(document.meta.title.as_deref(), Some("INDEXED-TARGET"));
        assert_eq!(document.meta.alias_target.as_deref(), Some("target.1"));
    }

    #[test]
    fn redirect_chains_find_compressed_targets_under_the_manual_root() {
        let root = std::env::temp_dir().join(format!("mant-root-so-{}", process::id()));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create manual section");
        let target = b".TH ROOT-TARGET 1\n.SH NAME\ntarget \\- zstd redirect target\n";
        fs::write(
            man1.join("target.1.zst"),
            zstd::stream::encode_all(target.as_slice(), 1).expect("encode redirect target"),
        )
        .expect("write redirect target");
        let alias = man1.join("alias.1");
        fs::write(
            &alias,
            r#".\" redirect fixture

'so man1/target.1 \" root target
"#,
        )
        .expect("write redirect stub");

        let document = parse_manual_page(&ManualPage {
            name: "alias".to_owned(),
            section: "1".to_owned(),
            path: alias.clone(),
            manual_root: root.clone(),
        })
        .expect("resolve compressed root target");
        fs::remove_dir_all(root).expect("remove compressed redirect fixture");

        assert_eq!(document.meta.title.as_deref(), Some("ROOT-TARGET"));
        assert_eq!(document.meta.alias_target.as_deref(), Some("man1/target.1"));
        assert_eq!(
            document.source.path.as_deref(),
            Some(alias.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn embedded_so_requests_fail_instead_of_silently_losing_content() {
        let root = std::env::temp_dir().join(format!("mant-embedded-so-{}", process::id()));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create manual section");
        fs::write(
            man1.join("target.1"),
            ".TH TARGET 1\n.SH NAME\ntarget \\- must not be partially included\n",
        )
        .expect("write redirect target");
        let source = man1.join("mixed.1");
        fs::write(&source, ".TH MIXED 1\n.so target.1\n.SH NAME\nmixed\n")
            .expect("write mixed source");

        let error = parse_manual_page(&ManualPage {
            name: "mixed".to_owned(),
            section: "1".to_owned(),
            path: source,
            manual_root: root.clone(),
        })
        .expect_err("reject an embedded include");
        fs::remove_dir_all(root).expect("remove mixed source fixture");

        assert_eq!(error.kind(), ManualErrorKind::Redirect);
        assert!(error.message().contains("redirect-only"));
    }

    #[cfg(unix)]
    #[test]
    fn redirect_chains_reject_cycles_parent_paths_and_escaping_symlinks() {
        let base = std::env::temp_dir().join(format!("mant-hostile-so-{}", process::id()));
        let root = base.join("root");
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create manual section");

        let first = man1.join("first.1");
        fs::write(&first, ".so second.1\n").expect("write first cycle page");
        fs::write(man1.join("second.1"), ".so first.1\n").expect("write second cycle page");
        let cycle = parse_manual_page(&ManualPage {
            name: "first".to_owned(),
            section: "1".to_owned(),
            path: first,
            manual_root: root.clone(),
        })
        .expect_err("reject a redirect cycle");
        assert!(cycle.message().contains("cycle"));

        let parent = man1.join("parent.1");
        fs::write(&parent, ".so ../outside.1\n").expect("write parent redirect");
        let parent_error = parse_manual_page(&ManualPage {
            name: "parent".to_owned(),
            section: "1".to_owned(),
            path: parent,
            manual_root: root.clone(),
        })
        .expect_err("reject parent traversal");
        assert_eq!(parent_error.kind(), ManualErrorKind::UnsafePath);

        let outside = base.join("outside.1");
        fs::write(
            &outside,
            ".TH OUTSIDE 1\n.SH NAME\noutside \\- must not be read\n",
        )
        .expect("write outside source");
        let top_level_link = man1.join("top-level-link.1");
        symlink(&outside, &top_level_link).expect("link top-level page outside manual root");
        let linked = parse_manual_page(&ManualPage {
            name: "top-level-link".to_owned(),
            section: "1".to_owned(),
            path: top_level_link,
            manual_root: root.clone(),
        })
        .expect("read an explicitly indexed top-level symlink");
        assert_eq!(linked.meta.title.as_deref(), Some("OUTSIDE"));

        symlink(&outside, man1.join("escape.1")).expect("link outside manual root");
        let alias = man1.join("alias.1");
        fs::write(&alias, ".so man1/escape.1\n").expect("write escaping redirect");
        let symlink_error = parse_manual_page(&ManualPage {
            name: "alias".to_owned(),
            section: "1".to_owned(),
            path: alias,
            manual_root: root.clone(),
        })
        .expect_err("reject a symlink escaping the manual root");
        fs::remove_dir_all(base).expect("remove hostile redirect fixtures");

        assert_eq!(symlink_error.kind(), ManualErrorKind::UnsafePath);
        assert!(symlink_error.message().contains("outside"));
    }

    #[test]
    fn redirect_depth_is_bounded_before_loading_another_target() {
        let root = std::env::temp_dir().join(format!("mant-deep-so-{}", process::id()));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create manual section");
        for depth in 0..=MAX_SO_REDIRECTS {
            fs::write(
                man1.join(format!("page-{depth}.1")),
                format!(".so page-{}.1\n", depth + 1),
            )
            .expect("write redirect chain page");
        }
        fs::write(
            man1.join(format!("page-{}.1", MAX_SO_REDIRECTS + 1)),
            ".TH TOO-DEEP 1\n.SH NAME\ntoo-deep \\- must not be reached\n",
        )
        .expect("write redirect chain target");

        let error = parse_manual_page(&ManualPage {
            name: "page-0".to_owned(),
            section: "1".to_owned(),
            path: man1.join("page-0.1"),
            manual_root: root.clone(),
        })
        .expect_err("reject an excessive redirect chain");
        fs::remove_dir_all(root).expect("remove deep redirect fixture");

        assert_eq!(error.kind(), ManualErrorKind::Redirect);
        assert!(error.message().contains("depth"));
    }

    #[test]
    fn redirect_chain_reads_against_the_remaining_total_budget() {
        let root = std::env::temp_dir().join(format!("mant-so-budget-{}", process::id()));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create manual section");
        fs::write(man1.join("alias.1"), "                    \n.so target.1\n")
            .expect("write padded redirect");
        fs::write(
            man1.join("target.1"),
            ".TH BUDGET 1\n.SH NAME\nbudget \\- target\n",
        )
        .expect("write redirect target");
        let page = ManualPage {
            name: "alias".to_owned(),
            section: "1".to_owned(),
            path: man1.join("alias.1"),
            manual_root: root.clone(),
        };

        let error = resolve_manual_redirects_with_budget(&page, ManualBudget::new(48))
            .expect_err("combined redirect inputs must fit the total budget");
        assert_eq!(error.kind(), ManualErrorKind::Limit);
        assert!(error.message().contains("14-byte limit"), "{error}");
        fs::remove_dir_all(root).expect("remove budget fixture");
    }

    #[test]
    fn manual_reads_are_bounded_without_trusting_reader_metadata() {
        let error = read_capped(
            std::io::repeat(0).take(MAX_MANUAL_BYTES + 1),
            MAX_MANUAL_BYTES,
        )
        .expect_err("reject an oversized streaming source");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeds"));
    }
}
