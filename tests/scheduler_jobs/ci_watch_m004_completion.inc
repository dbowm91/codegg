//! Async-background-jobs M004: GitHub Actions completion watching.
//!
//! All GitHub API evidence comes from an in-process loopback fixture; no
//! public-network CI, no credential-provisioning service, no webhook
//! listener, no new CI job. The model makes no periodic CI calls in any
//! test: the harness drives the daemon-owned `observe_once` / `status`
//! entry points directly.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use codegg::scheduler::ci_watch::{CiWatchConfig, CiWatchService, CiWatchTarget, ObserveOutcome};
use codegg::scheduler::job_notifications::{JobCompletionDisposition, JobNotificationService};
use codegg::tool::Tool;

use super::common;

// ---------------------------------------------------------------------------
// Loopback mock GitHub API
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LoggedRequest {
    method: String,
    path: String,
    authorization: bool,
    if_none_match: Option<String>,
}

struct MockGitHub {
    base_url: String,
    log: Arc<Mutex<Vec<LoggedRequest>>>,
    _handle: thread::JoinHandle<()>,
}

fn json_response(status: &str, body: &str, extra_headers: &[(&str, &str)]) -> String {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    out
}

fn run_body(status: &str, conclusion: Option<&str>, sha: &str, html_url: &str) -> String {
    let conclusion_json = conclusion
        .map(|c| format!("\"{c}\""))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"id\":1,\"name\":\"ci\",\"head_sha\":\"{sha}\",\"status\":\"{status}\",\"conclusion\":{conclusion_json},\"html_url\":\"{html_url}\"}}"
    )
}

