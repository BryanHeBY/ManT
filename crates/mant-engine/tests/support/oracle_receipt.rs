//! Common checks for recorded runs; fixture structs keep their original schema.

/// A borrowed run receipt. Older fixtures record stdout as `sha256` and have
/// no stderr hash; newer fixtures retain both stream hashes explicitly.
pub struct OracleReceipt<'a> {
    pub status: i32,
    pub stdout_sha256: &'a str,
    pub stderr_sha256: Option<&'a str>,
    pub diagnostics: &'a str,
}

pub fn assert_receipts<'a>(
    case_id: &str,
    source_sha256: &str,
    receipts: impl ExactSizeIterator<Item = (&'a str, OracleReceipt<'a>)>,
) {
    assert_eq!(source_sha256.len(), 64, "{case_id}: source identity");
    assert_eq!(receipts.len(), 5, "{case_id}: recorded profiles");
    for (profile, run) in receipts {
        assert_eq!(run.stdout_sha256.len(), 64, "{case_id}: {profile} stdout");
        if let Some(stderr_sha256) = run.stderr_sha256 {
            assert_eq!(stderr_sha256.len(), 64, "{case_id}: {profile} stderr");
        }
        if profile == "lint" {
            assert!(
                run.status == 0 || (run.status == 2 && !run.diagnostics.is_empty()),
                "{case_id}: {profile} status {}: {}",
                run.status,
                run.diagnostics
            );
        } else {
            assert_eq!(run.status, 0, "{case_id}: {profile}");
        }
    }
}
