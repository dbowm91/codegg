# Post-Audit Maintainability and Surface M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-maintainability-surface/006-terminal-compatibility-policy-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md#7-milestones`

Repository baseline reviewed: `f80b08d5`

Implementation commits:

- `f80b08d5` — feat(tool): converge terminal shell policy
- this closure commit — record M006 evidence and planning disposition

## 1. Executive finding

M006 is complete and strictly closed. The retained one-shot `terminal` tool
now delegates shell-risk evaluation to the canonical Bash policy seam over the
exact string later passed to `sh -c`. Its historical name, parameter shape,
deferred disclosure, compatibility builders, environment filtering, allowed
root validation, bounded execution, and managed-process ownership remain
intact. The independent terminal regex and duplicated default blocked-command
set were removed.

The convergence is monotonic: terminal-specific configuration can still narrow
authority, while canonical Bash rejection is always applied first. No storage,
protocol, scheduler, permission-level, or interactive-PTY behavior changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One owner classifies equivalent Bash/terminal shell content | `src/tool/bash/policy.rs::check_shell_security`; terminal delegates it | pass | The former terminal regex is deleted. |
| Default blocked-command policy is not duplicated | `default_blocked_commands()` is called by both constructors | pass | The shared list remains Bash-policy owned. |
| Terminal checks the effective execution payload | `TerminalTool::effective_script()` feeds policy and `sh -c` | pass | Separate `command` and `args` are normalized once. |
| No dangerous case is widened | Differential table covers substitutions, redirects, interpreters, wrappers, backgrounding, destructive commands, and quoting | pass | Canonical Bash adds stricter coverage for `${...}` and input `/dev` redirects. |
| Terminal-only validation remains bounded and justified | `is_safe_env_var_name()` remains local; child-root checks reuse Bash validator | pass | Environment names are intrinsic to terminal's env object contract. |
| Compatibility name/surface remains available | Registry, disclosure, permission, risk, agent, and stored-run reader census | pass | `terminal` remains retained and deferred; no migration required. |
| Managed execution contracts remain unchanged | Existing `ManagedProcessService` request path plus allowed-command integration test | pass | No PTY or second process lifecycle introduced. |

### Differential policy matrix

The matrix below records the compatibility census for equivalent shell
content. “Shared” means the old terminal and Bash decisions now come from the
same canonical seam; “stricter canonical” records a historical terminal case
that is intentionally narrowed to match Bash safety policy.

| Shell family | Historical terminal behavior | Canonical result | Evidence/disposition |
|---|---|---|---|
| Plain benign commands and quoting | Allowed | Allowed | `printf` and quoted/UTF-8 cases remain covered. |
| Command substitution and backticks | Blocked | Blocked | Shared canonical blocked patterns. |
| Braced/variable expansion (`${...}`) | Previously less restrictive | Blocked | Stricter canonical result; table-driven regression coverage. |
| Pipes and shell chaining | Blocked for dangerous forms | Blocked | Shared canonical patterns and command checks. |
| Output redirects | Blocked for dangerous forms | Blocked | Shared canonical redirect checks. |
| Input redirects from `/dev` and heredoc forms | Historically narrower | Blocked | Stricter canonical result; no widening. |
| Interpreters and wrappers (`python -c`, `bash -c`, `env`) | Blocked for dangerous forms | Blocked | Shared canonical checks over the effective script. |
| Backgrounding, `disown`, and `kill` | Blocked | Blocked | Shared canonical checks. |
| Destructive commands, fork bombs, loader/download/chmod/chown forms | Blocked | Blocked | Shared default list/pattern policy. |
| `command` plus `args` representation | Checked as terminal-specific command text | Checked as exact `sh -c` payload | `effective_script()` is used for both policy and spawn. |
| Custom blocked-command restrictions | Narrowed terminal authority | Narrowed authority | Terminal custom set is passed to the canonical seam. |
| Terminal allowlist | Could restrict terminal commands | Can only restrict | Canonical rejection runs before allowlist early return. |
| Environment variable names | Terminal-only validation | Terminal-only validation retained | Input-contract restriction, not shell classification. |
| Allowed workspace roots | Terminal reused child-workspace validation | Same validation | Existing Bash validator remains shared. |

## 3. Production implementation evidence

`src/tool/bash/policy.rs` now owns both the canonical default blocked-command
set and the reusable `check_shell_security()` seam. `BashTool` delegates its
existing method to that seam, preserving the Bash entrypoint and test surface.

`TerminalTool` now constructs the exact effective script first, sends that
same string through the Bash policy seam, then retains its compatibility-only
environment-name filtering and shared child-workspace validator. Its existing
timeout, output caps, cwd handling, sanitized environment, and
`ManagedProcessService` request construction were not refactored.

The deleted production authority consists of terminal's `BLOCKED_PATTERN`
regex and its duplicate default blocked-command literal set. No independent
terminal shell-risk classifier remains.

## 4. Verification executed

### Commands run

