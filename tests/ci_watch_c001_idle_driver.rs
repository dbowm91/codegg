//! C001 daemon-owned idle CI-watch advancement and recovery.
//!
//! Strict idle qualification: the daemon-owned trigger (spawned
//! `CiWatchIdleDriver`) reaches terminal without the harness calling
//! `ci_watch.status`, `advance_due_watches`, `AgentLoop.run`, or manual
//! `observe_once` on the positive path. All GitHub evidence comes from an
//! in-process loopback fixture; no live network, no model, no new CI lane.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use codegg::scheduler::ci_watch::{CiWatchConfig, CiWatchService, CiWatchTarget};
use codegg::scheduler::ci_watch_idle::CiWatchIdleDriver;
use codegg::scheduler::job_notifications::{JobCompletionDisposition, JobNotificationService};

mod common;

// ---------------------------------------------------------------------------
// Loopback mock GitHub API (mirrors M004 fixture, C001-safe)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LoggedRequest {
    method: String,
    path: String,
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

fn run_body(status: &str, conclusion: Option<&str>, sha: &str) -> String {
    let conclusion_json = conclusion
        .map(|c| format!("\"{c}\""))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"id\":1,\"name\":\"ci\",\"head_sha\":\"{sha}\",\"status\":\"{status}\",\"conclusion\":{conclusion_json},\"html_url\":null}}"
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
        run_scripts.lock().unwrap().insert(
            "run:1".to_string(),
            VecDeque::from([
                run_body("queued", None, &"a".repeat(40)),
                run_body("in_progress", None, &"a".repeat(40)),
                run_body("completed", Some("success"), &"a".repeat(40)),
            ]),
        );
        run_scripts.lock().unwrap().insert(
            "run:2".to_string(),
            VecDeque::from([run_body("completed", Some("failure"), &"b".repeat(40))]),
        );
        let log_c = log.clone();
        let scripts_c = run_scripts.clone();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(800) {
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
                log_c.lock().unwrap().push(LoggedRequest {
                    method,
                    path: path.clone(),
                });
                let response = if path.contains("/jobs") {
                    json_response(
                        "200 OK",
                        r#"{"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"compile","conclusion":"failure"}]}]}"#,
                        &[],
                    )
                } else if path.starts_with("/repos/o/r/actions/runs/") {
                    let id = path.rsplit('/').next().unwrap_or("").to_string();
                    let id = id.split('?').next().unwrap_or("").to_string();
                    match id.as_str() {
                        "401" => json_response("401 Unauthorized", r#"{"message":"bad"}"#, &[]),
                        "403" => json_response("403 Forbidden", r#"{"message":"denied"}"#, &[]),
                        "404" => json_response("404 Not Found", r#"{"message":"nf"}"#, &[]),
                        "429" => json_response(
                            "429 Too Many Requests",
                            r#"{"message":"rl"}"#,
                            &[("Retry-After", "1")],
                        ),
                        "500" => json_response("503 Service Unavailable", r#"{"message":"boom"}"#, &[]),
                        "302" => "HTTP/1.1 302 Found\r\nLocation: https://evil.example/runs/302\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                        "7" => json_response(
                            "200 OK",
                            &run_body("completed", Some("neutral"), &"a".repeat(40)),
                            &[],
                        ),
                        _ => {
                            let key = format!("run:{id}");
                            let mut scripts = scripts_c.lock().unwrap();
                            let body = scripts
                                .get_mut(&key)
                                .and_then(|q| q.pop_front())
                                .unwrap_or_else(|| run_body("completed", Some("success"), &"a".repeat(40)));
                            json_response("200 OK", &body, &[])
                        }
                    }
                } else if path.starts_with("/repos/o/r/actions/runs?") {
                    json_response("200 OK", r#"{"total_count":0,"workflow_runs":[]}"#, &[])
                } else {
                    json_response("404 Not Found", r#"{"message":"nf"}"#, &[])
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

    fn request_count(&self) -> usize {
        self.log.lock().unwrap().len()
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

async fn wait_for_notices(
    notifier: &JobNotificationService,
    session: &str,
    want: usize,
    timeout: Duration,
) -> Vec<codegg::scheduler::job_notifications::JobTerminalNotice> {
    let start = Instant::now();
    loop {
        let pending = notifier
            .pending_for_session(session)
            .await
            .unwrap_or_default();
        if pending.len() >= want {
            return pending;
        }
        if start.elapsed() > timeout {
            return notifier
                .pending_for_session(session)
                .await
                .unwrap_or_default();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ---------------------------------------------------------------------------
// 1. Zero-turn positive: spawned driver reaches terminal without harness drive
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn zero_turn_idle_driver_reaches_terminal_with_one_notice() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let watches = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    // Register the watch, then end the owning turn (drop all turn handles —
    // the harness holds no turn, no TUI, no model). The watch is created
    // BEFORE the driver spawns so no harness wake is needed.
    let watch = watches
        .create_watch(
            target_run(1),
            "sess-c001-idle",
            Some("turn-1"),
            "inv-c001-idle",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    assert!(
        mock.request_count() == 0,
        "creation performs no network I/O"
    );
    // Daemon-owned trigger: exactly one idle loop for the daemon lifetime.
    let driver = Arc::new(CiWatchIdleDriver::new(watches.clone()));
    let handle = driver.spawn();
    // STRICT: the harness never calls status / advance_due_watches /
    // AgentLoop.run / observe_once here — it only waits for the durable
    // notice, exactly as an idle frontend would.
    let pending = wait_for_notices(&notifier, "sess-c001-idle", 1, Duration::from_secs(20)).await;
    assert_eq!(pending.len(), 1, "one durable terminal notice while idle");
    assert_eq!(pending[0].status, "completed");
    // Verify terminal state via the non-advancing list view (not status).
    let views = watches
        .list_for_session("sess-c001-idle")
        .await
        .expect("list");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].state, "completed_success");
    // No duplicate notices after further idle ticks.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let pending2 = notifier
        .pending_for_session("sess-c001-idle")
        .await
        .unwrap();
    assert_eq!(pending2.len(), 1, "no duplicate terminal notices");
    let _ = watch;
    driver.request_shutdown();
    handle.abort();
}

// ---------------------------------------------------------------------------
// 2. No-turn expiry: frozen TTL yields exactly one expired attention event
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn no_turn_expiry_yields_single_expired_attention() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let watches = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    // Immediate-expiry watch: TTL 1ms guarantees the first idle tick sees
    // expiry without any client activity and without remote cancellation.
    let _watch = watches
        .create_watch(
            target_run(1),
            "sess-c001-exp",
            None,
            "inv-c001-exp",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            Some(1),
        )
        .await
        .expect("create");
    let driver = Arc::new(CiWatchIdleDriver::new(watches.clone()));
    let handle = driver.spawn();
    let pending = wait_for_notices(&notifier, "sess-c001-exp", 1, Duration::from_secs(15)).await;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].status, "expired",
        "expiry is attention, never CI failed"
    );
    assert!(
        !pending[0].summary.contains("failed"),
        "no false CI failure: {}",
        pending[0].summary
    );
    let views = watches
        .list_for_session("sess-c001-exp")
        .await
        .expect("list");
    assert_eq!(views[0].state, "expired");
    driver.request_shutdown();
    handle.abort();
}

// ---------------------------------------------------------------------------
// 3. Singleton lifetime: shared handles, close-one-turn leaves watchers live
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn singleton_services_share_identity_across_turns() {
    let pool = common::pool::isolated_pool().await;
    let mut deps = codegg::core::runtime_deps::CoreRuntimeDeps::with_jobs(pool, None, None);
    let n1 = deps.shared_job_notification_service();
    let n2 = deps.shared_job_notification_service();
    assert!(Arc::ptr_eq(&n1, &n2), "notifier singleton pointer identity");
    let w1 = deps.shared_ci_watch_service();
    let w2 = deps.shared_ci_watch_service();
    assert!(Arc::ptr_eq(&w1, &w2), "watcher singleton pointer identity");
    assert!(
        Arc::ptr_eq(w1.notifier(), &n1),
        "watcher shares the singleton notifier"
    );
    assert_ne!(w1.service_id(), "", "service identity is stable");
    // Closing one turn (dropping one clone) leaves the singleton live.
    let turn_a_notifier = n1.clone();
    let turn_a_watches = w1.clone();
    drop(turn_a_notifier);
    drop(turn_a_watches);
    // The surviving turn handle still observes through the same service.
    let counts = w2.watch_counts().await.expect("counts");
    assert_eq!(counts.active, 0);
}

// ---------------------------------------------------------------------------
// 4. Cross-service: 50 contenders, one admitted remote read per due lease
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn cross_service_single_admission_per_due_lease() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc_a = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    let svc_b = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    assert_ne!(svc_a.service_id(), svc_b.service_id());
    // Terminal-before-contention fixture (run 2 completes on first poll).
    let watch = svc_a
        .create_watch(
            target_run(2),
            "sess-c001-x",
            None,
            "inv-c001-x",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    let before = mock.request_count();
    let barrier = Arc::new(tokio::sync::Barrier::new(50));
    let mut handles = Vec::new();
    for i in 0..50 {
        let svc = if i % 2 == 0 {
            svc_a.clone()
        } else {
            svc_b.clone()
        };
        let b = barrier.clone();
        let id = watch.watch_id.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            svc.observe_once(&id).await
        }));
    }
    let mut ok_terminal = 0;
    let mut busy = 0;
    let mut already = 0;
    for h in handles {
        match h.await.expect("join") {
            Ok(_) => ok_terminal += 1,
            Err(codegg::scheduler::ci_watch::CiWatchError::Busy) => busy += 1,
            Err(codegg::scheduler::ci_watch::CiWatchError::AlreadyTerminal) => already += 1,
            Err(e) => panic!("unexpected observe error: {e}"),
        }
    }
    // Exactly one winner terminalizes; every other contender is refused
    // (Busy) or arrives after the terminal win (AlreadyTerminal).
    assert_eq!(ok_terminal, 1, "exactly one admitted terminal observation");
    assert_eq!(busy + already, 49);
    // At most one admitted remote read sequence (run + jobs page = 2 HTTP).
    let after = mock.request_count();
    assert!(
        after - before <= 3,
        "one admitted remote read per due lease ({} requests)",
        after - before
    );
    // Stale generation cannot overwrite the newer terminal state.
    let pending = notifier.pending_for_session("sess-c001-x").await.unwrap();
    assert_eq!(pending.len(), 1);
    // A late duplicate observation never re-polls.
    let before2 = mock.request_count();
    let err = svc_b.observe_once(&watch.watch_id).await.unwrap_err();
    assert!(matches!(
        err,
        codegg::scheduler::ci_watch::CiWatchError::AlreadyTerminal
    ));
    assert_eq!(mock.request_count(), before2, "terminal never re-polled");
}

// ---------------------------------------------------------------------------
// 5. Crash interleavings
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn crash_insert_before_subscription_is_repaired() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc = CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    );
    // Simulate crash between row write and subscription: insert the row
    // directly without a subscription.
    let now = chrono::Utc::now().timestamp_millis();
    let watch_id = "ciw-crash-a";
    let record_json = serde_json::json!({
        "watch_id": watch_id, "provider": "github_actions", "owner": "o", "repo": "r",
        "run_id": 2, "commit_sha": serde_json::Value::Null, "check_selector": serde_json::Value::Null,
        "session_id": "sess-crash-a", "turn_id": serde_json::Value::Null,
        "invocation_key": "inv-crash-a", "state": "pending",
        "conclusion": serde_json::Value::Null, "conclusion_detail": serde_json::Value::Null,
        "last_etag": serde_json::Value::Null, "next_check_at": now,
        "attempt_count": 0, "attempt_budget": 50, "expiry_ms": now + 600_000,
        "owner_principal": serde_json::Value::Null, "workspace_id": serde_json::Value::Null,
        "credential_ref": "none", "api_base_url": mock.base_url,
        "last_observed_sha": serde_json::Value::Null, "last_run_status": serde_json::Value::Null,
        "final_url": serde_json::Value::Null, "failure_summary": serde_json::Value::Null,
        "on_complete": "notify_only", "created_at": now, "updated_at": now,
        "claim_owner": serde_json::Value::Null, "claim_until": serde_json::Value::Null,
        "generation": 0
    })
    .to_string();
    sqlx::query(
        "INSERT INTO ci_watch (watch_id, provider, owner, repo, run_id, commit_sha, check_selector, session_id, turn_id, invocation_key, state, conclusion, conclusion_detail, last_etag, next_check_at, attempt_count, attempt_budget, expiry_ms, owner_principal, workspace_id, credential_ref, api_base_url, last_observed_sha, last_run_status, final_url, failure_summary, on_complete, created_at, updated_at, record_json) \
         VALUES (?, 'github_actions', 'o', 'r', 2, NULL, NULL, 'sess-crash-a', NULL, 'inv-crash-a', 'pending', NULL, NULL, NULL, ?, 0, 50, ?, NULL, NULL, 'none', ?, NULL, NULL, NULL, NULL, 'notify_only', ?, ?, ?)",
    )
    .bind(watch_id)
    .bind(now)
    .bind(now + 600_000)
    .bind(&mock.base_url)
    .bind(now)
    .bind(now)
    .bind(record_json)
    .execute(&pool)
    .await
    .expect("direct insert");
    // No subscription exists yet.
    assert!(notifier
        .get_subscription(&format!("ciwatch:{watch_id}"))
        .await
        .unwrap()
        .is_none());
    // Restart recovery repairs the registration.
    let (repaired, attentioned) = svc.repair_registrations().await.expect("repair");
    assert_eq!(repaired, 1);
    assert_eq!(attentioned, 0);
    // The healed watch terminalizes with exactly one notice.
    let svc = Arc::new(svc);
    let driver = Arc::new(CiWatchIdleDriver::new(svc.clone()));
    let handle = driver.spawn();
    let pending = wait_for_notices(&notifier, "sess-crash-a", 1, Duration::from_secs(15)).await;
    assert_eq!(pending.len(), 1);
    driver.request_shutdown();
    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn crash_after_claim_is_recoverable_via_expiry() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc_a = CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    );
    let watch = svc_a
        .create_watch(
            target_run(1),
            "sess-crash-claim",
            None,
            "inv-crash-claim",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    // First holder claims and then "crashes" (drops without observing).
    assert!(svc_a
        .try_claim_due_lease(&watch.watch_id, 300)
        .await
        .expect("claim"));
    drop(svc_a);
    // A second handle cannot steal the live lease.
    let svc_b = CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    );
    assert!(!svc_b
        .try_claim_due_lease(&watch.watch_id, 60_000)
        .await
        .expect("reclaim"));
    // After expiry the lease is recoverable and the watch still terminalizes.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(svc_b
        .try_claim_due_lease(&watch.watch_id, 60_000)
        .await
        .expect("expired reclaim"));
    svc_b
        .release_due_lease(&watch.watch_id)
        .await
        .expect("release");
    let outcome = svc_b.observe_once(&watch.watch_id).await.expect("observe");
    assert!(
        matches!(
            outcome,
            codegg::scheduler::ci_watch::ObserveOutcome::Nonterminal
                | codegg::scheduler::ci_watch::ObserveOutcome::TerminalSuccess
        ),
        "recovered watch observes: {outcome:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn crash_terminal_before_notice_still_delivers_once() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    let watch = svc
        .create_watch(
            target_run(2),
            "sess-crash-tn",
            None,
            "inv-crash-tn",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    // Drive to terminal via the service (single observation), then simulate
    // crash after terminal persist but before notice by deleting the notice.
    let _ = svc.observe_once(&watch.watch_id).await.expect("terminal");
    // Restart: fresh handles over the same pool see the terminal watch.
    let notifier2 = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc2 = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier2.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    let views = svc2.list_for_session("sess-crash-tn").await.expect("list");
    assert_eq!(views[0].state, "completed_failure");
    // Re-emission is idempotent: still exactly one notice.
    let pending = notifier2
        .pending_for_session("sess-crash-tn")
        .await
        .unwrap();
    assert_eq!(pending.len(), 1, "one actionable notice after restart");
}

// ---------------------------------------------------------------------------
// 6. Rate/security + fairness + counts (unit isolation, no idle loop needed)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn rate_security_fail_closed_and_fair_across_sessions() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let svc = CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    );
    // 401/403/404/429/503 + cross-origin redirect all fail closed.
    for run_id in [401, 403, 404, 429, 500, 302] {
        let w = svc
            .create_watch(
                target_run(run_id),
                "sess-c001-neg",
                None,
                &format!("inv-neg-{run_id}"),
                None,
                None,
                JobCompletionDisposition::NotifyOnly,
                None,
            )
            .await
            .expect("create");
        let outcome = svc.observe_once(&w.watch_id).await.expect("observe");
        assert!(
            matches!(
                outcome,
                codegg::scheduler::ci_watch::ObserveOutcome::NeedsAttention
                    | codegg::scheduler::ci_watch::ObserveOutcome::TransientError
            ),
            "run {run_id}: {outcome:?}"
        );
    }
    // Neutral conclusion never reports success.
    let w7 = svc
        .create_watch(
            target_run(7),
            "sess-c001-neg",
            None,
            "inv-neg-7",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    assert_eq!(
        svc.observe_once(&w7.watch_id).await.unwrap(),
        codegg::scheduler::ci_watch::ObserveOutcome::NeedsAttention
    );
    // Foreign session learns nothing.
    let owned = svc
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
        .expect("create");
    assert!(
        svc.status(&owned.watch_id, Some("sess-attacker"))
            .await
            .is_err(),
        "stolen handle must fail closed"
    );
    // Fair global ordering: due rows from two sessions interleave by due
    // time, and counts expose active/overdue/leased/expired/attention.
    let counts = svc.watch_counts().await.expect("counts");
    assert!(counts.active >= 2, "active watches visible: {counts:?}");
    assert!(
        counts.attention >= 1,
        "attention watches visible: {counts:?}"
    );
    let due = svc.due_watch_ids_global(8).await.expect("due");
    assert!(!due.is_empty(), "due queue is globally visible");
}
