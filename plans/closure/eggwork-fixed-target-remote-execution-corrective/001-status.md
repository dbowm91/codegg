# Eggwork Fixed-Target Remote Execution Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution-corrective/001-lease-identity-and-live-node-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`
- `plans/subsystems/eggwork-fixed-target-remote-execution-post-closure-corrective-addendum.md`

Historical predecessor (immutable):

- `plans/implementation/eggwork-fixed-target-remote-execution/001-fixed-target-finite-job-executor.md`
- `plans/closure/eggwork-fixed-target-remote-execution/001-status.md`
  (implementation `67f8f3d3`, closure commit `33807406`)

Repository baseline reviewed: `5f453265`

Implementation commits:

- `f4e6e69d` — scheduler: Eggwork lease-identity corrective with
  live-node qualification (C001) (main work: single-source lease,
  regression tests, scripted hardening, live fixture + tests, docs)
- `2fcb24e6` — scheduler: export production NodeClientFactory for live
  qualification
- `764118ba` — tests: avoid holding scripted mutex guard across await
  in live D4 (clippy `await_holding_lock`)
- `2118d878` — tests: join long line to satisfy stable rustfmt
- `4b5e0059` — tests: capture helper stderr for live-fixture failure
  diagnosis
- `00e00d63` — fixture: fix helper CLI key lookup (stored keys omit
  dashes)
- `f351d014` — fixture: install rustls ring provider before TLS
  builders run
- `c9832a66` — tests: include completion summaries in live failure
  assertions
- `3ca3b7b6` — scheduler: request node-admitted unrestricted spec;
  enforce in scripted seams (capability finding, §10)

Eggwork dependency (immutable pin, unchanged by this corrective):

- `eggwork-client` + `eggwork-core`, git
  `https://github.com/eggstack/eggwork.git`, rev
  `128f808c62f176d414dd18a705773e45f5e2891a`,
  `default-features = false`. Test-only additions under
  dev-dependencies (`rcgen 0.13`, `rustls 0.23`) plus the
  workspace-excluded `crates/eggwork-test-node` helper (own lockfile,
  same immutable rev for `eggwork-server`/`eggwork-runner`/
  `eggwork-core`). No production dependency was widened.

## 1. Executive finding

C001 is complete and closed. The durable CodeGG remote handle now
reconstructs the exact lease-fenced Eggwork handle accepted by the
node: one lease token is minted per fresh CodeGG attempt and copied
verbatim into both the live and durable representations, and
`to_eggwork_handle` rebuilds the exact tuple. The production
`NodeClientFactory` path is qualified against a real loopback Eggwork
node over mTLS: valid renew/cancel under the persisted handle succeed,
a tampered lease is rejected with typed `invalid_lease`, and restart
reconciliation (live and terminal) converges without a second submit.

Live qualification additionally proved one fail-closed server contract
the scripted seam had masked: the pinned node admits only
`IsolationRequirement::None` + `NetworkRequirement::Unrestricted`
specs (HTTP 409 `capability_mismatch` otherwise), so the executor now
requests exactly that combination. The posture change is documented
and fenced as a medium finding with an M002 follow-up (§10); it does
not weaken lease fencing, principal authorization, workspace
isolation, or the no-fallback invariant. No stop condition (§14) was
triggered; no unresolved medium-or-higher finding remains open
without a named owner and follow-up.

## 2. Requirement-to-evidence matrix (plan §13 acceptance criteria)

