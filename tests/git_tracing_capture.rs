//! D4: Tracing capture tests for the Git subsystem.
//!
//! Pins the contract added by the corrective security closure pass:
//! Codegg-owned `git` subprocess paths must not emit credential-bearing
//! URLs through `tracing::*` events. The two known `tracing::warn!`
//! call sites in `git_run_store.rs` carry only RunStore backend
//! errors (no URLs), but a regression test guards against future
//! leaks.

#![allow(clippy::needless_borrow)]

use std::process::Command;
use std::sync::{Arc, Mutex};

use codegg::git_mutations::{GitEnvPolicy, GitMutationExecutor};
use codegg::git_network_ops;
use tempfile::TempDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

mod common;

#[derive(Default, Clone)]
struct CapturedEvents {
    events: Arc<Mutex<Vec<String>>>,
}

impl CapturedEvents {
    fn drain(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().expect("lock"))
    }
}

/// A `tracing_subscriber::Layer` that records every formatted event
/// into a shared `Vec<String>` for offline assertion.
struct CaptureLayer {
    sink: CapturedEvents,
}

impl<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>> Layer<S>
    for CaptureLayer
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut buf = String::new();
        // Use a one-off visitor to format the event message verbatim.
        struct Visitor<'a>(&'a mut String);
        impl<'a> tracing::field::Visit for Visitor<'a> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={:?} ", field.name(), value);
            }
        }
        event.record(&mut Visitor(&mut buf));
        self.sink.events.lock().expect("lock").push(buf);
    }
}

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(argv: &[&str], cwd: &std::path::Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(argv)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    cmd
}

fn init_repo(dir: &std::path::Path) {
    run_git(&["init", "-q", "-b", "main"], dir)
        .status()
        .expect("init");
    std::fs::write(dir.join("README.md"), "hello\n").expect("write");
    run_git(&["add", "README.md"], dir).status().expect("add");
    run_git(&["commit", "-q", "-m", "initial"], dir)
        .status()
        .expect("commit");
}

fn executor() -> GitMutationExecutor {
    GitMutationExecutor::new()
        .with_env_policy(GitEnvPolicy::default())
        .with_timeout(std::time::Duration::from_secs(15))
}

/// Install a per-test `tracing` subscriber that captures events into
/// `sink` for the rest of the test's lifetime. Returns the sink so
/// the caller can drain it after the async work.
fn install_captured_tracing() -> CapturedEvents {
    let sink = CapturedEvents::default();
    let layer = CaptureLayer { sink: sink.clone() };
    let _ = tracing_subscriber::registry().with(layer).try_init();
    sink
}

#[tokio::test(flavor = "current_thread")]
async fn tracing_does_not_emit_credential_url_on_remote_add_success() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let sentinel = common::secret_scan::unique_sentinel("tracing_add");
    let dir = TempDir::new().expect("tempdir");
    init_repo(dir.path());
    let exec = executor();
    let sink = install_captured_tracing();
    let url = format!("https://u:{sentinel}@private.example.com/r.git");
    let _ = git_network_ops::remote_add(&exec, dir.path(), "private", &url).await;
    let events = sink.drain();
    let combined: String = events.join("\n");
    assert!(
        !combined.contains(&sentinel),
        "tracing emitted credential sentinel: {combined}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tracing_does_not_emit_credential_url_on_remote_add_failure() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let sentinel = common::secret_scan::unique_sentinel("tracing_add_fail");
    let dir = TempDir::new().expect("tempdir");
    init_repo(dir.path());
    let exec = executor();
    let sink = install_captured_tracing();
    let url = format!("https://u:{sentinel}@private.example.com/r.git");
    // Add once (succeeds), then a second time with the same name
    // (fails). The failure path is what we want to verify against.
    let _ = git_network_ops::remote_add(&exec, dir.path(), "tracing_test_target", &url).await;
    let _ = git_network_ops::remote_add(&exec, dir.path(), "tracing_test_target", &url).await;
    let events = sink.drain();
    let combined: String = events.join("\n");
    assert!(
        !combined.contains(&sentinel),
        "tracing emitted credential sentinel on failure path: {combined}"
    );
}
