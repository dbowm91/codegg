# Async Background Jobs Corrective C003 — Full Idle Lifecycle and Integration Qualification

Status: ready for handoff
Baseline: `8e155498e39118b913c47a7b37a4769223ea1c02` (C001 closed at `21b35c79`, C002 closed at `8e155498`; revalidate latest corrective head before work).
Roadmap: `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`
Historical milestone evidence: M001–M004 closures `plans/closure/async-background-jobs/001-status.md` through `004-status.md`.
Class: correctness/qualification, no generalized product expansion.

## 1. Objective

Verify as a **single unit** the promised mcode-like flow: asynchronous compilation/tests and external GitHub Actions watch run independently of the model; completion is persistently observable to the owning user while idle; optionally, a truly user-authorized single model continuation starts without an unrelated new user turn. Identify and correct only defects exposed by whole-lifecycle verification. Reconcile roadmap/registry/closure claims to exactly what the evidence proves.

C001 establishes daemon-scoped scheduling/notifications and durable watch registration. C002 establishes opt-in autonomous **normal** owner continuation. Previous M001–M004 closure tests covered local isolated pieces; M004 directly called `observe_once` and M003's continuation test checked markers/CAS, not idle provider dispatch. This gap in the verification *harness boundary*, not an invitation for a new E2E framework, is what C003 addresses.

## 2. Evidence matrix — must reproduce on implementation HEAD

| Journey | Required observation |
|---|---|
| Background `test` + `bash` | Submit via real ToolBroker/turn, leave owner idle, scheduler reaches terminal, one durable session attention/result and stable `JobHandle`, parent can work meanwhile |
| CI watch | Register run via real tool/daemon, parent ends, fake GitHub transitions to completed without *any* status/turn activity, daemon detects terminal within configured due budget, one notice; watcher holds no long-lived process permit |
| Default notify-only | No new provider invocation or model tokens when job/CI ends; notification visible across two clients and after daemon restart without manual status polling |
| Explicit consented resume | Authenticated owner grant + background completion → one newly accepted canonical turn; fake provider enters once, sees bounded trusted summary and preserved permissions; nothing runs without grant |
| Cancel/expiry/revocation | Cancelled owner, archived session, expired grant, budget cap, revoked principal suppress model dispatch but leave actionable notice; CI-watch cancel never cancels remote workflow |
| Crash/race | Lost notifier, shutdown/restart, after watch-insert-before-subscription, after claimed due observation, after terminal-before-notice, after event append-before-ack, after continuation claim/turn accepted/provider started; no duplicate terminal notice, remote polling or provider dispatch; ambiguous provider start becomes attention |
| Security | Model-forged principal/workspace, guessed other watch/job ID, malicious GitHub text, disallowed URL redirect, invalid token, 429/5xx, sandbox Required unavailable, shell policy Ask/Deny all preserve canonical authority |
| Compatibility | M001–M004, Tool Programs M016/M017, AgentRun M009, existing foreground Bash/Test and session/projection/reconnect behavior pass |

## 3. Ordered execution

A. **Pin source and freeze harness.** Record exact branch head and inherited mainline divergence. Re-run existing M001–M004 targeted tests first. Then implement one deterministic fake-provider + SQLite daemon + HTTP loopback *actual tool invocation* trajectory, using current project `tests/` and existing fake providers, not a new standalone runner/framework. A fake clock/timer injection must advance the daemon's own due coordinator; test code must **not** call `observe_once`, `advance_due_watches`, `ci_watch.status` or manually inject a notification after the session ends.

B. **Complete the full lifecycle.** Exercise two independent sessions / two frontends / two concurrent watchers and a long background build/test. Assert no model-driven polling, one end-to-end result per watch/job, bounded output, and stable schema/transaction recovery across daemon restart. Verify genuine active-turn safe-boundary notice and fully idle notice as separate paths. Check UI/projection and session replay output contains no secrets or unauthorized session details.

C. **Qualification and defects.** Run M001/M002/M003/M004 suites, current authorization/command policy/sandbox tests, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` (respect repo's prohibition on `--all-features` sweeping), `scripts/verify.sh quick`, static scheduler-bypass/command-execution ownership guards. Request/run existing hosted `CI / verify` and relevant Desktop E2E on the same exact submitted SHA/PR before strict merged-ready status; capture workflow ID, commit SHA, conclusion and required jobs. Never assert hosted success from local green output. No new permanent CI workflow or ad hoc live network gate.

D. **Security and operational negative evidence.** Use local API fixtures for GitHub authentication failure/SSRF/ETag/429/Retry-After/500, forged session/turn, shell policy Ask and Deny, required sandbox unsupported, note two independent SQLite handles/claimants, crash checkpoints and budget exhaustion. At least one native supported-OS test for installed/daemon lifecycle if runner exists. macOS, Windows, GitHub hosted API and paid live-provider tests remain explicitly **unverified** where infrastructure/credentials are unavailable; do not fabricate green results or indefinitely block a local deterministic safety proof on unavailable optional external evidence.

E. **Corrective-only fixes.** If this sweep reveals one or two bounded defects in C001/C002 implementations, repair with focused regression tests and record any deviation. If defects require another scheduler, cross-platform sandbox claim, auth engine, second live agent runtime or generic orchestration, **stop** and register a new specific corrective; do not silently expand C003.

F. **Status and integration handoff.** Produce `plans/closure/async-background-jobs-corrective/003-status.md` with exact SHA, a table of required/optional tests and real run IDs, security/failure/restart results, high/medium/low findings and any blocked/unverified surfaces, source vs default-branch delta, downstream unblocking and PR/merge readiness. Update **only** the corrective addendum + `plans/registry.md` statuses, leaving original M001–M004 history unchanged. If passing on the corrective branch but not on `main`, say "implemented/qualified on branch; not merged" rather than "repo shipped".

## 4. Strict pass/fail and stop gates

- **No strict closure** without an idle CI-watch terminal result from a real daemon coordinator, a notify-only zero-inference result, and an explicit-grant idle continuation that starts one normal fake-provider turn. A manual `status` call or new user turn to drive the watch/resume falsifies the positive.
- **No strict closure** with any unbounded poller, two concurrent API observations for one due lease, orphan persisted watch lacking subscriber, cross-session notification, unauthorized extra model turn, duplicate provider invocation, or blind replay after ambiguous provider start.
- **No production-ready/merge-ready claim** without exact-head green required hosted CI, or an explicit written unavailability/blocker and a separate integration decision. Existing closed work may remain historically closed while corrective is blocked; never rewrite its prior evidence.
- **No expanded Windows security assurance:** Windows native sandbox remains its own independently blocked security line.
- If the canonical runtime cannot support unattended model starts safely, close any provably corrected notify-only slice separately and keep the resume requirement **blocked**, not falsely closed.

Document no new long-lived operational loop except the single daemon-owned bounded due coordinator accepted in C001. Archive or close the corrective only when every required row above passes and the registry references the accepted C003 closure.