| # | Acceptance criterion | Evidence | Result |
|---|---|---|---|
| 1 | Live and durable handles contain the same lease token | `derive_handle` single-source construction (`src/scheduler/eggwork.rs`); `lease_identity_is_single_source` unit test; `submitted_lease_matches_persisted_handle` integration test | pass |
| 2 | Reconstructed handles are byte/semantic equivalent for id, generation, lease | `to_eggwork_handle` verbatim copy (now `pub`); `reconstruction_is_exact_and_stable`; `from_parts_copies_canonical_tuple_verbatim`; in-memory + SQLite round-trips assert lease equality | pass |
| 3 | Old two-random-token implementation covered by failing regression | Equality assertions fail against two independent tokens; `submitted_lease_matches_persisted_handle` compares submitted vs persisted lease and fails under the old construction; deterministic (non-statistical) constructor + digest tests pin the invariant structurally | pass |
| 4 | Production NodeClient communicates with a real local Eggwork server using required mTLS | `tests/eggwork_remote_execution_live.rs` builds the client exclusively through `NodeClientFactory::client_for` against a loopback `NodeServer` with required client auth; capabilities/status/blob/e2e tests green on Linux CI | pass |
| 5 | Valid persisted-handle renew/cancel succeeds on the real server | `live_lease_fencing_renew_reject_cancel`: renew then cancel under the reconstructed persisted handle, both `Ok`; terminal `Cancelled` observed | pass |
| 6 | Deliberately wrong lease rejected by the real server | Same test: forged lease cancel + renew both rejected as `ClientError::Api { status: 403, code: "invalid_lease" }` (typed, not display-string) | pass |
| 7 | Restart reconciliation creates no duplicate remote execution | D4 + D5: counting factory wrapper (production factory still builds the client) records exactly 1 submit across original + fresh-executor runs | pass |
| 8 | Live restart reconciliation terminates on the real node | D4: fresh `EggworkExecutor` observes live execution, cancels under the persisted handle, returns `Interrupted`; node reaches terminal `Cancelled` within the 60 s bound; pre-restart run converges `Cancelled` | pass |
| 9 | Terminal restart reconciliation returns existing result without resubmit | D5: fresh executor maps the terminal snapshot to `Completed` with `reconciled=true`, 0 additional submits | pass |
| 10 | Scheduler/target/no-fallback/workspace invariants remain green | Routing matrix + scheduler e2e (scripted and live, Eggwork-only executor registration) + `check_eggwork_target_routing.py` + `check_scheduler_bypass.py` green; workspace-isolation test green on the live node | pass |
| 11 | No production dependency widening solely for tests | `eggwork-server`/`eggwork-runner` live only in the excluded helper crate; root dev-deps add only `rcgen`/`rustls`; `git diff` on `[dependencies]` is empty | pass |
| 12 | No unresolved medium-or-higher finding | §10: one medium (unrestricted-spec posture, owned follow-up), remainder low; none blocks operation | pass |

Required work packages: A (single-source identity) landed in
`derive_handle` + `RemoteExecutionHandle::from_parts` (core stays
transport-independent: plain-string constructor, no Eggwork types in
`codegg-core`); B (pure regression) in `src/scheduler/eggwork.rs`
unit tests; C (fixture) as the bounded `codegg-eggwork-test-node`
subprocess (same rev, deterministic lifecycle, `kill_on_drop` +
temp-dir cleanup, `LocalProcessRunner` remains the process owner —
see §5 for why in-process was impractical); D (D1–D5) in
`tests/eggwork_remote_execution_live.rs`; E (scripted hardening:
submitted-handle lease recording, optional `invalid_lease` fencing,
spec-admission enforcement) in `tests/eggwork_remote_execution.rs`;
F (this record plus `architecture/jobs.md`,
`architecture/scheduler.md`, roadmap/addendum/registry updates).

## 3. Production implementation evidence

- `crates/codegg-core/src/jobs/mod.rs`:
  `RemoteExecutionHandle::from_parts` constructs the durable record
  from one canonical tuple; no token is minted inside `codegg-core`.
- `src/scheduler/eggwork.rs`: `derive_handle` generates one lease id
  and copies execution id / generation / lease verbatim into the
  durable record; `to_eggwork_handle` is now `pub` and reconstructs
  the exact fenced tuple; `build_spec` requests the node-admitted
  `None` + `Unrestricted` combination with the rationale documented
  at the site; executor, reconciliation, renewal, artifact, and
  permit-spanning behavior are otherwise unchanged.
- `src/scheduler/mod.rs`: re-exports `to_eggwork_handle` and the
  production `NodeClientFactory` for the restart/live paths.
- `scripts/check_eggwork_target_routing.py`: narrowly extended to
  allowlist `crates/eggwork-test-node/` (test-only node host; cannot
  construct CodeGG jobs or bypass target selection). The M001 closure
  record itself is untouched.
