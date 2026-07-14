//! Reusable secret-scanning assertions for Git subprocess tests.
//!
//! The corrective security closure pass requires that every
//! persistent Codegg-owned Git surface (MutationResult, RunStore
//! artifacts, projection, error conversion) is free of credentials.
//! These helpers centralize the check so future tests can opt into
//! the same gate by calling `assert_no_credentials_in(...)` with a
//! sentinel value and any number of surfaces.

/// A unique sentinel value to embed in test inputs and assert
/// against. The prefix `CODEGG_TEST_SECRET_` is intentionally
/// distinct from any realistic credential prefix so the assertion
/// is unambiguous. Use [`unique_sentinel`] to mint a unique sentinel
/// per test so concurrent runs do not collide on a shared value.
pub const SENTINEL_PREFIX: &str = "CODEGG_TEST_SECRET_";

/// Generate a per-test sentinel string. The caller passes a short
/// label; the resulting sentinel is `CODEGG_TEST_SECRET_<label>_<random>`.
pub fn unique_sentinel(label: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{SENTINEL_PREFIX}{label}_{nanos}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_format_starts_with_prefix() {
        let s = unique_sentinel("foo");
        assert!(s.starts_with(SENTINEL_PREFIX), "got {s}");
        assert!(s.contains("foo"), "missing label: {s}");
    }

    #[test]
    fn sentinel_unique_per_call() {
        let a = unique_sentinel("a");
        let b = unique_sentinel("a");
        assert_ne!(a, b, "two sentinels with same label collided");
    }

    #[test]
    fn assert_no_credentials_passes_when_clean() {
        let sentinel = "CODEGG_TEST_SECRET_x";
        let surfaces: Vec<(&str, Vec<&str>)> = vec![
            ("stdout", vec!["clean output", "more text"]),
            ("stderr", vec![""]),
            (
                "argv",
                vec!["git", "remote", "add", "origin", "https://example.com/x"],
            ),
        ];
        assert_no_credentials_in(sentinel, surfaces);
    }

    #[test]
    #[should_panic(expected = "credential leak detected")]
    fn assert_no_credentials_fails_on_leak() {
        let sentinel = "CODEGG_TEST_SECRET_y";
        let surfaces: Vec<(&str, Vec<&str>)> =
            vec![("stdout", vec!["hello CODEGG_TEST_SECRET_y world"])];
        assert_no_credentials_in(sentinel, surfaces);
    }

    #[test]
    fn collect_bytes_finds_file_contents() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), b"hello world").expect("write");
        std::fs::create_dir(dir.path().join("nested")).expect("mkdir");
        std::fs::write(dir.path().join("nested/b.txt"), b"goodbye").expect("write");
        let all = collect_bytes_recursive(dir.path());
        let flat: Vec<u8> = all.iter().flatten().copied().collect();
        let s = String::from_utf8_lossy(&flat);
        assert!(s.contains("hello world"));
        assert!(s.contains("goodbye"));
    }
}
