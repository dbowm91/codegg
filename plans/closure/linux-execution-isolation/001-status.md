# Linux Execution Isolation M001 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Implementation commits or pull requests:

- `eb0f18c76e27688c41c437143c2eba60e55035ca` — initial trusted Git process
  policy, hostile fsmonitor/hook probes, documentation and static guard.

## 1. Executive finding

The initial trusted Git boundary is implemented and focused Linux evidence
passes. M001 cannot close because the fail-closed policy also refuses ordinary
typed filter/merge-driver and network operations. The existing approval path
does not yet identify a separate contained execution class for those
operations. A maintainer decision and implementation are required before the
compatibility acceptance criterion can pass.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Repository hooks and fsmonitor do not execute through the trusted local route | Native Linux tests in `crates/egggit/src/process.rs` and `status.rs` | pass | Marker files remain absent for post-commit hook and fsmonitor fixtures. |
| External diff/textconv, credential helper, signing, pager and SSH command config are pinned | `GitEnvPolicy` fixed `-c` overrides; policy unit test | pass | Does not establish proxy/SSH host config safety or network isolation. |
| Caller config/repository redirects and unknown aliases cannot override the policy | Policy tests for `-c`, `--config-env`, `-C`, `--git-dir`, unknown subcommand | pass | Unknown names fail before launching Git. |
| Repository filter/merge driver paths are non-executable | Refusal of add/checkout/restore/merge/rebase/stash mutation, filter-enabled cat-file, clone/fetch/push and hard reset | partial | They fail before Git spawn, but no separately authorized contained route exists yet. |
| Benign authorized Git workflows retain expected changes | `git_mutations_integration` | fail to establish | Existing suite passes only its current cases; it does not prove that newly refused typed operations retain behavior. The compatibility gap is the closure blocker. |
| All trusted entrypoints share the canonical process owner | `check_git_forbidden_patterns.py`; `check_execution_ownership.py`; CodeGG/core call sites reviewed | pass | The static owner guard passes; complete hostile-metadata coverage of every entrypoint remains a follow-up evidence item. |

## 3. Production implementation evidence

`egggit::process::GitEnvPolicy` now inserts fixed Git config overrides, refuses
caller-supplied global config/repository identity overrides and unknown
subcommands, and denies operations that can execute workspace-controlled
filters, merge drivers or external network helpers. Diff/show/log receive
`--no-ext-diff --no-textconv`. The process owner still clears the inherited
environment and applies its existing allowlist and process-group behavior.

No namespace, mount, seccomp, network or resource isolation is implemented by
this milestone. Writable `.git` metadata is not protected.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk proxy bash -lc 'CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p egggit -- --test-threads=1'
rtk proxy bash -lc 'CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p codegg --lib git -- --test-threads=1'
rtk proxy bash -lc 'CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test git_mutations_integration -- --test-threads=1'
rtk proxy bash -lc 'CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p codegg --test sandbox_containment -- --test-threads=1'
rtk python3 scripts/check_git_forbidden_patterns.py
rtk python3 scripts/check_execution_ownership.py
rtk proxy bash -lc 'CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target scripts/verify.sh quick'
```

### Results

`cargo fmt`, the final egggit suite (89 passed), root Git-filtered unit suite
(322 passed), sandbox-containment integration test, both static guards, and
`verify.sh quick` passed. Quick verification included workspace
`cargo check --all-targets --locked`. The egggit suite includes native Linux
hook/fsmonitor marker probes. The Git mutation integration suite did **not**
pass: 9 of 12 tests fail because they expect filter/merge/switch operations to
complete, while the new policy refuses them. This is the recorded M001
compatibility blocker, not accepted evidence of a completed workflow. An
earlier hang-based fsmonitor assertion was updated to assert successful status
and marker absence after the policy stopped the hook.

## 5. Invariant review

- Hook, fsmonitor, external diff/textconv, credential helper, GPG signer and
  repository-selected SSH command values are overridden in the trusted route.
- Arbitrary config argv, repository redirection and unknown alias command names
  are refused before Git starts.
- Filter/merge-driver operations fail closed before Git starts.
- The caller's environment remains allowlisted; no credentials are added to
  diagnostics by the new policy.
- Landlock-only and other platform defaults were not changed.

## 6. Failure and recovery review

Denied requests fail at process spawn through a deliberately missing
executable; the current caller sees a generic spawn error rather than a typed
policy-denial result. No child starts and no repository operation is replayed.
This generic error shape is another item to resolve when adding the authorized
contained route. Cancellation, restart and contention behavior are unchanged.

## 7. Migration and compatibility review

No schema or protocol migration occurred. Typed `add`, checkout/restore,
merge/rebase, mutating stash, clone, fetch, push and hard reset requests are
currently refused. This exceeds a diagnostic-only limitation and blocks
expected workflows; M001 remains blocked until the maintainer selects a
contained authorized path or explicitly revises the compatibility requirement.

## 8. Security review

Fixed `-c` options protect the listed Git configuration execution points. The
policy is not a filesystem boundary, and the worktree `.git` directory remains
writable by child processes. Network host SSH configuration and complete
adversarial entrypoint qualification are not claimed as closed evidence.

## 9. Documentation and operations

- Updated `architecture/git.md` and `architecture/security.md` with the
  current trusted Git boundary and limitations.
- Appended a dated addendum to `docs/validation/git-security-review.md`; the
  historical review text remains intact.
- Extended `scripts/check_git_forbidden_patterns.py` with a canonical policy
  wiring guard.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No separately authorized contained route for ordinary filter/merge/network Git workflows | Trusted typed Git mutations and remote operations lose expected behavior | Select and implement the route; add positive compatibility fixtures; then rerun M001 closure evidence. |
| medium | Denial is exposed as a generic spawn error | Operator/user cannot distinguish policy refusal from a missing executable | Return a typed policy error through the Git process and mutation result path. |
| medium | Full hostile-metadata matrix and exact-entrypoint native qualification remain incomplete | Static checks and a small probe set do not prove every Git subcommand safe | Complete the plan's inventory and hostile fixture matrix after the authorized route is designed. |

## 11. Roadmap disposition

M001 is blocked; no downstream milestone is unblocked. The plan remains open
for a corrective design/implementation pass. M002 independently remains
blocked on maintainer acceptance of proposed ADR-0022.

## 12. Registry updates

- `plans/registry.md`: M001 set to blocked with the authorized contained-route
  requirement; M002–M008 remain blocked.
- `plans/subsystems/linux-execution-isolation-roadmap.md`: M001 set to blocked
  and the compatibility gate recorded.