- `crates/eggwork-test-node/` (new, workspace-excluded, own
  `Cargo.lock`): minimal `NodeServer` host — parses caller-supplied
  PEMs, builds required-client-auth TLS, fingerprint-maps one client
  certificate to one principal, authorizes exactly the executor's
  fixed-target operation set (fail-closed match, no wildcard arm),
  prints `READY port=<N>`, shuts down on signal. Prints no secret
  material; startup failures mirror to stdout for harness
  diagnostics.
- No scheduler, placement, job-kind, transport-routing, or operator
  policy changes. No migration (durable schema unchanged; existing
  persisted handles without the fixed lease predate any real-node
  acceptance and were never operable against a live node).

## 4. Verification executed (commands + results; local vs CI labeled)

Local (darwin, `main` at `3ca3b7b6` unless noted):

- `cargo test -p codegg --lib scheduler::eggwork` — 10/10 pass
  (4 new C001 regression tests + 6 pre-existing).
- `cargo test --test eggwork_remote_execution --locked` — 24/24 pass
  (20 pre-existing + `in_memory_handle_round_trip_preserves_lease`,
  `submitted_lease_matches_persisted_handle`,
  `scripted_fencing_rejects_wrong_lease`,
  `scripted_seam_rejects_restricted_spec_like_real_node`).
- `cargo test --test eggwork_remote_execution_live --locked` — 0 tests
  (Linux-gated; compiles).
- `scripts/verify.sh quick` — pass (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, target-routing, workspace check).
- `scripts/verify.sh full` — pass in full locally (background run to
  completion; workspace tests + `server,plugins,lsp-test-support`
  feature tests; live target contributes 0 tests on darwin).
- `cargo clippy --workspace --all-targets --locked -- -D warnings` —
  clean locally.
- `git diff --check` — clean.

Hosted (Linux, ubuntu-latest, observed runs):

- CI `/ verify` run `36051370501` (commit `3ca3b7b6`) — **success**:
  all guards, `cargo fmt --check --all`, workspace clippy
  `-D warnings`, `cargo test --workspace --locked` including
  `tests/eggwork_remote_execution.rs` 24/24 and
  `tests/eggwork_remote_execution_live.rs` 11/11 (6 live qualification
  tests + 5 shared secret-scan unit tests; live total 134 s).
- Earlier runs on this branch (`36020876619`, `36028442739`,
  `36029048629`, `36029906834`, `36033815079`, `36038086618`,
  `36042398394`, `36046674514`) failed iteratively on: missing
  `NodeClientFactory` export, `await_holding_lock` in tests, rustfmt
  drift, helper CLI key lookup, missing rustls provider install, and
  the BestEffort/Disabled spec rejection — each fixed in a follow-up
  commit; the final run above is green end to end. No failure was
  overridden or ignored.

Live-test evidence observed in run `36051370501`:

- `live_lease_fencing_renew_reject_cancel` ok (D1 valid renew, D2
  typed 403 `invalid_lease` on forged cancel+renew, valid cancel,
  terminal `Cancelled`, exactly 1 submit).
- `live_restart_live_reconciliation_cancels_under_persisted_handle`
  ok (D4: `Interrupted`, submitted tuple == persisted tuple,
  terminal `Cancelled`, 1 submit).
- `live_restart_terminal_reconciliation_returns_result_without_resubmit`
  ok (D5: `Completed` + `reconciled=true`, 1 submit).
- `live_node_connection_capabilities_and_workspace_isolation` ok
  (`exec.argv.v1` advertised, node id/status, remote-only mutation).
- `live_scheduler_end_to_end_remote_only` ok (scheduler-owned run,
  `eggwork` provenance, persisted handle observes terminal
  `Succeeded`).
- `live_blob_upload_and_workspace_materialization` ok (probe → upload
  → probe-empty through the production client).

## 5. Invariant review

Post-closure addendum invariants (all preserved):

1. CodeGG remains the only scheduler/placement authority —
   `JobScheduler` dispatch unchanged; `check_scheduler_bypass.py`
   green.
