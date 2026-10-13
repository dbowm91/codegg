# Command Authorization M001 C003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/006-m001-developer-workflow-friction-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`

Baseline reviewed and measured: `7b72582c4f01f8a53c7f20ed5dde1bc620f82965`; post-change implementation reviewed at `ee01106f`.

Implementation commit:

- `ee01106f` — qualify routine developer command policy behavior and document the authorization/sandbox boundary.

## 1. Executive finding

C003 is complete as a policy-friction and host-qualification slice. A curated
62-command corpus covering Rust, Python, JavaScript, Go, generic task runners,
workspace cleanup, local Git, wrappers, environment assignments, quoting,
pipelines and substitutions was checked without executing its commands. Under
an already-authorized workspace/action, the post-change PermissionChecker
result was Allow for all 62 commands, with zero Ask and zero Deny outcomes.
Concrete risky targets still Ask under the default checker, and an explicit
`git push` text Deny still denies.

This is policy evidence, not a claim that all 62 invocations were executed or
that command names imply safety. Separately, the Linux helper reported
Landlock ABI 4 and the real sandbox containment suite passed. That backend
confines the filesystem paths it reports, but explicitly does not isolate
network; Unix IPC and inherited open file descriptors are not claimed as
contained. Parent M001 is ready for aggregate closure review; M002 remains
blocked until that parent closure is accepted.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Deterministic 50+ command corpus across everyday development families | `tests/permission.rs::authorized_workspace_development_corpus_has_no_syntax_only_asks` | pass | 62 commands. Policy fixture only; no command is spawned. |
| No syntax-only Ask/Deny with authorized workspace/action | Same test | pass | 62 Allow, 0 Ask, 0 Deny under explicit Bash Allow. |
| Before/after friction comparison against the fresh-plan baseline | Temporary fixture in a detached worktree at `7b72582c`; post fixture at `ee01106f` | pass with scope noted | Baseline found 5/62 built-in review signals and 0 hard blocks in the routine corpus; post corpus has 0 Ask/Deny outcomes in the authorized policy fixture. Baseline signal count is not represented as a measured UI prompt count. |
| Concrete risk remains reviewable | `tests/permission.rs::low_friction_corpus_keeps_risk_review_and_explicit_deny` | pass | Root recursive deletion and `sudo` return Ask; commands are not executed. |
| Explicit user text Deny remains terminal | Same test; existing permission Deny precedence tests | pass | `git push origin main` is denied by configured `bash_deny_patterns`. |
| Approval routing does not duplicate a human request | `tests/approval_router.rs::destructive_ask_in_interactive_emits_one_human_request`; `python3 scripts/check_approval_router.py` | pass | One pending event is observed for the logical Ask. C002 merges advisory reasons into the existing router path. |
| Filesystem enforcement reflects actual obtained backend | `tests/sandbox_containment.rs` | pass | 13/13 passed; the helper status test reports Landlock ABI 4. |
| Actual benign command can run under the obtained Linux filesystem backend | `tests/sandbox_containment.rs::sandboxed_cargo_metadata_uses_workspace_and_scoped_scratch` | pass | This is a separate sandbox fixture, not one of the 62 policy corpus commands. |
| Approval-mode and explicit-denial invariants remain | `tests/approval_router.rs`; `tests/policy_surface_m007.rs` | pass | Interactive/Automatic/Yolo behavior, child ceilings, FullHost warning surface, and Deny precedence retained. |
| User guidance describes scope and unavailable dimensions honestly | `architecture/permission.md`, `architecture/security.md`, `architecture/tool.md`, `docs/tools.md`, `docs/security-semantics.md` | pass | Landlock-only network/IPC limitations and human shell separation are explicit. |

## 3. Corpus and baseline measurement

The 62 routine entries cover:

- Rust format/check/build/test/clippy and `rustc` discovery;
- Python, pytest, uv, Ruff and mypy;
- npm, pnpm, yarn, Go, Make, Just and CMake;
- workspace discovery, local cleanup, editing utilities and Git inspection/mutation;
- `env`, `bash -c`, `sh -c`, quoted substitution, pipes, redirection, chained commands and mixed package/Git workflows.

At baseline `7b72582c`, a temporary unit fixture called the built-in risk
collector and Bash security check for the same 62 commands. It observed five
review signals and zero terminal security blocks. These are classifier-level
signals, not a synthetic UI event count. The post-change fixture sends each
command through `PermissionChecker::check_with_args` with an explicit
already-authorized Bash Allow and observes 62 Allow results. Existing approval
router tests separately verify that one logical Ask publishes exactly one
human request. Because the baseline collector signal and post policy-result
measure different layers, no percentage reduction in human prompts is claimed.

The temporary baseline test was added only in a detached worktree and removed
with that worktree. Its first invocation used an overly restrictive `--exact`
filter and selected zero tests; the corrected invocation selected and passed
the intended single fixture. No baseline command was executed.

## 4. Production implementation evidence

- Added the no-model 62-command PermissionChecker corpus and paired risk/Deny
  controls in `tests/permission.rs`. The fixture measures policy outcomes; it
  does not use executable-name allowlists or spawn candidate commands.
- Expanded permission, security and tool architecture guidance for routine
  work within an already-authorized scope, explicit operator rules, approval
  modes, FullHost, required sandbox failure and actual Landlock limitations.
- Updated `docs/tools.md` to distinguish advisory command-risk findings from
  deterministic typed-input failures, explain saved grant scope, and preserve
  the separate human shell behavior.