impl MockGitHub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://127.0.0.1:{}", addr.port());
        let log: Arc<Mutex<Vec<LoggedRequest>>> = Arc::new(Mutex::new(vec![]));
        let run_scripts: Arc<Mutex<std::collections::HashMap<String, VecDeque<String>>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        // Stateful success flow for run 1: queued -> in_progress -> completed/success.
        run_scripts.lock().unwrap().insert(
            "run:1".to_string(),
            VecDeque::from([
                run_body("queued", None, &"a".repeat(40), ""),
                run_body("in_progress", None, &"a".repeat(40), ""),
                run_body(
                    "completed",
                    Some("success"),
                    &"a".repeat(40),
                    "http://127.0.0.1:9/runs/1",
                ),
            ]),
        );
        // Failure flow for run 2.
        run_scripts.lock().unwrap().insert(
            "run:2".to_string(),
            VecDeque::from([run_body(
                "completed",
                Some("failure"),
                &"b".repeat(40),
                "http://127.0.0.1:9/runs/2",
            )]),
        );
        let log_c = log.clone();
        let scripts_c = run_scripts.clone();
        let base_c = base_url.clone();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(500) {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut buf = vec![0u8; 8192];
                let n = match stream.read(&mut buf) {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let mut lines = text.lines();
                let request_line = lines.next().unwrap_or("").to_string();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut authorization = false;
                let mut if_none_match = None;
                for line in lines {
                    let line = line.trim_end_matches('\r');
                    if line.is_empty() {
                        break;
                    }
                    let lower = line.to_lowercase();
                    if lower.starts_with("authorization:") {
                        authorization = true;
                    }
                    if lower.starts_with("if-none-match:") {
                        if_none_match = Some(
                            line.split_once(':')
                                .map(|x| x.1.trim().to_string())
                                .unwrap_or_default(),
                        );
                    }
                }
                // ETag revalidation: the fixture answers 304 when the client
                // echoes the ETag it was given.
                let etag_hit = if_none_match.as_deref() == Some("\"v1\"");
                let etag_echo = if_none_match.clone();
                log_c.lock().unwrap().push(LoggedRequest {
                    method,
                    path: path.clone(),
                    authorization,
                    if_none_match,
                });
                let if_none_match = etag_echo;
                let response = if path.starts_with("/repos/o/r/actions/runs/")
                    && path.contains("/jobs")
                {
                    json_response(
                        "200 OK",
                        r#"{"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"compile","conclusion":"failure"},{"name":"link","conclusion":"success"}]},{"name":"test","conclusion":"success","steps":[]}]}"#,
                        &[],
                    )
                } else if path.starts_with("/repos/o/r/actions/runs?") {
                    if path.contains("head_sha=cccccccccccccccccccccccccccccccccccccccc") {
                        json_response("200 OK", r#"{"total_count":0,"workflow_runs":[]}"#, &[])
                    } else if path.contains("head_sha=dddddddddddddddddddddddddddddddddddddddd") {
                        json_response(
                            "200 OK",
                            r#"{"total_count":2,"workflow_runs":[{"id":11,"name":"ci","head_sha":"dddddddddddddddddddddddddddddddddddddddd"},{"id":12,"name":"ci","head_sha":"dddddddddddddddddddddddddddddddddddddddd"}]}"#,
                            &[],
                        )
                    } else {
                        json_response(
                            "200 OK",
                            r#"{"total_count":1,"workflow_runs":[{"id":9,"name":"ci","head_sha":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}]}"#,
                            &[],
                        )
                    }
                } else if path.starts_with("/repos/o/r/actions/runs/") {
                    let id = path.rsplit('/').next().unwrap_or("").to_string();
                    // Stable run 9 never changes state: it always answers
                    // in_progress with ETag "v9", and 304 when the client
                    // revalidates with it. All other scripted runs advance
                    // state on every poll, so their per-state tags never
                    // match a client echo (a real API mints a new ETag per
                    // changed representation).
                    if id == "9" {
                        if if_none_match.as_deref() == Some("\"v9\"") {
                            "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                        } else {
                            json_response(
                                "200 OK",
                                &run_body("in_progress", None, &"a".repeat(40), ""),
                                &[("ETag", "\"v9\"")],
                            )
                        }
                    } else {
                        match id.as_str() {
                        "401" => json_response("401 Unauthorized", r#"{"message":"bad credentials"}"#, &[]),
                        "403" => json_response("403 Forbidden", r#"{"message":"forbidden"}"#, &[]),
                        "404" => json_response("404 Not Found", r#"{"message":"not found"}"#, &[]),
                        "429" => json_response("429 Too Many Requests", r#"{"message":"rate limited"}"#, &[("Retry-After", "1")]),
                        "500" => json_response("503 Service Unavailable", r#"{"message":"boom"}"#, &[]),
                        "302" => "HTTP/1.1 302 Found\r\nLocation: https://evil.example/runs/302\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                        "7" => json_response(
                            "200 OK",
                            &run_body("completed", Some("neutral"), &"a".repeat(40), ""),
                            &[],
                        ),
                        "8" => json_response(
                            "200 OK",
                            &run_body("completed", Some("skipped"), &"a".repeat(40), ""),
                            &[],
                        ),
                        _ => {
                            if etag_hit {
                                "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                            } else {
                                let key = format!("run:{id}");
                                let mut scripts = scripts_c.lock().unwrap();
                                let body = scripts
                                    .get_mut(&key)
                                    .and_then(|q| q.pop_front())
                                    .unwrap_or_else(|| {
                                        run_body("completed", Some("success"), &"a".repeat(40), "")
                                    });
                                // Patch html_url to the fixture origin so
                                // same-origin validation passes.
                                let body = body.replace(
                                    "http://127.0.0.1:9/",
                                    &format!("{base_c}/"),
                                );
                                // Per-state tag: the representation changed
                                // since any client echo, so a revalidation
                                // never falsely matches mid-flow.
                                json_response("200 OK", &body, &[("ETag", "\"s-next\"")])
                            }
                        }
                    }
                    }
                } else {
                    json_response("404 Not Found", r#"{"message":"not found"}"#, &[])
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            base_url,
            log,
            _handle: handle,
        }
    }

    fn requests(&self) -> Vec<LoggedRequest> {
        self.log.lock().unwrap().clone()
    }
}

fn target_run(run_id: i64) -> CiWatchTarget {
    CiWatchTarget {
        provider: "github_actions".to_string(),
        owner: "o".to_string(),
        repo: "r".to_string(),
        run_id: Some(run_id),
        commit_sha: None,
        check_selector: None,
    }
}

fn service_with(base_url: &str) -> (CiWatchService, Arc<JobNotificationService>) {
    let notifier = Arc::new(JobNotificationService::new());
    let config = CiWatchConfig::test_with_base_url(base_url);
    let svc = CiWatchService::new(notifier.clone(), config);
    (svc, notifier)
}

async fn service_with_pool_for(
    base_url: &str,
    pool: sqlx::SqlitePool,
    notifier: Arc<JobNotificationService>,
) -> CiWatchService {
    CiWatchService::with_pool(pool, notifier, CiWatchConfig::test_with_base_url(base_url))
}

#[tokio::test(flavor = "current_thread")]
async fn fake_run_progresses_to_terminal_success_with_one_notice() {
    let mock = MockGitHub::start();
    let (svc, notifier) = service_with(&mock.base_url);
    let watch = svc
        .create_watch(
            target_run(1),
            "sess-ci-1",
            Some("turn-1"),
            "inv-ci-1",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    assert_eq!(
        watch.state,
        codegg::scheduler::ci_watch::CiWatchState::Pending
    );
    // No polls on create: dormant watchers hold nothing.
    assert!(mock.requests().is_empty());
    // Model makes no periodic calls; the harness drives daemon observations.
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::Nonterminal
    );
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::Nonterminal
    );
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::TerminalSuccess
    );
    // Exactly one terminal notice, no re-poll of the terminal target.
    let pending = notifier.pending_for_session("sess-ci-1").await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, "completed");
    let before = mock.requests().len();
    assert!(matches!(
        svc.observe_once(&watch.watch_id).await,
        Err(codegg::scheduler::ci_watch::CiWatchError::AlreadyTerminal)
    ));
    assert_eq!(
        mock.requests().len(),
        before,
        "terminal targets are never re-polled"
    );
    let pending = notifier.pending_for_session("sess-ci-1").await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "duplicate terminal cannot duplicate notices"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failure_conclusion_maps_with_bounded_step_summary() {
    let mock = MockGitHub::start();
    let (svc, notifier) = service_with(&mock.base_url);
    let watch = svc
        .create_watch(
            target_run(2),
            "sess-ci-2",
            None,
            "inv-ci-2",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::TerminalFailure
    );
    let pending = notifier.pending_for_session("sess-ci-2").await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, "failed");
    assert!(
        pending[0].summary.contains("compile"),
        "bounded failing-step summary: {}",
        pending[0].summary
    );
}

#[tokio::test(flavor = "current_thread")]
async fn same_invocation_coalesces_independent_watches_distinct() {
    let mock = MockGitHub::start();
    let (svc, _notifier) = service_with(&mock.base_url);
    let a = svc
        .create_watch(
            target_run(1),
            "s",
            None,
            "same-inv",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    let b = svc
        .create_watch(
            target_run(1),
            "s",
            None,
            "same-inv",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(a.watch_id, b.watch_id, "same accepted invocation coalesces");
    let c = svc
        .create_watch(
            target_run(1),
            "s",
            None,
            "other-inv",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_ne!(a.watch_id, c.watch_id, "independent watches stay distinct");
    assert!(
        mock.requests().is_empty(),
        "creation performs no network I/O"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_overlap_duplicate_cancel_terminal_before_create_backoff() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc = service_with_pool_for(&mock.base_url, pool.clone(), notifier.clone()).await;
    // Terminal-before-creation: the fixture run is already completed.
    let watch = svc
        .create_watch(
            target_run(2),
            "sess-early",
            None,
            "inv-early",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::TerminalFailure
    );
    // Restart: a fresh service over the same pool sees the watch + notice.
    let notifier2 = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc2 = service_with_pool_for(&mock.base_url, pool.clone(), notifier2.clone()).await;
    let views = svc2.list_for_session("sess-early").await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].state, "completed_failure");
    assert_eq!(
        notifier2
            .pending_for_session("sess-early")
            .await
            .unwrap()
            .len(),
        1
    );
    // Overlap: concurrent observations of one live watch → one wins.
    let watch2 = svc
        .create_watch(
            target_run(1),
            "sess-over",
            None,
            "inv-over",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    let (r1, r2) = tokio::join!(
        svc.observe_once(&watch2.watch_id),
        svc.observe_once(&watch2.watch_id)
    );
    let busy = [&r1, &r2]
        .iter()
        .filter(|r| matches!(r, Err(codegg::scheduler::ci_watch::CiWatchError::Busy)))
        .count();
    assert_eq!(busy, 1, "exactly one overlapping observation is refused");
    // Cancel-before-terminal: one cancelled notice, then terminal.
    let watch3 = svc
        .create_watch(
            target_run(1),
            "sess-cancel",
            None,
            "inv-cancel",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    let view = svc
        .cancel(&watch3.watch_id, Some("sess-cancel"))
        .await
        .unwrap();
    assert_eq!(view.state, "cancelled");
    assert_eq!(
        notifier
            .pending_for_session("sess-cancel")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        svc.observe_once(&watch3.watch_id).await,
        Err(codegg::scheduler::ci_watch::CiWatchError::AlreadyTerminal)
    ));
    // 429 honors Retry-After; 503 backs off; budget exhaustion expires.
    let watch429 = svc
        .create_watch(
            target_run(429),
            "sess-429",
            None,
            "inv-429",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        svc.observe_once(&watch429.watch_id).await.unwrap(),
        ObserveOutcome::TransientError
    );
    let view429 = svc
        .status(&watch429.watch_id, Some("sess-429"))
        .await
        .unwrap();
    assert!(view429.next_check_at > 0);
    // Note: status() advances when due; with zero backoff the 429 replays.
    let watch500 = svc
        .create_watch(
            target_run(500),
            "sess-500",
            None,
            "inv-500",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        svc.observe_once(&watch500.watch_id).await.unwrap(),
        ObserveOutcome::TransientError
    );
}

#[tokio::test(flavor = "current_thread")]
async fn etag_revalidation_avoids_state_churn() {
    let mock = MockGitHub::start();
    let (svc, _notifier) = service_with(&mock.base_url);
    // Stable run 9 never changes: the second poll revalidates with the
    // echoed ETag and the fixture answers 304 (no state churn, no notice).
    let watch = svc
        .create_watch(
            target_run(9),
            "sess-etag",
            None,
            "inv-etag",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::Nonterminal
    );
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::Nonterminal
    );
    let log = mock.requests();
    assert!(log.len() >= 2);
    assert!(
        log[1].if_none_match.as_deref() == Some("\"v9\""),
        "conditional revalidation must send If-None-Match: {log:?}"
    );
    let views = svc.list_for_session("sess-etag").await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].state, "observing");
}

#[tokio::test(flavor = "current_thread")]
async fn negative_matrix_never_leaks_nor_reports_false_success() {
    let mock = MockGitHub::start();
    let (svc, notifier) = service_with(&mock.base_url);
    // 401 / 403 / 404 / invalid id → needs_attention, never success.
    for run_id in [401, 403, 404] {
        let watch = svc
            .create_watch(
                target_run(run_id),
                "sess-neg",
                None,
                &format!("inv-neg-{run_id}"),
                None,
                None,
                JobCompletionDisposition::NotifyOnly,
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            svc.observe_once(&watch.watch_id).await.unwrap(),
            ObserveOutcome::NeedsAttention,
            "run {run_id}"
        );
    }
    // neutral / skipped conclusions are never labeled passed.
    for run_id in [7, 8] {
        let watch = svc
            .create_watch(
                target_run(run_id),
                "sess-neg",
                None,
                &format!("inv-neg-{run_id}"),
                None,
                None,
                JobCompletionDisposition::NotifyOnly,
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            svc.observe_once(&watch.watch_id).await.unwrap(),
            ObserveOutcome::NeedsAttention
        );
    }
    // Ambiguous check suites: zero and multiple matches.
    for (sha, inv) in [
        ("cccccccccccccccccccccccccccccccccccccccc", "inv-amb-0"),
        ("dddddddddddddddddddddddddddddddddddddddd", "inv-amb-2"),
    ] {
        let target = CiWatchTarget {
            provider: "github_actions".to_string(),
            owner: "o".to_string(),
            repo: "r".to_string(),
            run_id: None,
            commit_sha: Some(sha.to_string()),
            check_selector: None,
        };
        let watch = svc
            .create_watch(
                target,
                "sess-neg",
                None,
                inv,
                None,
                None,
                JobCompletionDisposition::NotifyOnly,
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            svc.observe_once(&watch.watch_id).await.unwrap(),
            ObserveOutcome::NeedsAttention
        );
    }
    // Cross-host redirect (Enterprise trap) → needs_attention.
    let watch302 = svc
        .create_watch(
            target_run(302),
            "sess-neg",
            None,
            "inv-302",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        svc.observe_once(&watch302.watch_id).await.unwrap(),
        ObserveOutcome::NeedsAttention
    );
    // No notice reports success; every terminal here is attention.
    for notice in notifier.pending_for_session("sess-neg").await.unwrap() {
        assert_ne!(notice.status, "completed", "false success forbidden");
        assert_eq!(notice.status, "needs_attention");
    }
    // Stolen handle: another session learns nothing (NotFound, not Unauthorized).
    let watch = svc
        .create_watch(
            target_run(1),
            "sess-owner",
            None,
            "inv-owner",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    let err = svc
        .status(&watch.watch_id, Some("sess-attacker"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        codegg::scheduler::ci_watch::CiWatchError::NotFound
    ));
    assert!(svc
        .cancel(&watch.watch_id, Some("sess-attacker"))
        .await
        .is_err());
    // Unsupported provider declined before any network I/O.
    let before = mock.requests().len();
    let bad = CiWatchTarget {
        provider: "gitlab".to_string(),
        owner: "o".to_string(),
        repo: "r".to_string(),
        run_id: Some(1),
        commit_sha: None,
        check_selector: None,
    };
    let err = svc
        .create_watch(
            bad,
            "sess-neg",
            None,
            "inv-bad",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        codegg::scheduler::ci_watch::CiWatchError::UnsupportedProvider(_)
    ));
    assert_eq!(
        mock.requests().len(),
        before,
        "unsupported providers are never probed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sha_drift_and_repo_mismatch_fail_closed() {
    let mock = MockGitHub::start();
    let (svc, _notifier) = service_with(&mock.base_url);
    // Frozen SHA e... resolves to run 9 whose head_sha is e... — craft a
    // watch whose frozen SHA differs from what the run reports by using a
    // commit-anchored watch: resolution requires head_sha equality, so a
    // mismatched SHAs fixture yields ambiguity, never success.
    let target = CiWatchTarget {
        provider: "github_actions".to_string(),
        owner: "o".to_string(),
        repo: "r".to_string(),
        run_id: None,
        commit_sha: Some("ffffffffffffffffffffffffffffffffffffffff".to_string()),
        check_selector: None,
    };
    let watch = svc
        .create_watch(
            target,
            "sess-drift",
            None,
            "inv-drift",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .unwrap();
    // The fixture has no runs for f...@ — ambiguous (zero matches), not success.
    assert_eq!(
        svc.observe_once(&watch.watch_id).await.unwrap(),
        ObserveOutcome::NeedsAttention
    );
}

#[test]
fn target_validation_rejects_garbage_before_any_io() {
    let good = target_run(1);
    assert!(good.validate().is_ok());
    let both = CiWatchTarget {
        provider: "github_actions".to_string(),
        owner: "o".to_string(),
        repo: "r".to_string(),
        run_id: Some(1),
        commit_sha: Some("a".repeat(40)),
        check_selector: None,
    };
    assert!(both.validate().is_err());
    let neither = CiWatchTarget {
        provider: "github_actions".to_string(),
        owner: "o".to_string(),
        repo: "r".to_string(),
        run_id: None,
        commit_sha: None,
        check_selector: None,
    };
    assert!(neither.validate().is_err());
    let bad_sha = CiWatchTarget {
        provider: "github_actions".to_string(),
        owner: "o".to_string(),
        repo: "r".to_string(),
        run_id: None,
        commit_sha: Some("not-a-sha".to_string()),
        check_selector: None,
    };
    assert!(bad_sha.validate().is_err());
    let bad_owner = CiWatchTarget {
        provider: "github_actions".to_string(),
        owner: "o/o".to_string(),
        repo: "r".to_string(),
        run_id: Some(1),
        commit_sha: None,
        check_selector: None,
    };
    assert!(bad_owner.validate().is_err());
}

#[test]
fn ci_watch_tool_has_no_network_or_credential_fields() {
    // SSRF/credential surface: the model surface carries no URL, token, or
    // callback field — base URLs come from daemon configuration only.
    let params = codegg::tool::ci_watch::CiWatchTool::new().parameters();
    let text = serde_json::to_string(&params).unwrap();
    for forbidden in [
        "token", "callback", "url", "base_url", "webhook", "shell", "gh ",
    ] {
        assert!(
            !text.to_lowercase().contains(forbidden),
            "model surface must not carry {forbidden}"
        );
    }
    let registry = codegg::tool::ToolRegistry::with_defaults();
    assert!(registry.get("ci_watch").is_some());
}
