# Windows Native Sandbox C044 — Closure Status

Status: closed (negative bounded native feasibility)
Source implementation plan: `plans/implementation/windows-native-sandbox/044-compiled-native-two-hop-launch-diagnostic.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `76e9e9c9`
Implementation commits: `f0355e94` (native CI path: probe alias matrix, ps1 runner, workflow, RUN/plan docs), `245b9597` (GITHUB_ENV plumbing), `0362676d` (platform-absolute test dir), `3bd1b469` (LogonUserW diagnostic), `e2deea5e` (aliases via stage file), `c93c3053` (params file), `76e9e9c9` (exit-aware entry poll + LASTEXITCODE guard removal)

## 1. Executive finding

C044 is closed negative. The milestone's hypothesis — C032's PowerShell
inner child never reached entry because of PowerShell/.NET/loader
startup confusion, curable with a tiny compiled native binary — was
fully exercised on a hosted disposable Windows runner and answered:
the statically-linked (`/MT`) native probe, launched via the proven
LogonW → restricted-own-token → Job/suspend/resume topology, was
created, Job-verified before resume, resumed (suspend count 1), and
remained **alive (`STILL_ACTIVE`, 0x103) for the full 30 s window
without writing its first-instruction entry marker**. No token proof,
no same-file read comparison, no stage/host write, no tool
compatibility, and no handle/IPC evidence was obtained. Per the plan,
a child that never reaches entry proves nothing, and no read-denial
is claimed. The one bounded helper-startup correction (exit-aware
entry poll) is consumed; the milestone closes without another loop.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Isolated Rust 1.89-compatible helper, no product installer | `two-hop-native-diagnostic/Cargo.toml` (`[workspace]`, `publish=false`) + `Cargo.lock`; harness 4/4 tests pass on Windows (run `38468468249`) and Linux | pass | MSRV-neutral; excluded from workspace builds |
| Native first-instruction probe before interpreter loads | `native_probe.c` (`/MT /O1 /GS /sdl`, SHA `8234E176…49BF`, cl 19.51.36260.0); entry marker is the first operation in `wmain` | pass (source + build) | Compiled clean under `/W4` on the host |
| Immutable manifest + one-shot nonce + minimal descriptors | Nonce = run GUID; entry/proof paths nonce-bound in stage; runner + probe read-only in immutable; narrow 8-var environment block | pass | Aliases + runner params travel via stage files |
| Two-hop launch (LogonW + restricted own-token + Job/suspend/resume) | Run `38468468249` (exact SHA `76e9e9c9`): first-hop account `…-1003`, `first_hop_suspend_count=1`, Job-verified; `LogonUserW` diagnostic OK; inner child created suspended, Job-verified, suspend 1, resumed | pass | Full topology exercised |
| Entry marker within 30 s | `child_reached_entry=false`, `child_died_before_entry=false`, `child_exit_code=259 (0x103 STILL_ACTIVE)` at window end | fail (decisive) | Alive-yet-never-enters: rules out instant loader death AND interpreter startup as the blocker |
| Same-file broad-vs-restricted read with token proof | not measured (no entry) | negative (absent) | Never counted as denial |
| Stage/host writes, alias/ADS/reparse, handles/IPC, descendants, cleanup | child probes not run; fixture DACLs/hashes byte-identical; account+profile+fixtures removed (`C044_CLEANUP` all true) | not run / pass (teardown) | Teardown proven; probes gated on entry |
| Tool compatibility (cmd/Git/Cargo) after positive entry | `native_shell=not_run`, `git=not_run` (correctly gated) | not run | Entry gate never opened |
| No persistent account / host mutation | `C044_CLEANUP principal_removed=True profile_removed=True fixtures_removed=True`; secret/host/stage DACLs + bytes unchanged | pass | Nothing provisioned beyond the disposable user |

## 3. Production implementation evidence

Research-only, isolated from production: extended `native_probe.c`
(alias matrix `argv[6..]`), new `scripts/
windows_c044_two_hop_native.ps1` (C032 P/Invoke pattern, native inner
child, exit-aware poll, restricted cmd/Git gating), new workflow
`.github/workflows/windows-c044-two-hop-native.yml`
(windows-latest, probe build + hash pin, harness tests, fixture run,
30-day evidence artifacts), updated `README.md`/`RUN.md`, traversal-
test platform fix in `src/main.rs`. No changes to `src/`, workspace
`Cargo.toml`, installers, protocols, or product docs. No product
installer, no PATH/service mutation, no `Required` activation.

## 4. Verification executed

### Hosted runs (exact-SHA, windows-latest Server 2025 Datacenter b26100 UBR33438 X64)

- `38460916120` (head `f0355e94`): plumbing failure — multi-line
  `cl.exe /Bv` output written to `GITHUB_ENV`. Fixed (`245b9597`).
- `38461383244` (head `245b9597`): probe built; harness test
  `manifest_rejects_traversal_probe_names` failed (`/tmp` not
  absolute on Windows). Fixed (`0362676d`).
- `38462019729` / `38462939545`: first-hop `CreateProcessWithLogonW`
  failed `E_INVALIDARG` (-2147024809) with 1448/1028-char command
  lines while `LogonUserW` succeeded and C032's ~975-char equivalent
  worked the same day. Fixed by moving aliases (`e2deea5e`) then all
  runner inputs (`c93c3053`) to stage files (259-char command line).
  Operational note for future work: keep LogonW command lines short;
  exact limit uncharacterized.
- `38463927648` (C032 control re-run, same image pool): first hop OK,
  inner PowerShell child created/Job-verified/resumed, timed out
  before entry — historical negative reproduced exactly.
- `38468468249` (head `76e9e9c9`): **decisive run**, workflow
  `success`. Report `codegg.two_hop_c044.v1`, `outcome=
  blocked-before-entry`: control read allowed, topology fully
  verified, child alive 30 s without entry, clean teardown, stable
  fixtures. Artifacts `windows-c044-two-hop-native-38468468249`
  (report JSON + console, 30-day retention).

### Local (darwin)

```bash
cargo test --manifest-path research/windows-native-sandbox/two-hop-native-diagnostic/Cargo.toml --locked --offline  # 4/4
cargo fmt --check --all; git diff --check
python3 scripts/check_adr_integrity.py; python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py; python3 scripts/check_windows_test_support_release.py
```

All green. C032 historical negative preserved and not relitigated;
C044 answers the strictly narrower native-entry question.

## 5. Invariant review

- No PowerShell/.NET/loader confusion in the verdict: the whole
  point of the native probe was to remove that variable, and the
  alive-without-entry result is interpretable without it.
- No durable account, original-user ACEs, or host grants: disposable
  user + profile removed, fixtures removed, DACLs/hashes unchanged.
- No production `Required` activation; test-only scaffolding cannot
  report `Enforced` (consistent with C042 `test_only` refusal).
- C032 negative, C011/M004–M006, C015–C018, C029/C034/C038
  no-admission unchanged. C043 negative (this line) is independent
  and consistent: both candidate topologies now have hosted
  no-entry/no-candidate evidence.

## 6. Failure and recovery review

Missing entry is classified `blocked-before-entry`, never a denial.
The exit-aware poll distinguishes instant loader death (exit code),
hang (STILL_ACTIVE, observed), and success — the observed class is
hang-without-entry for a static native binary. Teardown (Job-tree
termination via outer `TerminateJobObject` path, account/profile/
fixture removal with negative evidence) is proven by
`C044_CLEANUP`. No fixtures leaked in any of the six hosted runs.

## 7. Migration and compatibility review

None (research harness, no production surface, no migration).

## 8. Security review

No new authority or privilege path. The harness documents consent
(admin-gated disposable runner), disposability (RUNNER_TEMP fixtures,
full teardown), and exact evidence protocol; it handles no real
credentials (random disposable password, zeroed after use) or user
worktrees. Host-secret/global-ACL mutation is out-of-scope and
refused. The alive-without-entry finding is not shaped into a
confinement claim in either direction.

## 9. Documentation and operations

- `scripts/windows_c044_two_hop_native.ps1` (orchestration of record).
- `.github/workflows/windows-c044-two-hop-native.yml` (hosted path of
  record; push + dispatch).
- `RUN.md` §0 (hosted CI runbook) + manual-VM §1 retained.
- `README.md` (layout now describes the ps1-orchestrated path
  honestly; the Rust `run()` stub remains an off-host guard).
- Evidence protocol `codegg.two_hop_c044.v1` with outcome taxonomy
  `strict-positive-candidate | partial-positive | negative |
  blocked-before-entry | control-failed`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Static native child stays alive 30 s under the restricted token without reaching user entry | No token/read/write/tool/handle evidence obtainable in this topology on Server 2025; strict-positive impossible here | None in this line — closed negative; a new topology would need a new plan, not a C044 loop |
| medium | LogonW command-line length sensitivity uncharacterized (fails ~1K, works 259) | Future LogonW-based work must keep command lines short or use param files | Recorded here; params-file pattern is the standing solution |
| low | C probe alias matrix unexercised (entry gate never opened) | Alias routes still unproven in the two-hop topology | Nothing pending — covered by the negative; MXC alias denials stand separately via C033 |

## 11. Roadmap disposition

Milestone closed (negative bounded native feasibility). It does not
gate C043 (parallel MXC line, closed negative this revision) or the
C047 audit slice (closed via C042). **C045 stays blocked**: its hard
gate (strict-positive C044) has now definitively failed closed — no
provisioning ADR may be premised on this topology. C046 stays
blocked (no selected candidate). C048 stays blocked (no
strict-positive backend on either line). No write-only outcome exists
to promote; ADR-0030 acceptance is untouched.

## 12. Registry updates

Covered by the single line-closure commit (see C041 §12): C044 row
`blocked (harness landed; execution not-run)` → `closed (negative
bounded native feasibility)`, closure
`plans/closure/windows-native-sandbox/044-status.md`. Implementation
plan `044-*.md` status: `active` → `implemented (negative bounded
native feasibility)`. Unblock audit: C044 closes nothing downstream;
C045 remains blocked (strict-positive gate failed closed, still needs
a provisioning ADR that no longer has a qualifying backend),
C046/C048 remain blocked. The bounded correction budget is consumed;
no further C044 verification loop is authorized.