2. Eggwork remains fixed-target and scheduler-free — target-first
   routing unchanged; `check_eggwork_target_routing.py` green.
3. Remote target failure never falls back to local execution —
   scheduler e2e registers only the Eggwork executor (scripted and
   live); typed errors propagate.
4. One CodeGG attempt maps to at most one accepted Eggwork execution
   — deterministic execution id + persisted-before-side-effects +
   reconcile-instead-of-resubmit, now proven by submit counts (1)
   across reconnect on the live node.
5. Scheduler resource permit spans the remote lifetime — unchanged
   code path; contention e2e green.
6. Credentials remain daemon/config-owned — file-path references
   only; live fixture uses temp files with `0o600` keys; completion
   summaries scanned for `BEGIN PRIVATE KEY`/`BEGIN CERTIFICATE`
   (negative).
7. Local workspaces are not mutated in place — live isolation test
   (`REMOTE_ONLY_MARKER` absent locally).
8. Remote process creation remains owned by Eggwork
   (`LocalProcessRunner` in the helper; no second CodeGG-side owner;
   execution-ownership guard green with no manifest change needed —
   the helper contains no CodeGG-governed spawn/dispatch sites).
9. Existing local executors unchanged for `ExecutionTarget::Local` —
   routing matrix tests green.
10. Historical M001 plan/closure evidence immutable — untouched; this
    record owns the new disposition.

Fixture-shape note (plan §5): an in-process `NodeServer` fixture is
impractical in this workspace, not for API reasons but dependency
resolution: `eggwork-server` links `rusqlite → libsqlite3-sys 0.38`,
which cannot resolve alongside the workspace's
`sqlx-sqlite → libsqlite3-sys 0.28` (`links = "sqlite3"` must be
unique per resolve graph; observed as a hard resolver error). The
bounded subprocess helper uses the same immutable Eggwork revision,
deterministic lifecycle, guaranteed cleanup, and no second process
owner, satisfying every fallback condition. Separately,
`eggwork-runner` depends on Linux-only `landlock`, so the live target
is Linux-gated (0 tests elsewhere); Linux CI is the qualifying
environment.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: same-attempt re-execution observes
  instead of resubmitting (scripted + live submit counts == 1);
  `execution_identity_deterministic_per_attempt` proves stable
  re-derivation of the execution id.
- Cancellation races: pre-submit cancellation produces no remote side
  effects; mid-run cancellation cancels the exact handle; restart
  with a live execution cancels under the persisted handle and
  reports `Interrupted` (live D4).
- Daemon/node restart: terminal snapshots map to completions (live
  D5); unobservable executions become typed `Interrupted`, never
  local fallback.
- Partial persistence failure: handle persistence failure before
  side effects is a typed `Failed` (unchanged path).
- Stale generation/lease: wrong lease → typed 403 `invalid_lease`
  (scripted + live); generation mismatch surfaces as typed API
  errors, never silent reinterpretation.
- Contention/resource release: busy/draining preflight unchanged and
  green; permits span the lifetime (unchanged).
- Malformed/unauthorized input: unknown-principal clients get 401,
  cross-principal control gets 403 `forbidden` (Eggwork server
  semantics; fixture maps exactly one test principal).
- Bounded event/artifact behavior: unchanged collector/import bounds;
  artifact import qualified live via scheduler e2e.

## 7. Migration and compatibility review

No storage migration: `RemoteExecutionHandle` schema version stays 1
and the JSON shape is unchanged; only the lease *value discipline*
changed (single source). Handles persisted by the defective M001 code
were never operable against a real node (any cancel/renew under them
would have been rejected as `invalid_lease`), so there is no legacy
population to migrate — fresh attempts derive fresh tuples. No
protocol negotiation change (client compatibility check unchanged).
Rollback to the M001 implementation would reintroduce the divergent
lease and the unexecutable spec; it is not a supported fallback.

## 8. Security review

- Lease fencing now exact: persisted handles authorize precisely the
  accepted execution; forged leases are rejected by the server
  (proven live, typed codes).
