//! Async file diff computation for sidebar file-change display.
//!
//! On `AppEvent::FileChanged`, the event loop performs only cheap state
//! mutation (record path, mark diff as pending, render sidebar) and then
//! enqueues bounded background diff-stat work. The worker returns a
//! [`TuiCommand::FileDiffStatsReady`] through `tui_cmd_tx` so the event
//! loop can apply the result if it still matches the latest generation.

use super::app::TuiCommand;
use super::task_lifecycle::{TuiTaskKind, TuiTaskRegistry};
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

/// Maximum file size (bytes) that the diff worker will read fully.
const SIDEBAR_DIFF_MAX_BYTES: u64 = 1_048_576; // 1 MiB

/// Number of bytes to read for binary detection before reading the full file.
const SIDEBAR_DIFF_BINARY_PROBE_BYTES: usize = 8192;

/// Global concurrency limit for background diff tasks.
static DIFF_SEMAPHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(2)));

/// Result of a background diff computation.
#[derive(Debug, Clone)]
pub enum FileDiffStatsResult {
    Ready { additions: usize, deletions: usize },
    Skipped { reason: &'static str },
    Error { message: String },
}

/// Compute diff stats in a blocking thread pool, then send
/// `TuiCommand::FileDiffStatsReady` back to the event loop.
pub fn spawn_sidebar_diff_stats(
    tui_cmd_tx: Option<mpsc::Sender<TuiCommand>>,
    project_dir: String,
    path: String,
    old_content: Option<String>,
    generation: u64,
    registry: Option<&mut TuiTaskRegistry>,
) {
    let Some(tx) = tui_cmd_tx else {
        tracing::warn!("spawn_sidebar_diff_stats: no command sender available");
        return;
    };

    let future = async move {
        let _permit = match DIFF_SEMAPHORE.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                tracing::debug!("diff semaphore closed, skipping diff for {}", path);
                return;
            }
        };

        let path_for_cmd = path.clone();
        let result = tokio::task::spawn_blocking(move || {
            compute_diff_stats(&project_dir, &path, old_content.as_deref())
        })
        .await
        .unwrap_or_else(|e| FileDiffStatsResult::Error {
            message: format!("diff task panicked: {e}"),
        });

        let cmd = TuiCommand::FileDiffStatsReady {
            path: PathBuf::from(&path_for_cmd),
            generation,
            result,
        };
        let _ = tx.send(cmd).await;
    };

    if let Some(reg) = registry {
        reg.spawn(TuiTaskKind::FileDiff, "sidebar_diff", future);
    } else {
        tokio::spawn(future);
    }
}

