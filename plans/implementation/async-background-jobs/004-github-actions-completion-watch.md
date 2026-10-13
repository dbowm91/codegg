# Async Background Jobs M004 — GitHub Actions CI Completion Watching

Status: blocked — M003 must close
Baseline: `e6a5e85de632ea5057d59aee585942d10f9d950b` (revalidate before implementation)
Roadmap: `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m004--github-actions-remote-ci-terminal-watcher`
Class: capability. Hard dependency: M003 durable event/continuation policy; interface dependency: existing project GitHub/credential/client HTTP and authorization contracts. Soft dependency: M002 shell UX; not required for GitHub polling.

## Objective

Provide a canonical daemon-owned `watch_ci`/equivalent operation that registers one remote GitHub Actions workflow run or commit/check-suite identity, returns a stable **watch handle**, observes completion without repeated model `gh run view`/sleep/loop calls, and reports a single actionable terminal result/attention event through M001/M003. The model may keep working or end the turn under explicit continuation policy.

This is *not* a remote process execution job, nor a `cargo build`-style resource reservation for the full CI duration. Remote GitHub observation should not occupy a CPU/process permit for 20 minutes.

## Prior evidence / live seams to inspect

- `crates/codegg-core/src/jobs/{mod,schedule,schedule_store}.rs` and `src/scheduler/{scheduler,submission}.rs`: one canonical scheduling/reconcile authority, missed-run/overlap/occurrence/idempotency behavior.
- `src/core/daemon_jobs.rs`: authorized Job protocol and status/wait endpoints.
- GitHub and credential wiring, existing HTTP client/TLS, project repository remote/commit provenance, `architecture/authorization.md`, `src/agent/turn_runtime.rs`, session events/projections, M001/M003 notification/recovery.
- Do not assume a GitHub connector in ChatGPT exists in Codegg runtime; use Codegg's own configured GitHub API auth or the existing project credential provider, never a hardcoded hosted-service token.

## Identity, lifecycle and security

Freeze a typed watch target: canonical provider `github_actions`, GitHub owner/repo ID, run ID (preferred exact identity) OR accepted immutable commit SHA + precisely defined check selector, repository/commit binding, authenticated workspace/project/session owner, generation/time window, endpoint host and optional credential reference. Ambiguous multiple runs/check suites MUST not be reported as a successful unique target; either return choices or report `ambiguous`.

Explicit watch states: `pending | observing | completed_success | completed_failure | cancelled | expired | needs_attention` (names flexible). GitHub `queued` / `in_progress` are *nonterminal*; `completed` + conclusion maps all GitHub terminal conclusions (success, failure, cancelled, timed_out, action_required, neutral, skipped, stale, startup_failure, unknown) to typed outcomes and never conflates workflow transport failure with CI failure. A watch timeout is expiration/attention, not remote job cancellation. Cancellation stops **watching**, not the GitHub workflow unless separately authorized.

Use only allowlisted GitHub API base URLs from daemon configuration and canonical repo identity, TLS verification, credential refs resolved at dispatch, strict redirect/SSRF rejection and bounded request/response sizes. Never accept arbitrary model-supplied callback URL, shell snippet or token. Project/repo access and server host allowlist govern inspect/cancel. Surface secrets neither in logs nor durable JSON. GitHub API output is data, not model instruction. Check SHA/run mismatch or repo rename before declaring success.

A poll is a *short finite scheduled observation*, using the existing `ScheduleStore` / `JobSubmissionService` as the only durable timer/executor authority and a narrow low-resource network-read operation; do not keep a scheduler attempt running for the full CI duration. One active occurrence per watch; cap watches per principal/project, budget requests per host, honor 403/429 rate-limit/Retry-After and ETag/If-None-Match where supported, exponential bounded backoff with jitter persisted or deterministically regenerated. Zero pollers on idle watches; no per-watch Tokio interval, `gh run watch` process or agent-authored loops. Do not represent a perpetual recurrence as an active CPU process job.

## Ordered work packages

