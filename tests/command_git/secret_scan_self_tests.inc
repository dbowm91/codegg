//! The self-tests of `tests/common/secret_scan.rs` (M007, work package D).
//!
//! `tests/common/` is declared by **eight** root test families, and
//! `secret_scan.rs` is compiled into all of them. While the helper's five
//! `#[test]` functions lived inside `tests/common/secret_scan.rs`, they
//! were compiled — and run — **eight** times: forty executions of five
//! assertions, of which only one family had any use for the helper.
//!
//! The assertions themselves are unchanged; only their *home* moved. They
//! now live in the one family that uses the helper, `command_git`, so they
//! execute exactly once. `tests/common/secret_scan.rs` is now
//! assertion-free and carries only the helper API the families call.

use super::common::secret_scan::{
    assert_no_credentials_in, collect_bytes_recursive, unique_sentinel, SENTINEL_PREFIX,
};

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
