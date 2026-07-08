use crate::test_runner::types::{FailureClass, TestFailure, TestLanguage};

fn strip_ansi_escapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.next() == Some('[') {
                for esc_ch in chars.by_ref() {
                    if esc_ch.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct TestParseState {
    pub language: Option<TestLanguage>,
    pub tests_seen: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub last_progress_line: Option<String>,
    pub failures: Vec<TestFailure>,
    pub compile_errors: Vec<TestFailure>,
    pub collection_error_seen: bool,
}

pub fn ingest_stdout_line(state: &mut TestParseState, line: &str) {
    let line = strip_ansi_escapes(line);
    let line = line.as_str();
    let mut rust_matched = false;

    if line.contains("running ") && line.contains(" test") {
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
        // Check doctest format FIRST: "test file - func (line N) ... FAILED"
        if line.contains(" - ") && line.contains("(line ") {
            state.tests_failed += 1;
            state.language = Some(TestLanguage::Rust);
            let name = extract_doctest_name(line);
            state.failures.push(TestFailure {
                name,
                file: None,
                line: None,
                message: line.to_string(),
                failure_class: FailureClass::RustDoctestFailure,
            });
            state.last_progress_line = Some(line.to_string());
            rust_matched = true;
        } else {
            state.tests_failed += 1;
            state.language = Some(TestLanguage::Rust);
            let name = extract_rust_test_name(line);
            state.failures.push(TestFailure {
                name,
                file: None,
                line: None,
                message: line.to_string(),
                failure_class: FailureClass::RustTestFailure,
            });
            state.last_progress_line = Some(line.to_string());
            rust_matched = true;
        }
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
            failure_class: FailureClass::RustPanic,
        });
        state.language = Some(TestLanguage::Rust);
        rust_matched = true;
    }

    if let Some(err) = extract_compile_error(line) {
        state.compile_errors.push(err);
        state.language = Some(TestLanguage::Rust);
        rust_matched = true;
    } else if line.trim_start().starts_with("--> ") {
        // Location line following a compile error: "  --> src/main.rs:10:5"
        if let Some(last_err) = state.compile_errors.last_mut() {
            if last_err.file.is_none() {
                let loc = line.trim_start().strip_prefix("--> ").unwrap_or("");
                let mut parts = loc.split(':');
                if let Some(f) = parts.next() {
                    if !f.is_empty() {
                        last_err.file = Some(f.to_string());
                        last_err.line = parts.next().and_then(|l| l.parse::<u32>().ok());
                    }
                }
            }
        }
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

    if line.contains("ERRORS") || line.contains("ERROR collecting") {
        state.collection_error_seen = true;
        state.language = Some(TestLanguage::Python);
    }

    if line.contains("PASSED") && (line.contains("::") || line.contains(".py")) {
        state.tests_passed += 1;
        state.language = Some(TestLanguage::Python);
        state.last_progress_line = Some(line.to_string());
    } else if line.contains(" FAILED") || line.starts_with("FAILED ") {
        state.tests_failed += 1;
        state.language = Some(TestLanguage::Python);
        let name = extract_pytest_test_name(line);
        let (file, line_num, msg) = extract_pytest_failure_detail(line);
        state.failures.push(TestFailure {
            name,
            file,
            line: line_num,
            message: if msg.is_empty() {
                line.to_string()
            } else {
                msg
            },
            failure_class: FailureClass::PytestFailure,
        });
        state.last_progress_line = Some(line.to_string());
    } else if line.contains("::") && line.contains(" ERROR") {
        state.tests_failed += 1;
        state.language = Some(TestLanguage::Python);
        let name = extract_pytest_test_name(line);
        state.failures.push(TestFailure {
            name,
            file: None,
            line: None,
            message: line.to_string(),
            failure_class: FailureClass::PytestError,
        });
        state.last_progress_line = Some(line.to_string());
    } else if line.starts_with("ERROR ") && !line.contains("::") {
        state.collection_error_seen = true;
        state.language = Some(TestLanguage::Python);
        state.tests_failed += 1;
        state.failures.push(TestFailure {
            name: None,
            file: None,
            line: None,
            message: line.to_string(),
            failure_class: FailureClass::PytestCollectionError,
        });
    }
}

pub fn ingest_stderr_line(state: &mut TestParseState, line: &str) {
    let line = strip_ansi_escapes(line);
    let line = line.as_str();
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

    if line.starts_with("E  ") && state.failures.is_empty() {
        let msg = line.trim().to_string();
        if !msg.is_empty() {
            state.collection_error_seen = true;
            state.language = Some(TestLanguage::Python);
        }
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

fn extract_doctest_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("test ") {
        let before_status = rest.split(" ...").next()?;
        // Format: "src/lib.rs - my_function (line 5)"
        if let Some(dash_idx) = before_status.find(" - ") {
            let after_dash = &before_status[dash_idx + 3..];
            // Extract name before " (line" or end of string
            let name = if let Some(paren_idx) = after_dash.find(" (") {
                &after_dash[..paren_idx]
            } else {
                after_dash
            };
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        // Fallback: return the full before_status
        return Some(before_status.trim().to_string());
    }
    if let Some(idx) = line.find("doctest ") {
        let after = &line[idx + "doctest ".len()..];
        let name = after.split_whitespace().next()?;
        return Some(name.trim().to_string());
    }
    None
}

fn extract_panic(line: &str) -> Option<(String, Option<String>, Option<u32>)> {
    let idx = line.find("panicked at '")?;
    let start = idx + "panicked at '".len();

    let after_msg = &line[start..];
    let end = find_panic_message_end(after_msg);
    let msg = after_msg[..end].to_string();

    let rest = after_msg[end..].trim();
    let rest = rest
        .strip_prefix("'")
        .unwrap_or(rest)
        .trim_start_matches(',')
        .trim();

    let (file, line_num) = if let Some(paren_start) = rest.find('(') {
        let inner = &rest[paren_start + 1..];
        let paren_end = inner.find(')')?;
        let loc = &inner[..paren_end];
        let mut parts = loc.split(':');
        let f = parts.next()?.to_string();
        let l: u32 = parts.next()?.parse().ok()?;
        (Some(f), Some(l))
    } else if let Some(colon_pos) = rest.find(':') {
        let file_part = rest[..colon_pos]
            .trim()
            .trim_start_matches(',')
            .trim()
            .to_string();
        let rest_after = rest[colon_pos + 1..].trim();
        // Handle file:line:col format — take only the line number before any further colon
        let line_str = rest_after.split(':').next().unwrap_or("");
        let line_num: u32 = line_str.parse().ok()?;
        (Some(file_part), Some(line_num))
    } else {
        (None, None)
    };

    Some((msg, file, line_num))
}

fn find_panic_message_end(s: &str) -> usize {
    // Find the closing quote of the panic message.
    // The message is followed by either:
    //   ', file:line'   (comma-space then location)
    //   ')' or ' ('     (parenthesized location)
    //   end of string
    // Search forward from the start to find the first quote that is followed by
    // one of these patterns.
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\'' {
            let after = &s[i + 1..];
            let trimmed = after.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with(',')
                || trimmed.starts_with('(')
                || trimmed.starts_with(')')
            {
                return i;
            }
        }
    }
    s.len()
}

fn extract_compile_error(line: &str) -> Option<TestFailure> {
    let error_code = if let Some(rest) = line.strip_prefix("error[E") {
        let code_end = rest.find(']')?;
        Some(format!("E{}", &rest[..code_end]))
    } else if line.starts_with("error:") || line.starts_with("error[") {
        None
    } else {
        return None;
    };

    let message = if let Some(colon_pos) = line.find(": ") {
        line[colon_pos + 2..].trim().to_string()
    } else {
        line.trim().to_string()
    };

    let (file, line_num) = extract_location_from_diagnostic(line);

    Some(TestFailure {
        name: error_code,
        file,
        line: line_num,
        message,
        failure_class: FailureClass::RustCompileError,
    })
}

fn extract_location_from_diagnostic(line: &str) -> (Option<String>, Option<u32>) {
    let parts: Vec<&str> = line.splitn(3, "--> ").collect();
    if parts.len() >= 2 {
        let loc = parts[1].trim();
        let mut sub_parts = loc.split(':');
        if let Some(f) = sub_parts.next() {
            if !f.is_empty() {
                let file = Some(f.to_string());
                let line_num = sub_parts.next().and_then(|l| l.parse::<u32>().ok());
                return (file, line_num);
            }
        }
    }
    (None, None)
}

fn parse_pytest_collected(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("collected ")?;
    let n: usize = rest.split_whitespace().next()?.parse().ok()?;
    Some(n)
}

fn extract_pytest_test_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("FAILED ") {
        let name_part = rest.split(" - ").next()?;
        return Some(name_part.trim().to_string());
    }
    if let Some(rest) = line.strip_prefix("ERROR ") {
        let name_part = rest.split(" - ").next()?;
        return Some(name_part.trim().to_string());
    }
    let parts: Vec<&str> = line.splitn(2, " PASSED").collect();
    if parts.len() == 2 {
        return Some(parts[0].trim().to_string());
    }
    let parts: Vec<&str> = line.splitn(2, " FAILED").collect();
    if parts.len() == 2 {
        return Some(parts[0].trim().to_string());
    }
    let parts: Vec<&str> = line.splitn(2, " ERROR").collect();
    if parts.len() == 2 {
        return Some(parts[0].trim().to_string());
    }
    None
}

fn extract_pytest_failure_detail(line: &str) -> (Option<String>, Option<u32>, String) {
    let name_part = if let Some(rest) = line.strip_prefix("FAILED ") {
        rest.split(" - ").next().unwrap_or("").trim()
    } else {
        return (None, None, String::new());
    };

    let file = name_part.find("::").map(|idx| name_part[..idx].to_string());

    let line_num = None;

    let msg = if let Some(idx) = line.find(" - ") {
        line[idx + 3..].trim().to_string()
    } else {
        String::new()
    };

    (file, line_num, msg)
}

pub fn failure_class_summary(
    failures: &[TestFailure],
    compile_errors: &[TestFailure],
) -> FailureClass {
    if !compile_errors.is_empty() {
        return FailureClass::RustCompileError;
    }
    if failures
        .iter()
        .any(|f| matches!(f.failure_class, FailureClass::RustPanic))
    {
        return FailureClass::RustPanic;
    }
    if failures
        .iter()
        .any(|f| matches!(f.failure_class, FailureClass::RustDoctestFailure))
    {
        return FailureClass::RustDoctestFailure;
    }
    if failures
        .iter()
        .any(|f| matches!(f.failure_class, FailureClass::RustTestFailure))
    {
        return FailureClass::RustTestFailure;
    }
    if failures
        .iter()
        .any(|f| matches!(f.failure_class, FailureClass::PytestCollectionError))
    {
        return FailureClass::PytestCollectionError;
    }
    if failures
        .iter()
        .any(|f| matches!(f.failure_class, FailureClass::PytestError))
    {
        return FailureClass::PytestError;
    }
    if failures
        .iter()
        .any(|f| matches!(f.failure_class, FailureClass::PytestFailure))
    {
        return FailureClass::PytestFailure;
    }
    if !failures.is_empty() {
        return failures[0].failure_class;
    }
    FailureClass::UnknownFailure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_extracts_failed_test_name() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "test foo::bar::baz ... FAILED");
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].name.as_deref(), Some("foo::bar::baz"));
        assert_eq!(
            state.failures[0].failure_class,
            FailureClass::RustTestFailure
        );
    }

    #[test]
    fn rust_extracts_panic_file_line() {
        let mut state = TestParseState::default();
        ingest_stdout_line(
            &mut state,
            "thread 'main' panicked at 'assertion failed', src/foo.rs:42",
        );
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].failure_class, FailureClass::RustPanic);
        assert_eq!(state.failures[0].file.as_deref(), Some("src/foo.rs"));
        assert_eq!(state.failures[0].line, Some(42));
    }

    #[test]
    fn rust_extracts_assertion_message() {
        let mut state = TestParseState::default();
        ingest_stdout_line(
            &mut state,
            "thread 'test_worker' panicked at 'assertion `left == right` failed: left: 1, right: 2', src/lib.rs:100:5",
        );
        assert_eq!(state.failures.len(), 1);
        assert!(state.failures[0]
            .message
            .contains("assertion `left == right` failed"));
        assert_eq!(state.failures[0].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(state.failures[0].line, Some(100));
    }

    #[test]
    fn rust_extracts_compile_error_code_and_location() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "error[E0432]: unresolved import `foo`");
        assert_eq!(state.compile_errors.len(), 1);
        assert_eq!(state.compile_errors[0].name.as_deref(), Some("E0432"));
        assert_eq!(state.compile_errors[0].message, "unresolved import `foo`");
        assert_eq!(
            state.compile_errors[0].failure_class,
            FailureClass::RustCompileError
        );
    }

    #[test]
    fn rust_compile_error_with_file_location() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "error[E0382]: borrow of moved value: `x`");
        ingest_stdout_line(&mut state, "  --> src/main.rs:10:5");
        assert_eq!(state.compile_errors.len(), 1);
        assert_eq!(state.compile_errors[0].name.as_deref(), Some("E0382"));
        assert_eq!(state.compile_errors[0].file.as_deref(), Some("src/main.rs"));
        assert_eq!(state.compile_errors[0].line, Some(10));
    }

    #[test]
    fn rust_detects_doctest_failure() {
        let mut state = TestParseState::default();
        ingest_stdout_line(
            &mut state,
            "test src/lib.rs - my_function (line 5) ... FAILED",
        );
        assert_eq!(state.failures.len(), 1);
        assert_eq!(
            state.failures[0].failure_class,
            FailureClass::RustDoctestFailure
        );
    }

    #[test]
    fn pytest_extracts_failed_test_name() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "tests/test_bar.py::test_alpha FAILED");
        assert_eq!(state.tests_failed, 1);
        assert_eq!(
            state.failures[0].name.as_deref(),
            Some("tests/test_bar.py::test_alpha")
        );
        assert_eq!(state.failures[0].failure_class, FailureClass::PytestFailure);
    }

    #[test]
    fn pytest_extracts_failed_file() {
        let mut state = TestParseState::default();
        ingest_stdout_line(
            &mut state,
            "FAILED tests/test_x.py::test_y - AssertionError: bad",
        );
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].file.as_deref(), Some("tests/test_x.py"));
        assert_eq!(state.failures[0].message, "AssertionError: bad");
    }

    #[test]
    fn pytest_extracts_assertion_message() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "FAILED tests/test_x.py::test_y");
        ingest_stderr_line(&mut state, "E   AssertionError: expected 1, got 2");
        assert!(state.failures[0].message.contains("expected 1, got 2"));
    }

    #[test]
    fn pytest_detects_collection_error() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "ERRORS");
        ingest_stdout_line(
            &mut state,
            "ERROR collecting tests/test_broken.py - ModuleNotFoundError: No module named 'nonexistent'",
        );
        assert!(state.collection_error_seen);
        assert_eq!(state.failures.len(), 1);
        assert_eq!(
            state.failures[0].failure_class,
            FailureClass::PytestCollectionError
        );
    }

    #[test]
    fn pytest_detects_error_vs_failure() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "tests/test_x.py::test_error ERROR");
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].failure_class, FailureClass::PytestError);
        assert_eq!(state.tests_failed, 1);
    }

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
    fn rust_parser_detects_panic_file_line_legacy() {
        let mut state = TestParseState::default();
        ingest_stdout_line(
            &mut state,
            "thread 'main' panicked at 'assertion failed', src/foo.rs:42",
        );
        assert_eq!(state.failures.len(), 1);
        assert_eq!(state.failures[0].failure_class, FailureClass::RustPanic);
        assert_eq!(state.failures[0].file.as_deref(), Some("src/foo.rs"));
        assert_eq!(state.failures[0].line, Some(42));
    }

    #[test]
    fn rust_parser_detects_compile_error_legacy() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "error[E0432]: unresolved import `foo`");
        assert_eq!(state.compile_errors.len(), 1);
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
    fn pytest_parser_extracts_assertion_message_legacy() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "FAILED tests/test_x.py::test_y");
        ingest_stderr_line(&mut state, "E   AssertionError: expected 1, got 2");
        assert!(state.failures[0].message.contains("expected 1, got 2"));
    }

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1b[31mtest foo::bar ... FAILED\x1b[0m";
        let stripped = strip_ansi_escapes(input);
        assert_eq!(stripped, "test foo::bar ... FAILED");
    }

    #[test]
    fn strip_ansi_handles_no_escapes() {
        let input = "test foo::bar ... ok";
        let stripped = strip_ansi_escapes(input);
        assert_eq!(stripped, "test foo::bar ... ok");
    }

    #[test]
    fn strip_ansi_handles_multiple_codes() {
        let input = "\x1b[1m\x1b[31mFAILED\x1b[0m \x1b[32mok\x1b[0m";
        let stripped = strip_ansi_escapes(input);
        assert_eq!(stripped, "FAILED ok");
    }

    #[test]
    fn ansi_wrapped_failure_detected() {
        let mut state = TestParseState::default();
        ingest_stdout_line(&mut state, "\x1b[31mtest foo::bar ... FAILED\x1b[0m");
        assert_eq!(state.tests_failed, 1);
        assert_eq!(state.failures[0].name.as_deref(), Some("foo::bar"));
    }
}