A. **Contract/negative-first fixtures.** Identify what GitHub API path and token transport already exists. Freeze a narrow `CiWatchTarget`, creation/subscription idempotency, status/projection/control response and authorized resume policy. Cover workflow run ID and optional SHA/check selector; explicitly decline ambiguous, untrusted or unsupported providers. Mock GitHub REST API in process/loopback; no public-network CI.

B. **Durable watch record and observation scheduling.** Persist typed watch identity, state, next_check_at, last_etag, attempt budget, expiry, owner, final remote conclusion, immutable last observed SHA/run. If current `ScheduleStore` can materialize finite repeated observations safely without clutter or overlap, use it; if not, add only a narrowly typed watch schedule/record adapter that delegates each due attempt to the *existing* scheduler. Document migration and restart/reconciliation. No second timer loop or queue.

C. **GitHub adapter and credentials.** Validate canonical GitHub URLs and project remote association; use an existing trusted HTTP/TLS/credential path and least-privilege GitHub read token. Respect endpoint error codes, pagination, JSON size and rate limits. Fail closed on credential unavailable, scope/repo mismatch, 404 visibility ambiguity, malformed response or unsupported GitHub Enterprise host.

D. **Terminal reconciliation / notification.** On an authoritative `completed` CI state persist final result then generate one M001 notification; M003 handles idle `notify_only` and authorized `resume_owner`. Duplicate responses, daemon restarts and race with user cancel must not issue multiple notices or recheck terminal targets. Nonterminal updates may update projections, not model wakeups. Include stable GitHub run URL only after validation, bounded check/failing-step summaries, source SHA and status references. A failed API call cannot become 'CI failed'.

E. **Model-facing affordance and operator UX.** One small discoverable tool/action, e.g. `ci_watch { repo, run_id, on_complete }`, with `status|cancel`; name chosen after existing tool-catalog audit. Automatic adoption of arbitrary `gh` output is out of scope; explicit user/agent-supplied run ID only, optionally deterministic commit/run resolution after verifying uniqueness. Prompt guidance explains that background watcher notifies on completion rather than requiring repeated tool calls. TUI projects same bounded watch status and cancellation.

F. **Docs and closure.** Update `architecture/jobs.md`, scheduling/authorization/projection docs, config guidance for credentials, and roadmap/registry. Require recorded host/resource limits and a negative/uncertain state where capability cannot be established.

## Acceptance criteria and required evidence

- Fake GitHub run progresses queued -> in_progress -> completed/failure/success; one terminal event and optional one authorized continuation. Model makes no periodic CI calls.
- Two watchers for the same accepted invocation coalesce; two independent authorized watches retain distinct ownership. Restart, overlapping timers, duplicated API response, cancellation-before-terminal, terminal-before-creation, 429 rate limiting and 503 backoff are deterministic.
- A dormant watcher holds *no long-lived process or CPU permit*; network observations are separately admitted and bounded. Watch quota/rate limit and TTL prevent persistent uncontrolled load on low-powered SBC hosts.
- 401/403/404, repo mismatch, checked-out branch SHA drift, invalid run id, ambiguous check, GitHub Enterprise redirect, SSRF target and stolen/guessed watch handle do not leak data or produce false CI success. A GitHub conclusion of `neutral` or `skipped` cannot be labeled 'passed' by default.
- Watch cancellation does not cancel remote workflow. External event URLs are verified and rendered as inert user links, not followed as instructions.
- Existing Tool Program/AgentRun/scheduler notification suites stay green. Tests entirely use local HTTP fixtures; no new CI job, credential-provisioning service, webhook listener or GitHub-hosted live matrix.

Run focused new CI-watch lifecycle tests and existing scheduler schedule/restart, notification/continuation/authorization/projection suites, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `scripts/verify.sh quick`, `python3 scripts/check_scheduler_bypass.py`, `python3 scripts/check_execution_ownership.py`. Record platform testing limits; do not assert hosted API qualification without actual evidence.

Stop if implementation requires storing plaintext credentials, arbitrary network targets, permanent long-running pollers, conflating transient API error with failed CI, or fabricated project/commit identity. Closure: `plans/closure/async-background-jobs/004-status.md`.
