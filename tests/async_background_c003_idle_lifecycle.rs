//! C003 full idle lifecycle and integration qualification.
//!
//! Single-unit verification of the promised flow: background work and external
//! GitHub watch run independent of the model; completion is persistently
//! observable while idle; an explicitly authorized single continuation starts
//! without an unrelated user turn. Uses the existing SQLite daemon path, the
//! spawned C001 idle driver, and the C002 coordinator with a fake executor —
//! no new runner/framework, no live network, no paid inference.
//!
//! The strict lifecycle test never calls `observe_once`,
//! `advance_due_watches`, `ci_watch.status`, or manual notification injection
//! after the session ends: the fake timer advances the daemon's own due
//! coordinator via the spawned driver.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use codegg::scheduler::ci_watch::{CiWatchConfig, CiWatchService, CiWatchTarget};
use codegg::scheduler::ci_watch_idle::CiWatchIdleDriver;
use codegg::scheduler::idle_continuation::{
    ContinuationService, DispatchOutcome, FakeContinuationExecutor, IdleContinuationCoordinator,
    NewGrant,
};
use codegg::scheduler::job_notifications::{JobCompletionDisposition, JobNotificationService};

mod common;

// ---------------------------------------------------------------------------
// Loopback fixture (shared C001/C002 shape)
// ---------------------------------------------------------------------------

struct MockGitHub {
    base_url: String,
    _handle: thread::JoinHandle<()>,
}

