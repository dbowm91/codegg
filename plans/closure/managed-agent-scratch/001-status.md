# Managed Agent Scratch M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/001-private-session-scratch-and-launch-wiring.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Repository baseline reviewed: `b59e946d87f65b2ecb4ba8892e407a8f2cf9677e`

Implementation commits:

- `972e2af46f5b9bb0406fc538516949b32aa653f4` — private session scratch identity, managed launch wiring, workspace/scratch write permission correction, docs and guards.
- `87eba61e4cd074f5aaac8f6816c9e1fe424ca588` — bounded recovery for a truncated owner record left by interrupted allocation.
- `b59e946d87f65b2ecb4ba8892e407a8f2cf9677e` — validate the per-user cache root before creating managed scratch.

## 1. Executive finding

M001 is closed. A canonical authenticated session ID deterministically resolves one private CodeGG scratch directory across independent managed model-tool invocations and daemon/process restarts. Managed Bash and Python children receive `CODEGG_SCRATCH_DIR`, `TMPDIR`, `TMP`, and `TEMP`; Required sandbox launches explicitly grant the session scratch root while retaining launch-local staging. Structured file tools accept only the current host session's scratch root in addition to its workspace. Implicit default `Ask` is relaxed for structured writes beneath either trusted root; explicit stored decisions, explicit tool/path rules, and hard Deny remain authoritative.

The scratch root uses the per-user cache directory because the host's Required macOS Seatbelt policy correctly denies the credential-bearing Application Support tree. Scratch artifacts are disposable: operating-system or user cache maintenance may remove them. M001 adds no garbage collector or lease registry, so CodeGG itself does not reclaim an allocated directory. The implementation makes no Linux or Windows native sandbox qualification claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Distinct session roots; repeated/concurrent allocation converges | `scratch::tests::{session_paths_are_stable_and_separate,concurrent_allocations_converge_on_one_owned_directory}` | pass | Eight concurrent requests for one identity return one path; different identities differ. |
| Bounded owner record and interrupted allocation recovery | `scratch::tests::owner_record_recovers_from_an_interrupted_prefix` | pass | Only an exact prefix of the host-computed record is repaired; other malformed content fails closed. |
| Owner-private directory and symlink refusal | `scratch::tests::{allocation_refuses_a_preexisting_symlink,structured_file_tools_are_scoped_to_the_host_session}` | pass | Unix scratch directories are `0700`; cache parent is checked for owner, symlinks and group/world write; a symlink escape and parent write are refused. |
| Consecutive managed Bash calls share scratch and exact temp variables | `tool::bash::timeout_execution_tests::session_scratch_is_shared_across_bash_invocations` | pass | First call writes, second reads, and all four env values equal the authenticated session path. |
| Python temp behavior uses the same persistent session scratch | `python_script::executor::tests::python_tempfile_uses_the_persistent_session_scratch` | pass | `tempfile.gettempdir()` and `CODEGG_SCRATCH_DIR` resolve to session scratch under Required sandbox on this host. |
| Required sandbox preserves launch staging and grants session root | `cargo test -p codegg --test sandbox_containment` (15/15), plus the Python Required-sandbox trajectory | pass | Executed with the native macOS Seatbelt helper on x86_64. No fallback to FullHost was used. |
| Structured tools stay within workspace/session scratch; sibling and parent access fail | `scratch::tests::structured_file_tools_are_scoped_to_the_host_session` | pass | A second session cannot read the first session's artifact. |
| Workspace and scratch structured writes do not inherit implicit Ask | `permission::tests::trusted_root_write_relaxes_only_the_default_ask` | pass | Outside-root writes remain Ask; explicit write Ask and Deny are preserved. Relative write paths resolve from workspace only. |
| Session ID is host-owned and scratch is exposed through trusted context | `src/agent/turn_runtime.rs`, `src/managed_process.rs`, `src/tool/bash.rs`, `src/scheduler/executors.rs` | pass | Session ID is parsed at turn entry and carried by the execution context/process provenance, not a model-supplied path. |
| Storage compatibility | No schema or protocol change; deterministic path and owner record derive from existing canonical SessionId | pass | No migration or `STORAGE_LAYOUT_VERSION` change. A repeated call with the same identity exercises restart resolution; a full daemon restart was not separately launched. |
| Human-originated shell authority remains separate | Code-path inspection and execution-ownership/scheduler guards | pass | Session scratch propagation is attached to model tool execution context; no ambient process environment is changed. |

## 3. Production implementation evidence

`src/scratch.rs` hashes the validated session identity for the path component, creates owner-only directories, and stores a bounded versioned owner marker. Allocation is idempotent, rejects symlinked path components and conflicting records, and can repair a partially written marker after bounded retries. The canonical session ID is already durable in existing session storage; no second registry or schema migration was introduced.

`ManagedProcessService` applies trusted scratch environment values after sanitizing the inherited environment. The Required sandbox receives both its existing per-launch private staging directory and the session scratch root. Non-session Required launches retain launch-local temp values. Managed Bash, scheduled Bash work, and Python script execution carry the authenticated session identity through the existing process/job owners.

`ReadTool` and `WriteTool` extend existing validated roots to the current session scratch while retaining workspace-root-relative semantics. Write permission resolution preserves explicit decisions and relaxes only the implicit default Ask for structured writes inside the authenticated workspace or session scratch. Trusted prompt and module documentation describe the disposable artifact path without claiming FullHost OS containment.

## 4. Verification executed

### Commands run

