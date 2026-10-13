# Linux Execution Isolation C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/linux-execution-isolation/009-m001-typed-git-policy-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `481d5077de516758795084a860a15c747a889170`

Implementation commits or pull requests:

- `ae025ef3` — typed Git execution classification and error propagation, with regression coverage and documentation.

## 1. Executive finding

C001 is closed as the typed-policy corrective. Fabricated Git executable failures and blanket operation-name denials have been removed. Git requests now classify into trusted non-executable work, contained execution requirements, additional authorization, explicit authority denial, invalid input, or backend unavailability. Callers preserve these outcomes and do not fall back from a rejected structured operation to raw Git.

This closure does not restore all M001 Git compatibility. Operations that can run repository filters, merge drivers, hooks, external diff/textconv, remote helpers, credentials, or network remain unavailable until the relevant real containment and mediation work closes. The historical M001 closure at `001-status.md` remains unchanged and blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Remove the fake executable and subcommand-name denial policy | `crates/egggit/src/process.rs`; `scripts/check_git_forbidden_patterns.py` | pass | Guard reports zero findings; actual trusted Git executable identity is preserved. |
| Return typed capability and authority outcomes | `GitExecutionPolicyOutcome`, `EgggitError::Policy`, and typed propagation through Git services, mutations, network operations, worktree, and tools | pass | Required contained operations report `BackendUnavailable` until a real backend exists. |
| Keep safe trusted Git and fixed execution pins | `cargo test -p egggit -- --test-threads=1` | pass | 89 tests passed, including hostile hook/fsmonitor and policy cases. |
| Prevent unsupported requests from falling through to raw Git | `cargo test -p codegg --lib git -- --test-threads=1`; `cargo test --test git_execution_origin_matrix -- --test-threads=1` | pass | 322 unit tests and 28 origin-matrix tests passed. |
| Keep audit outcomes structural and avoid claiming unavailable work executed | `cargo test --test identity_m002_live_audit_hooks -- --test-threads=1` | pass | 18 tests passed; policy blocks emit bounded structural records. |
| Preserve compatibility evidence for local and remote Git | `git_mutations_integration`, `git_noninteractive`, and `git_env_attack` targets | pass | Safe local ref/commit operations work; filter/driver and remote operations return typed unavailable outcomes without spawning a child. |
| Preserve process ownership and existing sandbox behavior | `cargo test --test sandbox_containment -- --test-threads=1`; execution-ownership guard | pass | 13 sandbox tests passed; ownership guard passed. |
| Run repository quick verification | `scripts/verify.sh quick` | pass | All included boundaries, static guards, and `cargo check --workspace --all-targets --locked` passed. |

## 3. Production implementation evidence

`egggit` now classifies complete shell-free Git argv and applies the fixed environment/configuration policy only to requests classified as trusted and non-executable. Requests needing repository-controlled execution or remote authority return typed backend-unavailable outcomes. Explicit global config/repository redirection is denied. External diff/textconv and remote operations are recognized as capability requests. Structured Git services and mutation paths preserve typed outcomes; the prior raw fallback is not used to bypass them. Policy denials in mutation paths produce structural audit outcomes without recording paths, URLs, credentials, or command text.

The policy guard, Git architecture/security docs, and validation review were updated. Tests now exercise both allowed trusted operations and refused operations with no child execution or false success record.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p egggit -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p codegg --lib git -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test git_mutations_integration -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test sandbox_containment -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test identity_m002_live_audit_hooks -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test git_execution_origin_matrix -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test git_env_attack --test git_noninteractive --test git_credential_cross_path -- --test-threads=1
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
git diff --check
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target scripts/verify.sh quick
```

### Results

All listed commands passed. Counts: egggit 89/89; root Git library 322/322; mutation integration 12/12; sandbox containment 13/13; audit hooks 18/18; execution-origin matrix 28/28; environment attack 20/20; noninteractive Git 5/5; credential cross-path 14/14. The quick verification finished successfully, including workspace/all-targets check. No native namespace or remote transport qualification was run or claimed.

## 5. Invariant review

- No repository-controlled executable is run by the trusted Git route; pins and stripped environment remain in place.
- Unknown operations do not gain trusted authority by default.
- Capability requirements do not create a fake process or become an ordinary I/O error.
- A typed refusal cannot trigger a more permissive raw Git fallback.
- Network, credentials, IPC, or filesystem containment are not inferred from classification or Landlock-only enforcement.
- Audit records remain structural and bounded.

## 6. Failure and recovery review

Policy rejection occurs before child creation. Focused negative tests verified no spawn for unavailable remote requests, no staging for unavailable filter-capable requests, and no shell fallback. Existing process-group timeout and cleanup behavior remains covered by the `egggit` and quick suites. This change adds no durable state, receipts, leases, or restart behavior.

## 7. Migration and compatibility review

No persisted schema, public protocol, configuration, or default sandbox behavior changed. Callers now receive typed policy errors where generic spawn errors or raw fallbacks previously obscured the reason. Safe operations such as trusted status/ref inspection and locally pinned commit remain available. Filter-capable worktree changes, recovery commands, remote operations, and other executable Git paths remain unavailable pending C002/C003.

## 8. Security review

Global caller config/repository redirection is denied. Repository hooks, fsmonitor, pager, SSH command, credential helper, signing, external diff, and textconv are pinned off for trusted operations; clean filters, merge drivers, executable aliases, and remote authority are capability-classified. Tests verify markers remain absent and credentials/paths are not retained in policy audit metadata. This is not a claim that arbitrary Git has been sandboxed.

## 9. Documentation and operations

- Updated `architecture/git.md`, `architecture/security.md`, and `docs/validation/git-security-review.md`.
- Updated `scripts/check_git_forbidden_patterns.py` to enforce typed classification and reject fabricated executable policy.
- `plans/implementation/linux-execution-isolation/009-m001-typed-git-policy-reconciliation.md` is marked implemented.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Filter-, merge-driver-, and hook-capable local Git is unavailable without a real contained launcher and protected repository view. | Some ordinary local Git mutations and recovery operations remain unavailable. | C002 remains blocked on M002–M005 native proof and accepted ADR-0023. |
| medium | Remote Git is unavailable without mediated transport, egress, and credential authority. | Fetch/pull/push/clone and remote helpers cannot execute through CodeGG's trusted route. | C003 remains blocked on C002, M005/M006, and transport/credential policy. |
| low | M001's historical compatibility failures are not reclassified as passing tests. | The original M001 capability is not complete. | Preserve `001-status.md`; complete the separately registered C002/C003 correctives before reconsidering M001. |

## 11. Roadmap disposition

C001 is closed. M002 was already independently ready under accepted ADR-0016 and ADR-0024; it is moved to active in this closure commit and may implement only the typed launch contract, feature probe, and test adapter. Production namespace/backend promotion remains blocked on proposed ADR-0023. C002 and C003 remain blocked on their native containment and transport dependencies. M001 remains blocked. No other plan is unblocked by this closure.

## 12. Registry updates

- Mark C001 closed and link this record in `plans/registry.md`.
- Mark the C001 corrective row closed in the subsystem roadmap; preserve the historical M001 blocked status.
- Move M002 from ready to active because work begins immediately after this closure.
- Leave C002, C003, M001, and M003–M008 blocked as recorded; no native-backend gate is waived.