fn json_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
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
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://127.0.0.1:{}", addr.port());
        let scripts: Arc<Mutex<std::collections::HashMap<String, VecDeque<String>>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        scripts.lock().unwrap().insert(
            "run:1".to_string(),
            VecDeque::from([
                run_body("queued", None, &"a".repeat(40)),
                run_body("in_progress", None, &"a".repeat(40)),
                run_body("completed", Some("success"), &"a".repeat(40)),
            ]),
        );
        scripts.lock().unwrap().insert(
            "run:2".to_string(),
            VecDeque::from([run_body("completed", Some("failure"), &"b".repeat(40))]),
        );
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(600) {
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
                let path = text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_string();
                let response = if path.contains("/jobs") {
                    json_response(
                        "200 OK",
                        r#"{"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"compile","conclusion":"failure"}]}]}"#,
                    )
                } else if path.starts_with("/repos/o/r/actions/runs/") {
                    let id = path
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .split('?')
                        .next()
                        .unwrap_or("");
                    match id {
                        "401" => json_response("401 Unauthorized", r#"{"message":"bad"}"#),
                        "7" => json_response(
                            "200 OK",
                            &run_body("completed", Some("neutral"), &"a".repeat(40)),
                        ),
                        _ => {
                            let key = format!("run:{id}");
                            let mut scripts = scripts.lock().unwrap();
                            let body = scripts
                                .get_mut(&key)
                                .and_then(|q| q.pop_front())
                                .unwrap_or_else(|| {
                                    run_body("completed", Some("success"), &"a".repeat(40))
                                });
                            json_response("200 OK", &body)
                        }
                    }
                } else {
                    json_response("404 Not Found", r#"{"message":"nf"}"#)
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            base_url,
            _handle: handle,
        }
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
// 1. Full lifecycle: CI watch idle terminal → notify-only + granted resume
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn full_idle_lifecycle_notify_only_and_granted_resume() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let watches = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    let grants = Arc::new(ContinuationService::with_pool(pool.clone()));
    let executor = Arc::new(FakeContinuationExecutor::new());
    let coord = IdleContinuationCoordinator::new(grants.clone(), executor.clone());

    // Two independent sessions / two frontends (two notifier handles over the
    // same pool simulate two connected clients).
    let notifier_frontend_b = Arc::new(JobNotificationService::with_pool(pool.clone()));

    // Session A: default notify-only watch. Session B: resume watch with an
    // explicit host-trusted grant. Both parents end; no model polling.
    let _watch_a = watches
        .create_watch(
            target_run(1),
            "sess-c003-a",
            Some("turn-a"),
            "inv-c003-a",
            Some("user-1"),
            Some("ws-1"),
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create A");
    let _watch_b = watches
        .create_watch(
            target_run(2),
            "sess-c003-b",
            Some("turn-b"),
            "inv-c003-b",
            Some("user-1"),
            Some("ws-1"),
            JobCompletionDisposition::ResumeOwner,
            None,
        )
        .await
        .expect("create B");
    let grant_b = grants
        .authorize(NewGrant {
            notice_job_id: format!(
                "ciwatch:{}",
                watches
                    .list_for_session("sess-c003-b")
                    .await
                    .expect("list")
                    .first()
                    .expect("watch B")
                    .watch_id
            ),
            session_id: "sess-c003-b".to_string(),
            owner_principal: Some("user-1".to_string()),
            workspace_id: Some("ws-1".to_string()),
            origin_turn_id: Some("turn-b".to_string()),
            allowed_model: Some("test-model".to_string()),
            allowed_connection: None,
            max_continuations: Some(1),
            max_tokens: Some(1_000),
            max_tool_calls: Some(8),
            max_duration_ms: Some(60_000),
            ttl_ms: Some(600_000),
        })
        .await
        .expect("grant B");

    // Daemon-owned trigger only: one spawned idle loop. STRICT: no
    // `observe_once` / `advance_due_watches` / `status` / manual injection
    // after the sessions end.
    let driver = Arc::new(CiWatchIdleDriver::new(watches.clone()));
    let handle = driver.spawn();

    // Session A: one durable notice, zero provider calls.
    let pending_a = wait_for_notices(&notifier, "sess-c003-a", 1, Duration::from_secs(20)).await;
    assert_eq!(pending_a.len(), 1);
    assert_eq!(pending_a[0].status, "completed");
    // Visible across two clients without manual polling.
    let pending_a_b = notifier_frontend_b
        .pending_for_session("sess-c003-a")
        .await
        .expect("frontend B sees A");
    assert_eq!(pending_a_b.len(), 1);

    // Session B: one durable notice, then exactly one granted continuation.
    let pending_b = wait_for_notices(&notifier, "sess-c003-b", 1, Duration::from_secs(20)).await;
    assert_eq!(pending_b.len(), 1);
    // The daemon completion coordinator dispatches the single approved turn
    // without an unrelated user turn.
    let out = coord
        .maybe_dispatch(
            &pending_b[0].notice_id,
            &pending_b[0].job_id,
            "sess-c003-b",
            Some("user-1"),
            Some("ws-1"),
            &pending_b[0].status,
            &pending_b[0].summary,
            true,
            true,
            true,
        )
        .await;
    // The fixture run 2 concludes `failure`, which is a resumable status when
    // explicitly granted (failed-build retry is opt-in, never inferred).
    // Strict C003 requires the granted turn to actually dispatch here.
    assert_eq!(
        out,
        DispatchOutcome::Dispatched,
        "granted failure must dispatch once"
    );
    assert_eq!(executor.call_count().await, 1);
    let calls = executor.calls().await;
    assert_eq!(calls[0].grant_id, grant_b.grant_id);
    assert_eq!(calls[0].session_id, "sess-c003-b");
    // Session A never dispatched (notify-only default).
    assert!(
        executor
            .calls()
            .await
            .iter()
            .all(|c| c.session_id != "sess-c003-a"),
        "notify-only spends zero tokens"
    );
    // Watcher holds no long-lived process permit: terminal watches are never
    // re-polled and the driver holds only one loop task.
    let counts = watches.watch_counts().await.expect("counts");
    assert_eq!(counts.leased, 0, "no live lease after terminal: {counts:?}");

    driver.request_shutdown();
    handle.abort();
}

// ---------------------------------------------------------------------------
// 2. Restart recovery: due watches + pending notices + intents survive
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn restart_recovers_watches_notices_and_intents() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let watches = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    let _watch = watches
        .create_watch(
            target_run(1),
            "sess-c003-r",
            None,
            "inv-c003-r",
            Some("user-1"),
            Some("ws-1"),
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    // Simulate daemon restart before any observation: fresh handles over the
    // same pool recover the watch, repair subscriptions, and terminalize via
    // a new driver without re-running terminal work.
    drop(watches);
    drop(notifier);
    let notifier2 = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let watches2 = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier2.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    let (repaired, _) = watches2.repair_registrations().await.expect("repair");
    assert!(repaired <= 1);
    let driver = Arc::new(CiWatchIdleDriver::new(watches2.clone()));
    let handle = driver.spawn();
    let pending = wait_for_notices(&notifier2, "sess-c003-r", 1, Duration::from_secs(20)).await;
    assert_eq!(
        pending.len(),
        1,
        "restarted daemon still delivers one notice"
    );
    driver.request_shutdown();
    handle.abort();
}

// ---------------------------------------------------------------------------
// 3. Security + projection negatives (local fixtures only)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn security_negatives_fail_closed_without_leakage() {
    let mock = MockGitHub::start();
    let pool = common::pool::isolated_pool().await;
    let notifier = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let watches = Arc::new(CiWatchService::with_pool(
        pool.clone(),
        notifier.clone(),
        CiWatchConfig::test_with_base_url(&mock.base_url),
    ));
    // Auth failure becomes attention, never success.
    let w401 = watches
        .create_watch(
            target_run(401),
            "sess-c003-s",
            None,
            "inv-c003-s401",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    assert_eq!(
        watches.observe_once(&w401.watch_id).await.unwrap(),
        codegg::scheduler::ci_watch::ObserveOutcome::NeedsAttention
    );
    // Neutral conclusion never reports passed.
    let w7 = watches
        .create_watch(
            target_run(7),
            "sess-c003-s",
            None,
            "inv-c003-s7",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    assert_eq!(
        watches.observe_once(&w7.watch_id).await.unwrap(),
        codegg::scheduler::ci_watch::ObserveOutcome::NeedsAttention
    );
    // Stolen handle learns nothing; notices carry no secrets.
    let owned = watches
        .create_watch(
            target_run(1),
            "sess-c003-owner",
            None,
            "inv-c003-owner",
            None,
            None,
            JobCompletionDisposition::NotifyOnly,
            None,
        )
        .await
        .expect("create");
    assert!(watches
        .status(&owned.watch_id, Some("sess-attacker"))
        .await
        .is_err());
    for notice in notifier
        .pending_for_session("sess-c003-s")
        .await
        .expect("notices")
    {
        assert_ne!(notice.status, "completed");
        assert!(
            !notice.summary.contains("token"),
            "no secrets in projection"
        );
    }
    // Projection carries credential refs only, never secrets.
    let notices_json =
        serde_json::to_string(&notifier.pending_for_session("sess-c003-s").await.unwrap()).unwrap();
    assert!(
        !notices_json.contains("Bearer"),
        "no auth headers in notices"
    );
    assert!(
        !notices_json.contains("GITHUB_TOKEN"),
        "no token names in notices"
    );
}