```bash
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg --lib scratch::tests
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg --lib permission::tests::trusted_root_write_relaxes_only_the_default_ask
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg --lib tool::bash::timeout_execution_tests::session_scratch_is_shared_across_bash_invocations
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg --lib python_script::executor::tests::python_tempfile_uses_the_persistent_session_scratch
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg --test sandbox_containment
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg --test tool_execution
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo test -p codegg-core
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 scripts/verify.sh quick
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo fmt --all -- --check
rtk git diff --check
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
bash scripts/check-core-boundary.sh
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
```

### Results

- Scratch tests: 6 passed, including stable/separate identity, eight-way contention, partial owner-record recovery, symlink refusal, structured read/write scope and sibling denial.
- Permission correction: 1 passed. Managed Bash trajectory: 1 passed. Python tempfile trajectory: 1 passed.
- `sandbox_containment`: 15 passed. `tool_execution`: 61 passed.
- `codegg-core`: 858 unit tests passed; all crate integration suites and doc tests passed (cached-models 4, context 6, continuation 11, decision fixtures 1, human-shell promotion 4, identity 2, project catalog 26, session title 2, work-plan foundation 10, work-plan projection 7; 0 doc tests).
- `scripts/verify.sh quick`: passed, including all configured static guards and `cargo check --workspace --all-targets --locked`.
- Workspace Clippy `--all-targets --locked -- -D warnings`, formatting and `git diff --check`: passed on the final implementation.
- Required sandbox helper built for x86_64 macOS using the recorded deployment and vendored-library environment. Initial Python Required-sandbox execution exposed that Application Support is intentionally denied; moving scratch to the per-user cache root made the real Seatbelt trajectory pass. The earlier test failure was environmental/path-policy evidence and is resolved in the final implementation.

## 5. Invariant review

- No daemon cwd or caller-supplied path grants scratch authority. Allocation consumes the canonical session identity only.
- Relative structured filesystem paths remain workspace-root-relative. Scratch is an additional authenticated absolute root; the scratch parent and sibling session roots are not granted.
- Required does not fall back to FullHost. Existing per-launch staging and process-tree ownership remain in place.
- `workspace_root` and `child_workspace_ceiling` remain independent. Human `!cmd`, plugins, and IDE/formatter paths do not inherit model scratch from ambient variables.
- No process-global environment mutation, raw process launcher, /tmp interception, shell parser rewrite, or dedicated model tool was added.
- Explicit permission Deny and Ask decisions remain authoritative for structured writes. Writes outside the trusted workspace and session scratch retain existing behavior.

## 6. Failure and recovery review

Concurrent initial allocation converges. A crash after creating but before fully writing the owner marker is recovered only when the existing bytes are an exact prefix of the expected record; malformed, longer, or conflicting content fails closed. Repeating allocation with the same authenticated session ID deterministically resolves the same path after daemon/process restart. M001 has no collector, so foreground completion cannot delete scratch used by a later call or a background process; active job leases and safe reclamation remain M002/M003 work.

The containing cache directory may be cleared by host maintenance. Because scratch contains disposable artifacts and is not a durable result store, clearing it starts with an empty directory at the same deterministic session locator; applications must not rely on scratch artifacts surviving cache purge.

## 7. Migration and compatibility review

No database, protocol, config schema, or serialization changes were required. Existing sessions allocate lazily at turn entry. The owner record is a local integrity/recovery marker and is not an authorization credential. Linux/macOS/Windows platform differences are not generalized from the single native host result.

## 8. Security review

The directory is rooted beneath the per-user cache location, whose directory ownership and group/world write bits are checked on Unix; CodeGG scratch directories are `0700` and the owner marker is `0600`. Path-component symlinks, sibling sessions, parent access, malformed identity, and conflicting owner content are refused. Structured filesystem operations continue through existing target validation. Required macOS Seatbelt explicitly grants only launch staging plus the host-authenticated session root. FullHost child programs remain cooperative; this closure makes no OS isolation claim for them. Same-UID hostile process isolation and race-resistant hostile local filesystem mutation are not newly claimed.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/permission.md`, `architecture/process-tool-execution-ownership.md`, `architecture/security.md`, `docs/tools.md`, `.opencode/skills/agent/SKILL.md`, `.opencode/skills/tool-execution/SKILL.md`, and `scripts/check_sandbox_policy_wiring.py`. The documentation names cache disposal behavior and limits native sandbox claims to evidence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Linux and Windows native runtime/Required-sandbox behavior were unavailable on the x86_64 macOS host. | No support or containment claim may be made for those hosts based on M001. | M003 performs available platform qualification and records unsupported cases explicitly. |
| Low | No full daemon restart or live provider/model call was launched; tests exercise repeated authenticated tool execution and deterministic identity recovery directly. | Cross-frontend behavior is derived from the canonical SessionId resolver and is not an end-to-end frontend observation. | M002 revalidates current owner/runtime interfaces; M003 can add platform/daemon qualification without changing M001's authority contract. |

No high-severity or correctness finding remains for M001's scoped single-session capability.

## 11. Roadmap disposition

M001 is strictly closed. Its required managed Bash/Python paths, structured filesystem scope, prompt/env wiring, permission defaults, recovery behavior, and native macOS Required-sandbox grants have executable evidence. Only M002 became eligible and is now active. M003 remains blocked until M002 strict closure. A registry audit found no other registered plan in this line depending directly on M001.

## 12. Registry updates

- Mark M001 implemented/closed and link this closure record.
- Move M002 from blocked to active after recording M001 strict closure; revalidate implementation interfaces against the new branch head.
- Keep M003 blocked on M002 strict closure. No other managed-agent-scratch plan is newly eligible.