```bash
rtk proxy cargo check -p codegg --tests
rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk proxy scripts/verify.sh quick
rtk cargo fmt --all -- --check
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_scheduler_bypass.py
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test -p codegg --lib -- tool::bash tool::terminal -- --test-threads=1
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test --test tool_execution -- --test-threads=1
rtk git diff --check
```

### Results

- `cargo check -p codegg --tests`: pass.
- All-feature workspace Clippy with `-D warnings`: pass.
- `scripts/verify.sh quick`: pass, including formatting, generated-agent,
  core-boundary, sandbox, execution-ownership, and workspace all-target check.
- Formatting, execution-ownership, scheduler-bypass, and diff checks: pass.
- Focused Bash/terminal test filter: 91 passed, 0 failed.
- `tests/tool_execution`: 55 passed, 0 failed, including
  `test_terminal_tool_simple_command`.
- The default executable-test attempt was initially blocked by the host's
  x86_64 link resolving `/opt/local/lib/liblzma.dylib` (arm64). The explicit
  static-LZMA/x86_64 search-path invocation above supplied a compatible local
  build and produced the passing results recorded here.
- `scripts/check_project_catalog_invariants.py` was also run and failed only
  its unrelated stale expectation that `STORAGE_LAYOUT_VERSION` be 54 while
  the repository currently declares 56. It does not inspect or affect M006.

## 5. Invariant review

- `bash` remains the canonical model shell; `terminal` remains a deferred
  compatibility-only tool.
- Canonical policy is evaluated before any managed process request. The new
  blocked-command side-effect test confirms a rejected substitution does not
  create its marker.
- Terminal remains one finite noninteractive `sh -c` execution and does not
  become a PTY or alternate scheduler authority.
- Timeout, cancellation/process-tree supervision, output bounds, cwd/root
  validation, and environment sanitation remain owned by their existing
  paths.
- Historical name, serde parameters, disclosure, permission/risk mappings,
  agent deny-lists, and stored-run/import readers remain present.
- `with_blocked_commands` and `with_allowlist` remain public; the canonical
  seam composes them as additional restrictions and never lets them bypass a
  canonical rejection.
- No second parser or generalized security-policy framework was introduced.

## 6. Failure and recovery review

Policy rejection is synchronous and pre-spawn. The canonical seam returns an
error directly; terminal has no fallback to its removed classifier or direct
execution. Execution timeout, cancellation, process-tree cleanup, output
caps, and resource release remain `ManagedProcessService` responsibilities.
There is no persistent terminal execution state, restart recovery, lease, or
new contention path.

## 7. Migration and compatibility review

No persisted data or wire protocol changed. The terminal tool name and
historical parameters remain unchanged. A repository census confirmed the
literal `terminal` references in disclosure, permission modes, risk mapping,
agent capabilities, tool registration, stored-run readers, and compatibility
tests remain intentional. Existing consumers of the two public terminal
builder methods retain them.

The policy convergence intentionally changes some historical terminal
decisions to the stricter canonical Bash result, notably braced/variable
expansion and input redirects from `/dev`; these are safety corrections, not
execution widening.

## 8. Security review

Canonical blocked-pattern and default blocked-command checks execute before
cwd resolution or process spawn. The differential tests cover command
substitution, backticks, `${...}`, shell pipes, `/dev` redirects, interpreters
with `-c`, backgrounding/disown, destructive wrappers, fork-bomb forms,
UTF-8, and argument concatenation. The side-effect test verifies a blocked
effective payload cannot reach `sh`.

Terminal's dynamic-loader environment-name filtering remains fail-closed for
invalid and dangerous names. Allowed-root enforcement continues through the
existing Bash child-workspace validator. No secrets, new privileges, network
authority, or audit/protocol surface was added.

## 9. Documentation and operations

Updated:

- `architecture/tool.md` — Bash policy as canonical shell-safety owner and
  terminal as a compatibility adapter.
- `src/tool/bash/policy.rs` and `src/tool/terminal.rs` — ownership comments
  and effective-payload rationale.

Existing operational guards remain sufficient; no CI lane or scanner was
added. The explicit linker environment used for local execution evidence is
host-specific and is not a repository requirement.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `check_project_catalog_invariants.py` expects storage layout 54 while current code declares 56 | Unrelated planning/guard drift; no M006 impact | Resolve under the project-catalog maintenance workstream. |

No critical, high, or medium M006 findings remain.

## 11. Roadmap disposition

M006 is closed. Its corrective boundary is complete and no corrective pass is
required. Independent M007 remains ready for handoff. The parent original
post-audit roadmap remains historical/closed; the corrective addendum remains
active solely for M007.

## 12. Registry updates

- Marked the implementation plan active during work and now
  `implemented — closed; see this record`.
- Moved M006 from dependency-ready to closed in the corrective addendum and
  active registry state.
- Added M006 to the registry's recently closed work.
- Audited every row in `plans/registry.md` under Blocked work and the
  corrective addendum dependency graph. No registered plan lists M006 as a
  hard or interface dependency, so no downstream plan became ready. M007
  remains ready and independent.
- No corrective follow-up plan was created; the only residual finding is
  unrelated low-severity catalog guard drift.
