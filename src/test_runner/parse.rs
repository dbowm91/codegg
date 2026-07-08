use crate::test_runner::types::{TestFailure, TestLanguage};

#[derive(Debug, Clone, Default)]
pub struct TestParseState {
    pub language: Option<TestLanguage>,
    pub tests_seen: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub last_progress_line: Option<String>,
    pub failures: Vec<TestFailure>,
    pub compile_error_seen: bool,
}

pub fn ingest_stdout_line(state: &mut TestParseState, line: &str) {
    let mut rust_matched = false;

    if line.contains("running ") && line.contains(" tests") {
        if let Some(n) = parse_test_count(line) {
            state.tests_seen = n;
            state.language = Some(TestLanguage::Rust);
            rust_matched = true;
        }
    }

    if line.contains("test ") && line.contains(" ... ok") {
        state.tests_passed += 1;
        state.language = Some(TestLanguage::Rust);
        state.last_progress_line = Some(line.to_string());
        rust_matched = true;
    } else if line.contains("test ") && line.contains(" ... FAILED") {
        state.tests_failed += 1;
        state.language = Some(TestLanguage::Rust);
        let name = extract_rust_test_name(line);
        state.failures.push(TestFailure {
            name,
            file: None,
            line: None,
            message: line.to_string(),
            failure_class: "test_failed".to_string(),
        });
        state.last_progress_line = Some(line.to_string());
        rust_matched = true;
    }

    if line.starts_with("test result:") {
        state.last_progress_line = Some(line.to_string());
        rust_matched = true;
    }

    if let Some((msg, file, line_num)) = extract_panic(line) {
        state.failures.push(TestFailure {
            name: None,
            file,
            line: line_num,
            message: msg,
            failure_class: "panic".to_string(),
        });
        state.language = Some(TestLanguage::Rust);
        rust_matched = true;
    }

    if line.starts_with("error[E") || line.contains("error[E") {
        state.compile_error_seen = true;
        state.language = Some(TestLanguage::Rust);
        rust_matched = true;
    }

    if rust_matched {
        return;
    }

    if line.contains("collected ") && line.contains(" items") {
        if let Some(n) = parse_pytest_collected(line) {
            state.tests_seen = n;
            state.language = Some(TestLanguage::Python);
        }
    }

    if line.contains(" PASSED") {
        state.tests_passed += 1;
        state.language = Some(TestLanguage::Python);
        state.last_progress_line = Some(line.to_string());
    } else if line.contains(" FAILED") || line.starts_with("FAILED ") {
        state.tests_failed += 1;
        state.language = Some(TestLanguage::Python);
        let name = extract_pytest_test_name(line);
        state.failures.push(TestFailure {
            name,
            file: None,
            line: None,
            message: line.to_string(),
            failure_class: "test_failed".to_string(),
        });
        state.last_progress_line = Some(line.to_string());
    } else if line.starts_with("ERROR ") {
        state.tests_failed += 1;
        state.language = Some(TestLanguage::Python);
        let name = extract_pytest_test_name(line);
        state.failures.push(TestFailure {
            name,
            file: None,
            line: None,
            message: line.to_string(),
            failure_class: "error".to_string(),
        });
    }
}

pub fn ingest_stderr_line(state: &mut TestParseState, line: &str) {
    if let Some(rest) = line.strip_prefix("E   ") {
        let msg = rest.trim().to_string();
        if let Some(last) = state.failures.last_mut() {
            if !msg.is_empty() {
                last.message.push('\n');
                last.message.push_str(&msg);
            }
        }
    }

    if line.contains("FAILED") || line.contains("failures:") {
        state.last_progress_line = Some(line.to_string());
    }
}

fn parse_test_count(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("running ")?;
    let n: usize = rest.split_whitespace().next()?.parse().ok()?;
    Some(n)
}

fn extract_rust_test_name(line: &str) -> Option<String> {
    let after_test = line.strip_prefix("test ")?;
    let name = after_test.split(" ...").next()?;
    Some(name.trim().to_string())
}

fn extract_panic(line: &str) -> Option<(String, Option<String>, Option<u32>)> {
    let idx = line.find("panicked at '")?;
    let start = idx + "panicked at '".len();
    let end = line[start..].find("'")?;
    let msg = line[start..start + end].to_string();

    let after = &line[start + end + 1..];
    let (file, line_num) = if let Some(paren_start) = after.find('(') {
        let inner = &after[paren_start + 1..];
        let paren_end = inner.find(')')?;
        let loc = &inner[..paren_end];
        let mut parts = loc.split(':');
        let f = parts.next()?.to_string();
        let l: u32 = parts.next()?.parse().ok()?;
        (Some(f), Some(l))
    } else if let Some(colon_pos) = after.find(':') {
        let file_part = after[..colon_pos]
            .trim()
            .trim_start_matches(',')
            .trim()
            .to_string();
        let rest_after = after[colon_pos + 1..].trim();
        let line_num: u32 = rest_after.split_whitespace().next()?.parse().ok()?;
        (Some(file_part), Some(line_num))
    } else {
        (None, None)
    };

    Some((msg, file, line_num))
}

fn parse_pytest_collected(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("collected ")?;
    let n: usize = rest.split_whitespace().next()?.parse().ok()?;
    Some(n)
}

fn extract_pytest_test_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("FAILED ") {
        return Some(rest.trim().to_string());
    }
    let parts: Vec<&str> = line.splitn(2, " PASSED").collect();
    if parts.len() == 2 {
        return Some(parts[0].trim().to_string());
    }
    let parts: Vec<&str> = line.splitn(2, " FAILED").collect();
    if parts.len() == 2 {
        return Some(parts[0].trim().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_parser_detects_running_count() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "running 3 tests");
        assert_eq!(state.tests_seen, 3);
        assert_eq!(state.language, Some(TestLanguage::Rust));
    }

    #[test]
    fn rust_parser_detects_ok_and_failed_tests() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "test foo::bar ... ok");
        ingest_stdout_line(&mut state, "test foo::baz ... FAILED");
        assert_eq!(state.tests_passed, 1);
        assert_eq!(state.tests_failed, 1);
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].name.as_deref(), Some("foo::baz"));
    }

    #[test]
    fn rust_parser_detects_panic_file_line() {
        let mut state = TestParseState::default();
        ingest_stdout_line(
            &mut state,
            "thread 'main' panicked at 'assertion failed', src/foo.rs:42",
        );
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].failure_class, "panic");
        assert_eq!(state.failures[0].file.as_deref(), Some("src/foo.rs"));
        assert_eq!(state.failures[0].line, Some(42));
    }

    #[test]
    fn rust_parser_detects_compile_error() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "error[E0432]: unresolved import `foo`");
        assert!(state.compile_error_seen);
        assert_eq!(state.language, Some(TestLanguage::Rust));
    }

    #[test]
    fn pytest_parser_detects_collection_count() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "collected 12 items");
        assert_eq!(state.tests_seen, 12);
        assert_eq!(state.language, Some(TestLanguage::Python));
    }

    #[test]
    fn pytest_parser_detects_failed_summary() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "tests/test_bar.py::test_alpha FAILED");
        assert_eq!(state.tests_failed, 1);
        assert_eq!(
            state.failures[0].name.as_deref(),
            Some("tests/test_bar.py::test_alpha")
        );
    }

    #[test]
    fn pytest_parser_extracts_assertion_message() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "FAILED tests/test_x.py::test_y");
        ingest_stderr_line(&mut state, "E   AssertionError: expected 1, got 2");
        assert!(state.failures[0].message.contains("expected 1, got 2"));
    }
}
