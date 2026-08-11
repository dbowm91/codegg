# Agent Runtime Correctness, Autonomy, and Simplification M008 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/008-routine-ci-and-static-guard-contraction.md`

Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Implementation commit: `66326ad` (`ci: contract routine agent verification`)

## 1. Outcome

M008 contracted routine verification without changing runtime behavior or the
release cadence. Generated builtin-agent synchronization now has one
authoritative checker, and the redundant handwritten parser was deleted.
Routine CI remains one bounded `verify` job for pull requests and pushes to
`main`.

## 2. CI and local verification surfaces

Before M008, the hosted `verify` job ran:

1. `generate_builtin_agents.py --check`
2. `check_builtin_agents.py`
3. `check-core-boundary.sh`
4. `check_sandbox_contract.py`
5. `check_execution_ownership.py`
6. `cargo fmt --check --all`
7. locked workspace Clippy with `-D warnings`
8. locked bounded workspace tests

After M008, the hosted job is unchanged in shape and runs the same list minus
item 2. `scripts/verify.sh quick` likewise retains formatting, generated-agent
synchronization, core-boundary, sandbox, execution-ownership, and locked
workspace all-target checking, with the duplicate checker removed. `full`
continues to add Clippy, bounded workspace tests, and the documented optional
feature compile check.

No CI matrix, scheduled audit, artifact publication, coverage/benchmark/size
gate, dependency bot, or release automation was added. Release cadence remains
manual.

## 3. Generated-agent checker evidence

The deleted checker independently reparsed `src/agent/builtins/generated.rs`
with handwritten regular expressions and compared the result to TOML/prompt
inputs. Its fields and permission comparison were a second implementation of
the generator's output contract; it provided no schema or mismatch invariant
not already covered by generator `--check`.

The retained generator check validates required names/descriptions, modes,
permission actions, prompt-file existence and containment, runtime kinds,
unknown keys, duplicate names, deterministic output, and exact checked-in
`generated.rs`/`mod.rs` drift. It passed against all 9 builtin agents on the
implementation commit. Active documentation now points only to this checker.

## 4. Routine guard disposition

| Guard | Disposition | Evidence / owner |
|---|---|---|
| `generate_builtin_agents.py --check` | retain routine | Sole schema, prompt, deterministic-output, and generated-source owner; passed. |
| `check-core-boundary.sh` | retain routine | Cargo dependencies do not express all forbidden source-level imports; the guard protects the core ownership boundary; passed. |
| `check_sandbox_contract.py` | retain routine | Protects the child-only sandbox and fail-closed typed status boundary; ordinary Rust tests do not make handwritten Landlock/pre-exec regressions impossible; passed. |
| `check_execution_ownership.py` | retain routine | Direct process-spawn, finite-output, typed-argv, and scheduler-bypass patterns remain outside compiler-enforced ownership; the manifest is still the explicit ownership inventory; passed. |
| `check_daemon_cwd_usage.py` | retain local/full only | The narrow production authority guard remains useful, but it is not part of routine CI/quick; explicit `ExecutionContext` construction and focused multi-workspace tests reduce its routine value. |
| `check_project_agent_pwd_inference.py` | retain local/full only | Boundary-specific project-agent invariant; useful maintenance evidence, not a routine CI duplicate. |
| discovery/catalog/project-authority guards | retain local/full only | Their bounded discovery and multi-project invariants remain valuable and are not redesigned by M008. |
| scheduler, broker, identity, Git, projection, and WebSocket guards | retain local/full only | Each protects a subsystem-specific authority/security contract and remains available through the documented focused verification catalog. |

No guard was removed merely for inconvenience, and no replacement static guard
was added. The execution-ownership manifest was reviewed and retained because
its owner/reason/entrypoint metadata is the explicit operational inventory used
to review process and scheduler ownership; no classification entry was found
to be a purely stylistic duplicate of a compiler-enforced boundary.

## 5. Verification evidence

Passed on the implementation commit:

- `scripts/verify.sh quick`
- `cargo fmt --check --all`
- `python3 scripts/generate_builtin_agents.py --check`
- `python3 scripts/check-core-boundary.sh`
- `python3 scripts/check_sandbox_contract.py`
- `python3 scripts/check_execution_ownership.py`
- `cargo check --workspace --all-targets --locked` (inside quick)
- `git diff --check`

The required locked workspace Clippy command was run and remains red on three
pre-existing findings unrelated to M008: two `field_reassign_with_default`
findings in `crates/codegg-core/src/model_profile/adapter.rs` and one
`clippy::too_many_arguments` finding on the existing `AgentLoop::new` in
`src/agent/loop.rs`. M008 changes no Rust production code and does not mask or
expand those findings. No hosted `verify` result was available during this
local closure pass.

## 6. Compatibility and unresolved workflow issues

The deleted `scripts/check_builtin_agents.py` command is intentionally no
longer supported; active guidance and CI no longer reference it. Runtime,
storage, protocol, feature, branch-protection check names, and release
behavior are unchanged.

Unresolved developer-workflow issues, medium severity: the default locked
workspace Clippy command is currently blocked by the three pre-existing
warnings described above. They should be handled by a future scoped cleanup or
the integration milestone if still present; they are not an M008 closure
defect. Hosted CI should provide the final branch-visible result.

## 7. Dependency audit and planning updates

M009 is the only registered plan with M008 as a hard dependency. Its M001–M008
predecessors now all have accepted closure records, so M009 was moved from
`blocked` to `ready` in the same governance update. No other registered plan
became unblocked as a result of M008.