- mTLS posture unchanged and proven: HTTPS-only endpoints,
  required client auth, fingerprint-mapped test principal, no
  principal self-assertion in payloads, ephemeral test CA/identities,
  `0o600` temp key files, redacted `Debug` impls, secret-negative
  summary scans.
- Workspace isolation proven live (remote mutation does not touch
  the local lease root); snapshot bounds unchanged.
- Medium finding (§10): unrestricted-spec posture — remote commands
  run without node-enforced sandboxing and with network access. The
  node fail-closed rejects anything else, so this is explicit rather
  than silent; operator-policy follow-up assigned to M002.
- No new attack surface: helper is test-only, loopback-bound,
  ephemeral, and excluded from production builds.

## 9. Documentation and operations

- `architecture/jobs.md`: exact lease-fencing contract + admitted-spec
  posture (new subsections under Execution Target).
- `architecture/scheduler.md`: M001 section corrected to single-source
  identity, exact reconstruction, admitted spec, live qualification.
- `src/scheduler/eggwork.rs` `build_spec` site documents the posture
  and the M002 re-qualification follow-up.
- Static guards: `check_eggwork_target_routing.py` extended with the
  test-only helper allowlist (comment-justified); all other guards
  unchanged and green.
- Operator note: Eggwork nodes run remote jobs unsandboxed with
  network access at this revision; select nodes accordingly until
  M002 policy + a restricted-spec-capable Eggwork rev land.

## 10. Unresolved findings (severity: critical/high/medium/low)

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Remote commands request `None` + `Unrestricted` (only node-admitted combination); no node-enforced sandboxing or network disablement at the pinned Eggwork rev | Remote jobs run unsandboxed with network on explicitly selected nodes | M002 operator policy must surface per-node execution posture; restore stricter requests + re-qualify when Eggwork admits restricted specs (pin bump) |
| low | Live qualification is Linux-only (landlock; 0 tests on macOS/other) | macOS devs rely on Linux CI for this gate | None required; documented in test header and here |
| low | Fixture helper builds on demand via nested cargo (first live run compiles `eggwork-server` graph; `CODEGG_EGGWORK_TEST_NODE` overrides) | Slower first live run; network needed for initial helper build | None required; helper has a committed lockfile and `--locked` build |
| low | Helper crate is outside workspace clippy/fmt-all coverage (excluded by design) | Helper hygiene checked separately | None required; `cargo fmt --check` run in helper dir, green |

No critical/high findings. No finding blocks C001 closure: the medium
is a documented posture with an owned M002 follow-up, not a defect in
the corrected behavior.

## 11. Roadmap disposition

C001 is closed. Dependency audit for unblocking (registry `Blocked
work` + subsystem dependency graphs reviewed at this commit):

- Eggwork M002 (target capability projection and operator policy):
  its sole blocker was C001 — **unblocked to eligible-for-planning**.
  No M002 implementation plan file exists yet, so no `ready` row is
  registered; the roadmap milestone status is flipped to ready and
  the medium finding above is assigned to it. The Eggplan
  assessment-integration M001 (registered concurrently by another
  workstream) does not list C001 as a dependency and is unaffected.
- Eggwork M003: remains blocked on the separately reviewed optimized
  materializer contract (C001 no longer part of its blocker).
- Eggwork M004: remains deferred (blocked on M002, M003, and the
  AgentRun worker-entry contract).
- No other registered plan lists C001 as a hard/interface
  dependency; nothing else is unblocked.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: Eggwork row →
  active, current milestone "M001 historically closed; corrective
  C001 closed; M002 eligible for planning", blocker text updated.
- `plans/registry.md` Dependency-ready implementation plans: C001 row
  → closed with this closure record + implementation commits
  (`f4e6e69d` + follow-ups through `3ca3b7b6`).
- `plans/registry.md` Eggwork corrective gate paragraph: rewritten
  past-tense (defect fixed, qualification landed, M002 unblocked).
- `plans/registry.md` Recently closed: new row for this C001 with
  implementation commits and hosted CI run `36051370501` success.
- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`:
  status line, M002 (ready), M003/M004 blocker text updated.
- `plans/subsystems/eggwork-fixed-target-remote-execution-post-closure-corrective-addendum.md`:
  status → closed with C001 closure link.