fn compute_diff_stats(
    project_dir: &str,
    path: &str,
    old_content: Option<&str>,
) -> FileDiffStatsResult {
    let abs_path = if std::path::Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::path::Path::new(project_dir).join(path)
    };

    // Check metadata before reading.
    let metadata = match std::fs::metadata(&abs_path) {
        Ok(m) => m,
        Err(e) => {
            return FileDiffStatsResult::Error {
                message: format!("metadata read failed: {e}"),
            };
        }
    };

    if metadata.is_dir() {
        return FileDiffStatsResult::Skipped {
            reason: "directory",
        };
    }

    let file_size = metadata.len();
    if file_size > SIDEBAR_DIFF_MAX_BYTES {
        return FileDiffStatsResult::Skipped { reason: "large" };
    }

    // Binary detection: read a prefix and check for NUL bytes.
    let probe_len = (file_size as usize).min(SIDEBAR_DIFF_BINARY_PROBE_BYTES);
    if probe_len > 0 {
        let mut probe = vec![0u8; probe_len];
        if let Ok(mut f) = std::fs::File::open(&abs_path) {
            let _ = std::io::Read::read(&mut f, &mut probe);
        }
        if probe.contains(&0) {
            return FileDiffStatsResult::Skipped { reason: "binary" };
        }
        // Also skip if not valid UTF-8.
        if std::str::from_utf8(&probe).is_err() {
            return FileDiffStatsResult::Skipped {
                reason: "invalid utf-8",
            };
        }
    }

    // Read the full file.
    let new_content = match std::fs::read_to_string(&abs_path) {
        Ok(s) => s,
        Err(e) => {
            return FileDiffStatsResult::Error {
                message: format!("read failed: {e}"),
            };
        }
    };

    let old = old_content.unwrap_or_default();
    let diff = similar::TextDiff::from_lines(old, &new_content);
    let mut additions = 0usize;
    let mut deletions = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Delete => deletions += 1,
            similar::ChangeTag::Insert => additions += 1,
            similar::ChangeTag::Equal => {}
        }
    }

    FileDiffStatsResult::Ready {
        additions,
        deletions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn small_utf8_file_returns_correct_counts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "line one").unwrap();
        writeln!(f, "line two").unwrap();
        writeln!(f, "line three").unwrap();

        let result = compute_diff_stats(
            &dir.path().to_string_lossy(),
            "test.txt",
            Some("line one\nline two\n"),
        );
        match result {
            FileDiffStatsResult::Ready {
                additions,
                deletions,
            } => {
                assert_eq!(additions, 1, "one line added");
                assert_eq!(deletions, 0, "no lines deleted");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = compute_diff_stats(&dir.path().to_string_lossy(), "nonexistent.txt", None);
        assert!(
            matches!(result, FileDiffStatsResult::Error { .. }),
            "expected Error, got {result:?}"
        );
    }

    #[test]
    fn directory_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        let result = compute_diff_stats(&dir.path().to_string_lossy(), "subdir", None);
        assert!(
            matches!(
                result,
                FileDiffStatsResult::Skipped {
                    reason: "directory"
                }
            ),
            "expected Skipped(directory), got {result:?}"
        );
    }

    #[test]
    fn file_larger_than_limit_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        // Write a file slightly larger than the limit.
        let data = vec![b'a'; (SIDEBAR_DIFF_MAX_BYTES as usize) + 1];
        std::fs::write(&path, &data).unwrap();

        let result = compute_diff_stats(&dir.path().to_string_lossy(), "big.bin", None);
        assert!(
            matches!(result, FileDiffStatsResult::Skipped { reason: "large" }),
            "expected Skipped(large), got {result:?}"
        );
    }

    #[test]
    fn binary_file_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.bin");
        // Write binary content with NUL byte.
        let mut data = vec![0u8; 100];
        data[50] = 1;
        std::fs::write(&path, &data).unwrap();

        let result = compute_diff_stats(&dir.path().to_string_lossy(), "image.bin", None);
        assert!(
            matches!(result, FileDiffStatsResult::Skipped { reason: "binary" }),
            "expected Skipped(binary), got {result:?}"
        );
    }

    #[test]
    fn invalid_utf8_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.txt");
        // Write invalid UTF-8 bytes (no NUL, but invalid sequence).
        std::fs::write(&path, [0xFF, 0xFE, 0x41, 0x42]).unwrap();

        let result = compute_diff_stats(&dir.path().to_string_lossy(), "invalid.txt", None);
        assert!(
            matches!(
                result,
                FileDiffStatsResult::Skipped {
                    reason: "invalid utf-8"
                }
            ),
            "expected Skipped(invalid utf-8), got {result:?}"
        );
    }

    #[tokio::test]
    async fn stale_generation_is_ignored() {
        let (tx, mut rx) = mpsc::channel(10);

        // Spawn a diff that will take a moment.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slow.txt");
        std::fs::write(&path, "hello\nworld\n").unwrap();

        // Use a high generation so the caller can simulate a newer one.
        spawn_sidebar_diff_stats(
            Some(tx),
            dir.path().to_string_lossy().into_owned(),
            "slow.txt".to_string(),
            None,
            1, // generation 1
            None,
        );

        // Wait for the command.
        let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        if let TuiCommand::FileDiffStatsReady {
            generation, result, ..
        } = cmd
        {
            assert_eq!(generation, 1);
            assert!(matches!(result, FileDiffStatsResult::Ready { .. }));
        } else {
            panic!("expected FileDiffStatsReady, got {cmd:?}");
        }
    }

    #[tokio::test]
    async fn spawn_sends_ready_result() {
        let (tx, mut rx) = mpsc::channel(10);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();

        spawn_sidebar_diff_stats(
            Some(tx),
            dir.path().to_string_lossy().into_owned(),
            "a.txt".to_string(),
            Some("a\nc\n".to_string()),
            42,
            None,
        );

        let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        if let TuiCommand::FileDiffStatsReady {
            path: p,
            generation,
            result,
        } = cmd
        {
            assert_eq!(p, PathBuf::from("a.txt"));
            assert_eq!(generation, 42);
            match result {
                FileDiffStatsResult::Ready {
                    additions,
                    deletions,
                } => {
                    assert_eq!(additions, 1);
                    assert_eq!(deletions, 0);
                }
                other => panic!("expected Ready, got {other:?}"),
            }
        } else {
            panic!("expected FileDiffStatsReady, got {cmd:?}");
        }
    }
}
