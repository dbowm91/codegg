# Windows Native Sandbox C044 — Closure Status

Status: blocked
Source implementation plan: `plans/implementation/windows-native-sandbox/044-compiled-native-two-hop-launch-diagnostic.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `edde2bebe`
Implementation commits: `edde2bebe` — feat(windows-sandbox): C041 mainline CI/build guard, C042 guarantee contract, C044 two-hop harness, C047 coverage audit (harness only; no Windows execution)

## 1. Executive finding

C044 is blocked after landing its diagnostic harness. The isolated
research harness (`research/windows-native-sandbox/
two-hop-native-diagnostic/`: pinned isolated Cargo project, minimal C
first-instruction probe, `cfg(windows)`-gated runner, portable
manifest/nonce logic, disposable-host RUN.md) is complete and its
Linux-portable tests pass. Windows execution — the milestone's actual
acceptance gate (same-file ordinary-read allowed vs restricted-read
denied with token proof, tool compatibility, stage/host writes, no
descriptor drift, Job cleanup) — did not run: no disposable consented
Windows host exists in this environment. The bounded one-correction
allowance is unconsumed. Per the plan, a child that never reaches
entry proves nothing, and no read-denial is claimed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Isolated Rust 1.89-compatible helper, no product installer | `two-hop-native-diagnostic/Cargo.toml` (`[workspace]`, `publish=false`, pinned `windows =0.62.2` cfg-gated) + `Cargo.lock` | pass | MSRV-neutral; excluded from workspace builds |
| Native first-instruction probe before interpreter loads | `native_probe.c` (entry marker first, then SID/restricting-group + read/write probes; pinned `cl.exe` build documented) | pass (source) | Not compiled or run here (Windows-only C) |
| Immutable manifest + one-shot nonce + minimal descriptors | `src/main.rs` `StageManifest::new/verify`, `entry_path/proof_path` | pass | 4/4 Linux unit tests green |
| Two-hop launch (LogonW + restricted own-token + Job/suspend/resume) | `src/main.rs` `run()` stub + `RUN.md` operator steps | partial | Runner documents exact Win32 sequence; execution not-run |
| Same-file broad-vs-restricted read with token proof | not run | blocked | Requires disposable host |
| Stage/host writes, alias/ADS/reparse, handles/IPC, descendants, cleanup | not run | blocked | Requires disposable host |
| Tool compatibility after positive entry (PS/cmd/Git/Cargo) | not run | blocked | Gated on entry proof |
| No persistent account / host mutation | no host touched | pass | Nothing provisioned, nothing mutated |

## 3. Production implementation evidence

Research-only, isolated from production: new directory
`research/windows-native-sandbox/two-hop-native-diagnostic/` with
`README.md` (protocol/stop conditions), `RUN.md` (consented-VM
runbook), `Cargo.toml`/`Cargo.lock`, `src/main.rs` (portable logic +
gated runner), `native_probe.c`. No changes to `src/`,
`Cargo.toml` workspace, installers, protocols, or docs beyond this
harness. No product installer, no PATH/service mutation.

## 4. Verification executed

### Commands run

```bash
cargo test --manifest-path research/windows-native-sandbox/two-hop-native-diagnostic/Cargo.toml --locked --offline
cargo fmt --check --all
git diff --check
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
```

### Results

Local (darwin, `edde2bebe`): harness 4/4 tests passed
(manifest/nonce/traversal/missing-entry); fmt/diff clean; guards
pass. Windows build (`cl.exe`, harness runner on host) and all
Windows child/matrix/handle/cleanup probes: not-run (no host).
C032 historical negative preserved and not relitigated. No hosted run
id exists for C044 and none is claimed.

## 5. Invariant review

- No PowerShell/.NET/loader confusion: probe emits entry before
  interpreters by construction.
- No durable account, original-user ACEs, or host grants: nothing
  provisioned.
- No production `Required` activation; test-only scaffolding cannot
  report `Enforced` (consistent with C042 `test_only` refusal).
- C032 negative, C011/M004–M006, C015–C018, C029/C034/C038
  no-admission unchanged.

## 6. Failure and recovery review

Missing entry is classified by `classify_missing_entry()` as
blocked-before-entry (loader/privilege/environment), never a denial —
tested. No fixtures created, so no cleanup/recovery path was
exercised; RUN.md specifies Job-tree termination, OS process-query
verification, re-hash, and account/profile/stage removal with
negative teardown evidence on the host run.

## 7. Migration and compatibility review

None (research harness, no production surface, no migration).

## 8. Security review

No new authority or privilege path. The harness documents consent,
disposability, and exact teardown; it handles no real credentials or
user worktrees. Host-secret/global-ACL mutation is explicitly
out-of-scope and refused.

## 9. Documentation and operations

`README.md` + `RUN.md` are the operator docs (build pin, evidence
protocol `codegg.two_hop_c044.v1`, stop conditions, teardown). No
architecture-doc change was needed beyond this harness; a future
positive run updates the evidence comparison, never rewrites C032.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No disposable consented Windows host run | Acceptance matrix (reads, writes, handles, cleanup, tools) entirely unproven | Execute RUN.md on a consented VM; consume at most one bounded startup correction, then close positive/negative/blocked |
| low | C probe never compiled (`cl.exe`) | Source-only review | Compile + hash-pin on host run |

## 11. Roadmap disposition

Milestone blocked (harness landed; execution outstanding). It does not
gate C043 (parallel MXC line) or the C047 audit slice (closed via
C042). A future host run closes C044 positive/negative/blocked and,
only if strict-positive plus a new provisioning ADR, makes C045
eligible. No write-only outcome may be promoted to strict profiles
without ADR-0028 acceptance.

## 12. Registry updates

Covered by the single line-closure commit (see C041 §12): C044 row
`ready` → `blocked (harness landed at edde2bebe; Windows execution
not-run; correction unconsumed)`, closure
`plans/closure/windows-native-sandbox/044-status.md`. Implementation
plan `044-*.md` status: `ready` → `blocked (harness landed)`. Unblock
audit: C044 blocks nothing new; C045 remains blocked (still needs a
strict-positive C044 plus a separate provisioning ADR).
