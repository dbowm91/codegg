# Windows Native Sandbox C047 — Closure Status

Status: closed (audit slice only; production integration deferred)
Source implementation plan: `plans/implementation/windows-native-sandbox/047-authoritative-windows-process-tool-coverage.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `edde2bebe`
Implementation commits: `edde2bebe` — feat(windows-sandbox): C041 mainline CI/build guard, C042 guarantee contract, C044 two-hop harness, C047 coverage audit (audit inventory only)

## 1. Executive finding

The C047 audit-safe slice is closed. Every native spawn surface was
inventoried and classified (`research/windows-native-sandbox/
c047-process-tool-coverage.md`): model-untrusted batch execution
already routes through `ManagedProcessService` (bash, scheduler
executors, python, plugins, hooks, formatters, human-shell finite
children, IDE helpers); three surfaces are explicitly uncovered (test
runner, MCP local servers, LSP subprocesses); interactive PTY and
operator helpers are excluded by design; trusted file mutations stay
approval-controlled and unlabeled as sandboxed. Production
restricted-execution integration (package C) and the backend-backed
negative matrix (package D) are explicitly deferred to post-admission
and are NOT claimed. No schema migration, no dispatch change, no
release claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Inventory + classify all spawn surfaces (A) | `c047-process-tool-coverage.md` table (13 rows with files, dispatch, class, rights/env/cwd/identity) | pass | Covers managed_process, bash, scheduler, test runner, interactive, LSP, MCP, plugins, git, python, remote |
| Single launcher contract statement (B) | Same doc §Package B + `architecture/process-tool-execution-ownership.md` C042 paragraph | pass | `ManagedProcessService` confirmed as canonical batch boundary |
| No shell-regex/tool-allowlist substitute (B) | Code review; no such mechanism added | pass | Guards green |
| Real-backend wiring (C) | deferred | not-run (explicit) | Requires admitted backend (none) |
| Negative indirection matrix vs real backend (D) | deferred | not-run (explicit) | Awaits admission + Windows host |
| Trusted tools not mislabeled (D) | Doc statement; no code change | pass | Approval control unchanged |
| No Unix/remote/human-shell regression | No dispatch edits; execution-ownership guard passes | pass | |

## 3. Production implementation evidence

Docs only: new `research/windows-native-sandbox/
c047-process-tool-coverage.md` (inventory, package-B disposition,
explicit deferred C/D, four uncovered items). One paragraph appended
to `architecture/process-tool-execution-ownership.md` (C042 gate
reference; this commit). No `src/` dispatch, storage, protocol,
runtime, frontend, or guard changes for C047. `docs/
execution-ownership.toml` unchanged (no new spawn site introduced).

## 4. Verification executed

### Commands run

```bash
git diff --check
python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_adr_integrity.py
cargo fmt --check --all
cargo check -p codegg --lib --locked
```

### Results

Local (darwin, `edde2bebe`): all guards passed; fmt/diff clean;
`cargo check` exit 0. No Windows execution (not-run by design for the
audit slice). No MCP/LSP/test-runner behavior change to test. The
deferred C/D matrix is recorded as not-run, not as pass.

## 5. Invariant review

- Same OS requirements + process ownership scope documented for all
  untrusted surfaces; no surface gained direct authority.
- Linux/macOS containment, remote-node ownership, human-shell
  promotion, approval gates preserved (no edits).
- No schema migration made or needed for the audit slice.
- C042 contract referenced as the future negotiation gate; no
  `FullHost` fallback, mid-attempt backend switch, or
  agent-controlled helper path introduced.

## 6. Failure and recovery review

No runtime change: no new failure, cancellation, restart, contention,
or idempotency surface. Deferred integration must specify
policy+Job-before-start, no fallback, no mid-attempt switch (already
stated in the plan and the audit doc).

## 7. Migration and compatibility review

None (docs only).

## 8. Security review

No authority change. The audit's value is enumerating the three
bypass surfaces (test runner, MCP, LSP) that MUST be routed or denied
with tests before any coverage claim — explicitly listed so a future
integration cannot silently claim coverage it lacks.

## 9. Documentation and operations

- New audit of record: `research/windows-native-sandbox/
  c047-process-tool-coverage.md`.
- `architecture/process-tool-execution-ownership.md` C042 paragraph
  (this commit).
- No operator diagnostics or recovery instructions changed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Test runner, MCP local servers, LSP subprocesses bypass canonical launcher | Production integration cannot close until each is routed or refused with tests | C048 integration work after backend admission |
| low | Git production-subprocess dispatch needs re-inventory at integration time | Audit snapshot may drift | Re-run inventory when C048 opens |

The medium finding bounds the audit slice (production integration
deferred); it does not invalidate the closed audit.

## 11. Roadmap disposition

Audit slice closed; production integration deferred to post-admission
(C048). C047's hard C042 dependency is satisfied. C047 does not by
itself admit any backend or unblock C048 (which additionally needs a
strict-positive backend, independent review, host matrix, and new
production ADR).

## 12. Registry updates

Covered by the single line-closure commit (see C041 §12): C047 row
`blocked` → `closed (audit slice; production integration deferred)`,
closure `plans/closure/windows-native-sandbox/047-status.md`.
Implementation plan `047-*.md` status: `blocked` → `implemented
(audit slice); production integration deferred`. Unblock audit: C047
audit unblocks nothing new; C048 stays blocked (still needs a
strict-positive backend + review + ADR).
