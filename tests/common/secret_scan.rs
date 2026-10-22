//! Reusable secret-scanning assertions for Git subprocess tests.
//!
//! The corrective security closure pass requires that every
//! persistent Codegg-owned Git surface (MutationResult, RunStore
//! artifacts, projection, error conversion) is free of credentials.
//! These helpers centralize the check so future tests can opt into
//! the same gate by calling `assert_no_credentials_in(...)` with a
//! sentinel value and any number of surfaces.
//!
//! This module carries **no assertions of its own**. It used to: five
//! `#[test]` functions lived here, and because `tests/common/` is
//! declared by eight root test families, they were compiled into all
//! eight and ran forty times — of which only one family had any use
//! for the helper. They now live in that one family, as
//! `tests/command_git/secret_scan_self_tests.inc` (M007, work package
//! D), and the assertions are unchanged.

// `tests/common/` is declared by eight root test families, and only one of
// them — `command_git` — uses this helper. The other seven compile it and
// use none of it, so every item below would be reported as dead code in
// those seven binaries. This is the standard idiom for a shared helper
// whose consumers use disjoint parts of it; it silences the lint, not a
// real defect.
#![allow(dead_code)]

/// A unique sentinel value to embed in test inputs and assert
/// against. The prefix `CODEGG_TEST_SECRET_` is intentionally
/// distinct from any realistic credential prefix so the assertion
/// is unambiguous. Use [`unique_sentinel`] to mint a unique sentinel
/// per test so concurrent runs do not collide on a shared value.
pub const SENTINEL_PREFIX: &str = "CODEGG_TEST_SECRET_";

/// Generate a per-test sentinel string. The caller passes a short
/// label; the resulting sentinel is `CODEGG_TEST_SECRET_<label>_<random>`.
pub fn unique_sentinel(label: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SENTINEL_COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sequence = SENTINEL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{SENTINEL_PREFIX}{label}_{nanos}_{sequence}")
}

/// Assert that none of the supplied text surfaces contain the
/// sentinel. This is the reusable gate the closure plan requires
/// for RunStore sentinel-scan tests.
///
/// Each surface is reported individually in the failure message so
/// a failing test points the developer at the specific surface
/// (e.g. `index_entries` vs `stdout_artifact` vs `serde_runmanifest`).
pub fn assert_no_credentials_in<I, S>(sentinel: &str, surfaces: Vec<(&str, S)>)
where
    I: AsRef<str>,
    S: IntoIterator<Item = I>,
{
    let mut failures: Vec<String> = Vec::new();
    for (label, surface) in surfaces {
        for (i, value) in surface.into_iter().enumerate() {
            let s = value.as_ref();
            if s.contains(sentinel) {
                let pos = s.find(sentinel).unwrap();
                let preview = &s[pos..s.len().min(pos + 32)];
                failures.push(format!(
                    "  surface={label} entry[{i}]: contains sentinel ({preview:?})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "credential leak detected in {} surface(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Walk a directory recursively, returning the byte contents of every
/// regular file under it. Useful for asserting that no credential
/// sentinel leaked into on-disk artifacts (RunStore manifest files,
/// stored stdout/stderr, index entries, etc.).
pub fn collect_bytes_recursive(root: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    collect_bytes_inner(root, &mut out);
    out
}

fn collect_bytes_inner(dir: &std::path::Path, out: &mut Vec<Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_bytes_inner(&path, out);
        } else if ft.is_file() {
            if let Ok(bytes) = std::fs::read(&path) {
                out.push(bytes);
            }
        }
    }
}
