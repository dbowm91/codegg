# Coding-Agent Tool Surface Corrective M007 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/coding-agent-tool-surface-corrective/007-m003-m004-closure-evidence-reconciliation.md`
Source subsystem roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m007--m003m004-closure-evidence-reconciliation`
Repository baseline reviewed: `1d343719` (current HEAD after planning-only activation; production implementation remained `4cf35238`)
Implementation commits: none; evidence-only qualification. Planning commits: `1d343719` — start qualification; `4ac7d7e7` — begin closure review.

## 1. Executive finding

M007 is strictly closed. The missing current-baseline evidence promised by the
M003 and M004 implementation plans has been executed and recorded without
rewriting either historical closure record or changing production code. Every
required command passed, except the historical `agent_run_tool` target, which
does not exist in the current repository or its Git history. That stale target
was replaced with the unambiguous canonical `cargo test -p codegg --lib
tool::task` coverage already used by M003; the substitution and failed lookup
are recorded below.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Freeze one current qualification baseline | HEAD `1d343719`; worktree clean before qualification; production changes are from M006 only | Pass |
| M003 focused tests | `tool_search`, `lsp`, `git`, `task`, tool-surface minimization, and Tool Program Git/LSP palette suites | Pass |
| M003 agent-run test requirement | Exact target attempted and reported absent; current canonical `tool::task` suite passed; no `agent_run_tool` file exists in repository/history | Stale-target substitution documented |
| M004 focused tests | `verify`, `command_intent`, `bash`, `test`, adversarial routing, and tool execution suites | Pass |
| Static ownership guards | Execution-ownership and scheduler-bypass checks passed | Pass |
| Broad verification | Format check, all-features Clippy, quick verification, and diff check passed | Pass |
| Historical truth | `003-status.md` and `004-status.md` were not edited | Pass |

## 3. Production implementation evidence

No production implementation changes were made by M007. The qualification
covered the installed M003/M004 production commits at one current baseline.
The one missing test target is a planning/target-name defect: `git log` and
`git ls-tree` found no `tests/agent_run_tool.rs`, and no current Cargo test
target bears that name. Agent-run/task behavior is currently covered by the
`tool::task` unit suite and the repository's existing agent-loop/subagent
integration suites; the required current equivalent was therefore the
focused `tool::task` command, not a fabricated test target.

## 4. Verification executed

### M003 qualification

| Command | Result |
|---|---|
| `cargo test -p codegg --lib tool::tool_search` | 3 passed |
| `cargo test -p codegg --lib tool::lsp` | 178 passed |
| `cargo test -p codegg --lib tool::git` | 13 passed |
| `cargo test -p codegg --lib tool::task` | 4 passed |
| `cargo test --test tool_surface_minimization -- --test-threads=1` | 13 passed |
| `cargo test --test tool_program_git_lsp_palette -- --test-threads=1` | 32 passed |
| `cargo test --test agent_run_tool -- --test-threads=1` | stale target: Cargo reported no such test target |
| Current equivalent: `cargo test -p codegg --lib tool::task` | 4 passed; substitution accepted and recorded |

### M004 qualification

| Command | Result |
|---|---|
| `cargo test -p codegg --lib tool::verify` | 3 passed |
| `cargo test -p codegg --lib command_intent` | 263 passed |
| `cargo test -p codegg --lib tool::bash` | 87 passed |
| `cargo test -p codegg --lib tool::test` | 54 passed |
| `cargo test --test command_routing_adversarial -- --test-threads=1` | 139 passed |
| `cargo test --test tool_execution -- --test-threads=1` | 55 passed |
| `python3 scripts/check_execution_ownership.py` | passed |
| `python3 scripts/check_scheduler_bypass.py` | passed |

### Shared verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| `scripts/verify.sh quick` | passed |
| `git diff --check` | passed |

All evidence is local evidence. No hosted or external operational evidence was
required by M007.

## 5. Invariant review

- Historical M003/M004 closure records were not rewritten.
- No test, flag, or acceptance criterion was weakened.
- No production fix was hidden in the evidence pass.
- Broad verification was run directly; it was not inferred from focused tests.
- The stale target was classified and replaced only because the current
  equivalent is already the canonical task-tool test surface.

## 6. Failure and recovery review

M007 introduces no runtime behavior, failure path, cancellation behavior, or
recovery state. The absent `agent_run_tool` target was a bounded command-name
issue, not a failing product test. There were no flaky retries or unexplained
red commands.

## 7. Migration and compatibility review

No migration, protocol, schema, or compatibility change was made. The
qualification is supplemental current-baseline evidence only.

## 8. Security review

The required execution-ownership and scheduler-bypass guards passed. No
authority, discovery, verification, or scheduler behavior was changed.

## 9. Documentation and operations

This closure record, the subsystem roadmap, and `plans/registry.md` were
updated. The historical closure records remain immutable evidence. The
repository's standard local posture remains format check, all-features
Clippy, and `scripts/verify.sh quick`.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. The stale `agent_run_tool` name is fully qualified as a current-equivalent
substitution and does not require a corrective implementation plan.

## 11. Roadmap disposition

M003 and M004 are restored from conditionally closed to strict closed because
all missing broad evidence is now available at the current baseline. M007 is
closed. M005 remains ready after the independent M006 unblock audit.

## 12. Registry updates

The M007 implementation plan and roadmap are marked closed and this record is
the accepted supplemental evidence. M003 and M004 rows in the roadmap and
registry are marked closed with M007 cited as the qualification source. No
blocked plan was newly unblocked by M007; M005 was already moved to `ready` by
the M006 closure. No corrective plan was registered because no product defect
was found.
