# Linux Execution Isolation C009 — Closure Status

Status: closed (bounded candidate qualification; M003 production remains blocked)

Source implementation plan:

- `plans/implementation/linux-execution-isolation/017-m003-alias-race-and-compatibility-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `6f764ae76e786ad3e3b1b6e6e25997bc3371ffd0`

Implementation commit:

- `2921a9c6a77588669801cebb24b80c1d1c8f1a4f` — descriptor-relative private Git object views, object-alias race coverage, bare-repository refusal, plan/architecture updates.

## 1. Executive finding

C009 is closed as a bounded qualification of the opt-in Linux protected-mount candidate. The exact implementation head passed the hosted Bubblewrap native tests, trusted-Git tests, and macOS compile workflow. Local verification also passed the focused egggit, sandbox, Landlock, ADR integrity, policy-guard, and quick suites.

This closure does not qualify M003 for production. Trusted object-reading Git commands now use a private per-command object view that omits `objects/info`, preventing late alternates and post-admission source-object mutation from changing the child view. A local fragmented object store measured 2.95 seconds for snapshot setup plus `git status`; packed-repository cost is unmeasured. No production Git route or Bubblewrap backend is registered, and Landlock remains canonical. Object-writing Git mutations remain a C002 dependency.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Ordinary, linked-worktree, external common-dir and nested-submodule topology | Hosted Linux run `38741704670`, native test filter | pass | Includes real Bubblewrap child, read/write boundary checks, nested submodule and common-dir protection. |
| Malformed/oversized pointers and bare-root refusal | Hosted Linux run `38741704670`; local protected-mount suite | pass | Setup fails before target marker. Bare Git repositories are not supported as writable roots. |
| Leased root, `.git` pointer/gitdir/common-dir and denied-alias replacement | Hosted Linux run `38741704670`; local replacement fixtures | pass | Includes workspace/ancestor replacement and deny-under-writable refusal. |
| Trusted Git config cannot execute hooks, fsmonitor, filters, diff/textconv or executable aliases | Hosted Linux trusted-Git suite, 18/18 | pass | Unsupported executable routes return typed outcomes before helper execution. |
| Static and late-created object alternates cannot redirect trusted reads | Hosted Linux trusted-Git suite; local ordinary/common-dir late-alternate regressions | pass | Static alternates return typed `BackendUnavailable[RepositoryAlias]`; late alternates are absent from the private object view. |
| Source loose-object mutation after admission cannot alter the child view | Local `trusted_git_read_view_is_immutable_after_admission` regression | pass | The private copy retains the admitted object bytes. |
| Trusted object-reading command coverage | Local `trusted_git_object_read_routes_require_private_view` matrix | pass | Covers `config --blob`, `check-attr --source`, `apply --3way`, reflog, branch/remote/worktree/stash views, tag/notes listing, and core object readers. |
| Python, Cargo check/test child, PTY first-hop, timeout/cancel and concurrent launches | Hosted Linux run `38741704670` | pass | Rustup's missing `/proc/self/exe` and deny paths nested under writable grants remain explicit limitations. |
| Native Bubblewrap host qualification | Hosted Linux run `38741704670` | pass | Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0-1ubuntu0.3`, UID/GID 1001, AppArmor `unconfined`. Ephemeral runner changed `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. |
| macOS compile and Windows cross-target compile | Hosted macOS run `38741704581`; local Windows GNU and macOS egggit checks | pass | No cross-platform enforcement claim is made. |
| Trusted object-view performance | Local current-checkout measurement | partial | 105 MiB / 3,945 object files on ext4 without reflink: 2.95 s for view setup plus trusted `git status`. Packed-repository timing is not measured and production routing remains blocked. |
| Object-writing Git mutation containment | Linux C002 dependency | not in scope | Commit/tag creation and other object-writing operations are not qualified by C009. |

## 3. Production implementation evidence

- `GitEnvPolicy` creates a command-scoped private object view for the trusted object-reading route matrix and keeps its temporary directory alive with the command wrapper through spawn and completion/cancellation.
- Unix object-store traversal uses descriptor-relative no-follow opens, validates regular files and supported fanout/pack layout, and bounds entry count. Linux uses filesystem reflink when supported and falls back to a private copy; other platforms copy object bytes.
- The view has no `objects/info`; `GIT_OBJECT_DIRECTORY` points Git at that view and optional locks are disabled. Existing static alternates are still refused with a typed capability result.
- The Linux mount compiler rejects bare repositories as writable workspaces. Existing linked-worktree/common-dir lease pinning and deny-path fail-closed behavior remain intact.
- No default, production backend registration, protocol, or persistent policy change was made.

## 4. Verification executed

### Commands run

```bash
cargo test -p egggit --locked trusted_git_object_read_routes_require_private_view -- --nocapture
cargo test -p egggit --locked -- --test-threads=1
CODEGG_RUN_C006_BWRAP_NATIVE=1 CODEGG_RUN_C009_BWRAP_NATIVE=1 cargo test --locked --lib protected_mount_ -- --nocapture --test-threads=1
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_landlock -- --test-threads=1
cargo check -p egggit --tests --target x86_64-pc-windows-gnu --locked
cargo check -p egggit --tests --target x86_64-apple-darwin --locked
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_adr_integrity.py
scripts/verify.sh quick
```

Hosted exact-head commands ran in `Linux C006 protected mount qualification` run [`38741704670`](https://github.com/dbowm91/codegg/actions/runs/38741704670) on commit `2921a9c6a77588669801cebb24b80c1d1c8f1a4f`; native protected-mount was 11/11 and trusted-Git repository-config was 18/18. Cold/warm mount-launch samples were 30.46 ms and 19.83 ms. Exact-head macOS compile passed in run [`38741704581`](https://github.com/dbowm91/codegg/actions/runs/38741704581).

### Results

- Egggit: 105 passed; focused private-view route matrix: 1 passed.
- Native protected mounts: 11 passed. Sandbox containment: 15 passed. Landlock: 3 passed.
- `scripts/verify.sh quick`: passed, including formatting, workspace all-target check, ADR integrity, and repository guards.
- Windows GNU and macOS egggit test-target checks: passed. Windows emitted three existing test-only unused warnings.
- `git diff --check`: passed before commit. Implementation was committed and pushed as `2921a9c6`.

## 5. Invariant review

- Landlock remains the default. Bubblewrap remains opt-in and unregistered as a production backend.
- Arbitrary nested `.git` creation remains workspace content. Trusted outer-worktree reads are pinned to the selected root; nested repository metadata does not redirect the tested trusted read route.
- Static object alternates fail closed; trusted object-reading commands use a private view with no alternate metadata.
- Denied paths beneath writable grants are rejected instead of relying on detachable masks.
- The obtained mount status describes filesystem scope only; no process, IPC, network, seccomp, or resource enforcement is inferred.

## 6. Failure and recovery review

- Malformed pointers, unsupported layouts, bare roots, identity changes, and private-view setup failures fail before trusted target execution or return typed `BackendUnavailable`.
- Native timeout, cancellation, concurrent launch isolation, and cleanup fixtures pass on the hosted runner.
- Private object views are command-scoped and removed when their command wrapper is dropped after completion or cancellation.
- No persistent state, schema, lease format, or recovery record changed.

## 7. Migration and compatibility review

There is no migration or persistent configuration change. Existing static-alternate repositories remain unsupported on trusted Git routes with a typed refusal. The opt-in mount candidate passes the tested Cargo check/test, Python write, and PTY first-hop subset. Rustup currently requires `/proc/self/exe`, which is unavailable in this filesystem-only mount view. Nested deny paths under writable grants are refused.

## 8. Security review

- Descriptor-relative no-follow traversal prevents path replacement during Unix object-store enumeration. Source object bytes are copied or reflinked rather than hard-linked into the trusted view.
- Object entry names/types and object count are validated before spawn; scratch is private and lifetime-scoped.
- Git global/system configuration and tested repository-controlled execution features remain neutralized or typed-unavailable.
- No secrets or command outputs are added to audit records.
- Object-writing Git operations remain on the separate C002 containment path and are not covered by this closure. Network and process isolation are not claimed.

## 9. Documentation and operations

Updated `architecture/security.md`, `.opencode/skills/permission/SKILL.md`, the C009 implementation plan, Linux roadmap, and `plans/registry.md`. ADR-0031 remains accepted and unchanged. The hosted Linux workflow records kernel, Bubblewrap, UID/GID, AppArmor, and user-namespace setup. Landlock remains the operational default.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Private object-view setup took 2.95 seconds for 105 MiB / 3,945 files on local ext4 without reflink support; packed-repository timing is unmeasured. | Repeated trusted Git reads may be noticeably slower on fragmented object stores. | Before production routing, measure representative packed and fragmented repositories and establish an acceptable budget or a safe lower-cost snapshot mechanism. |
| medium | Object-writing local Git operations remain outside the private read view and require C002 containment. | C009 does not establish safe local Git mutation or overall M003 production readiness. | Complete C002 with real filesystem/process/offline containment and Git metadata lease before any production Git mutation route. |
| low | Production PTY service profile routing is not integrated. | C009 qualifies only first-hop PTY transport compatibility. | Keep under M004 production admission/integration; it is not a C009 gate. |

## 11. Roadmap disposition

C009 is closed as bounded candidate qualification. The next Linux production dependency does not become ready from this closure. M003 and C002 remain blocked on production admission/containment and representative object-view performance evidence. No Bubblewrap backend is registered and no default changes.

## 12. Registry updates

`plans/registry.md` and `plans/subsystems/linux-execution-isolation-roadmap.md` record C009 closed, preserve M003 as blocked, retain C002/C003 as independent gates, and document the object-view performance finding. Historical C006 and M002 closure records remain unchanged.