- No sandbox profile, tool authority, child lease, Git route, network right or
  credential scope was broadened.

## 5. Verification executed

Host: `Linux deadpool 6.8.0-146-generic #146-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 3 16:12:30 UTC 2026 x86_64`; Rust `1.99.0`, host `x86_64-unknown-linux-gnu`. The helper reported `BackendId("landlock")`, Landlock ABI 4, filesystem allowlist enforcement and `no_new_privs`; its limits included `no network isolation` and inherited open file descriptors.

### Commands run

```bash
# Baseline, detached worktree at 7b72582c
CARGO_TARGET_DIR=<branch-target> cargo test -p codegg --lib temporary_c003_baseline_corpus_counts_heuristics -- --nocapture

# Post-change
cargo test -p codegg --test permission
cargo test -p codegg --test command_routing_adversarial
cargo test -p codegg --test approval_router -- --test-threads=1
cargo test -p codegg --test policy_surface_m007
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_containment helper_reports_enforcement_with_a_real_backend_identity -- --exact --nocapture
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_approval_router.py
python3 scripts/check_policy_surface.py
cargo fmt --all -- --check
git diff --check
scripts/verify.sh quick
```

### Results

- Baseline corpus fixture: 1 passed; 62 commands; 5 built-in review signals;
  0 hard blocks.
- Permission integration suite: 53 passed, 0 failed.
- Command routing adversarial suite: 139 passed, 0 failed.
- Approval router suite: 17 passed, 0 failed.
- Policy surface M007 suite: 14 passed, 0 failed.
- Sandbox containment suite: 13 passed, 0 failed, including actual workspace
  write/read-only/outside-root enforcement fixtures and sandboxed Cargo metadata.
- Exact backend-status test: 1 passed; Landlock ABI 4, no network isolation.
- Sandbox-wiring, approval-router and policy-surface guards passed.
- Formatting, `git diff --check`, and `scripts/verify.sh quick` passed.

All post-change results are local to the implementation revision. No hosted CI
result, macOS/Windows native run, offline networking proof, or Unix IPC denial
proof is claimed. The quick verification was run before a temporary assertion
was briefly tightened to an exact corpus size and then reverted; final test
source matches the passing 53-test permission suite.

## 6. Invariant review

- A workspace/action Allow does not imply FullHost, network, credentials or
  child-scope authority. Approval mode does not change the immutable sandbox
  ceiling.
- Explicit Deny remains ahead of reviewer/Yolo behavior. Concrete risk remains
  reviewable, while benign syntax does not produce a second policy Ask in the
  authorized fixture.
- Saved simple-command grants remain scoped; compound invocations do not
  inherit a first-token grant.
- Required filesystem confinement remains fail-closed. The Linux fixture
  confirms actual Landlock filesystem enforcement and its explicit escapes.
- No credential, route, scheduler, process ownership or cancellation path
  changed in C003.

## 7. Failure and compatibility review

No durable format, daemon protocol or execution lifecycle changed. Permission
storage and signed command-scope compatibility remain covered by existing
policy-surface tests. The temporary baseline measurement did not alter the
branch or persist state. Unsupported network/IPC isolation remains unavailable
and is not transformed into a fallback grant.

## 8. Security review

Command names and syntax only classify potential review signals. A 62/62 Allow
policy result describes the explicitly authorized fixture and does not certify
that the commands are harmless or that they were executed. Native Linux
evidence is limited to Landlock filesystem confinement. Network is unrestricted;
Unix IPC and inherited open file descriptors are not claimed as isolated.
FullHost remains host authority, and the human-owned `!` shell retains its
independent confirmation rules.

## 9. Documentation and operations

- Updated `architecture/permission.md`, `architecture/security.md`,
  `architecture/tool.md` and `docs/tools.md` with routine-scope UX, explicit
  rule behavior, route and sandbox limits.
- Existing `docs/security-semantics.md` carries the C002 distinction between
  invocation filters, review signals and actual enforcement.
- Focused guards and `scripts/verify.sh quick` pass.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Linux Landlock-only execution has no network isolation; Unix IPC isolation is not established | Workspace code can use these host capabilities despite filesystem confinement | Keep them reported unavailable/unrestricted. Continue the Linux execution-isolation native enforcement line before making denial claims. |
| low | The 62-command corpus measures policy decisions but does not execute each language/task runner under an end-to-end agent session | It proves zero syntax-only PermissionChecker prompts for the authorized fixture, not every frontend/runtime trajectory | Use existing sandbox fixtures and later M002/M003 routing/effect tests for route-specific claims; do not claim all corpus commands were executed. |
| low | Native macOS and Windows friction/enforcement were not exercised here | Platform-specific defaults may differ | Rely only on portable policy tests until native host evidence is available. |

No high-severity C003 finding remains. Medium network/IPC limitations are
pre-existing and explicitly remain open in the Linux execution-isolation plan
line.

## 11. Roadmap disposition

C003 is closed. C001, C002 and C003 are now closed, so parent M001 may enter
aggregate closure review against its complete blocker inventory and original
acceptance. M002 remains blocked until parent M001 closes. M003 remains blocked
on M002 and native evidence for each enforcement dimension claimed. No Linux
network or IPC guarantee is promoted by this closure.

## 12. Registry updates

- Marked implementation plan 006 implemented and added this closure record.
- Removed C003 from active implementation rows and added it to recently closed
  work.
- Moved parent M001 plan 001 to closing for aggregate inventory and acceptance
  review; kept M002/M003 blocked.
- Updated the command-authorization roadmap dependency graph and milestone
  table.
