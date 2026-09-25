# Execution ownership inventory

Codegg's daemon execution is a scheduler-owned service. Every process
spawn, worker dispatch, test runner entry, background loop, and durable
job creation must be declared in
[`docs/execution-ownership.toml`](./execution-ownership.toml) with an
explicit owner classification. The manifest is consumed by
`scripts/check_execution_ownership.py`, a static guard, and by the
runtime authority matrix in `tests/scheduler_authority_matrix.rs`.

## Owner classes

| Owner | Meaning |
|---|---|
| `scheduler` | Production daemon path that routes heavy work through `JobSubmissionService`. One job → one admission → one attempt → one terminal state. |
| `interactive` | Long-lived user-controlled PTY / REPL / editor session. Not a heavy-job submitter. |
| `standalone_compat` | Explicit `--standalone`, `--stdio`, or test-harness surface. Documented as outside the daemon singleton guarantee. |
| `definition_or_adapter` | Defines a canonical subsystem (scheduler executor, managed process service, test runner entry point, dispatcher trait, scheduler types) but does not invoke it on its own. The canonical invoker is a scheduler executor or another declared site. |
| `deferred_domain_executor` | Typed subsystem scheduled for future migration to scheduler. Documented compatibility path with a follow-up reference. |
| `test_only` | Test fixture (`#[cfg(test)]` or under `tests/`). |
| `forbidden_bypass` | Must be fixed; the static guard fails on this classification. |

## How to add a new site

1. Add a `[[site]]` entry to the bottom of the manifest with `path`,
   `owner`, and an explanatory `reason`. Optionally include
   `entrypoint` (canonical submission boundary), `followup` (future
   plan), and a `note` for migration context.
2. Run the guard locally:

   ```bash
   python3 scripts/check_execution_ownership.py
   ```

3. If the owner is `forbidden_bypass`, the guard fails immediately.
   Do not commit such an entry; rewrite the site first.

## How the static guard works

`scripts/check_execution_ownership.py` greps production Rust
files under `src/` and `crates/` for canonical spawn patterns:

- `tokio::process::Command::new(...)`
- `std::process::Command::new(...)`
- `StdCommand::new(...)`
- `JobStore::create_job(...)`
- `.spawner().send(...)` / `.spawner().send_and_wait(...)`
- `BackgroundScheduler::spawn_loop(...)`
- `test_runner::runner::resolve_and_run_test`
- `executor_kind_for_job(...)`
- `dispatch_to_test_runner`
- `hardened_git_command`

For each match, the guard requires either:

- a whole-file `[[site]]` classification in the manifest, OR
- an inline `// execution-ownership: <owner>` annotation on the
  matching line, OR
- the file's parent directory is in the whole-file exemption list
  (this only applies to files whose manifests are checked in).

Adding a new unclassified site fails CI.

## Canonical process lifecycle

Finite local process execution converges on `ManagedProcessService` in
`src/managed_process.rs`. It owns typed argv execution, explicit cwd/env,
process groups, cancellation and timeout, bounded capture/streaming,
sandbox-helper coordination, classification, cleanup, and reaping. Scheduler
executors admit durable work; the service runs the accepted attempt. Human
shells, formatters, hooks, RTK, IDE helpers, terminal tools, and plugin finite
children use adapters over this lifecycle. The full diagram and exception
inventory are in
[`architecture/process-tool-execution-ownership.md`](../architecture/process-tool-execution-ownership.md).

The guard is fail-closed for malformed manifests, forbidden owner classes,
unreadable production source, unclassified spawn/dispatch sites, unbounded
finite-process collection, and lossy argv reparsing. Its `--self-test` checks
one negative output-collection fixture and one negative argv fixture; the
normal invocation scans the checked-in source and manifest.

For the finite scheduler-governed command surfaces (`src/tool/bash.rs`,
`src/python_script/executor.rs`, and `src/scheduler/executors.rs`), the guard
also rejects `.output()`, `.wait_with_output()`, and direct process creation
that bypasses `ManagedProcessService`. Run
`python3 scripts/check_execution_ownership.py --self-test` to verify the
negative fixture remains rejected. The managed-process service itself is the
only direct-spawn owner for these paths. Sandbox helper launches remain private
one-shot plumbing beneath that service: helper identity is installation-owned,
the launch spec is an owner-only temporary file outside target `cwd`, and
setup/exec status travels over a bounded descriptor that is closed before the
target `exec`. The Python interpreter-prefix probe is an annotated, 64 KiB-capped
setup probe; it is not the user execution path.

## Remote execution without local spawn

`src/scheduler/eggwork.rs` (`EggworkExecutor`, M001–M002a) executes
explicitly targeted finite jobs on a named Eggwork node. It is covered
by the `src/scheduler/` manifest entry and introduces no local
process-spawn owner: all execution happens remotely via `eggwork-client`
(mutual TLS, bounded workspace snapshot, idempotent submit, lease
renewal, remote cancel). Scheduler admission, the permit lifetime, and
attempt provenance apply unchanged; the permit spans the whole remote
lifetime and remote failure never falls back to local execution. The
companion static guard `scripts/check_eggwork_target_routing.py`
pins target-first routing, crate confinement, and handle persistence.

### Operator policy

Named nodes are configured under `[eggwork.nodes.<name>]` in daemon
configuration. Keep private-key material in the referenced file; CodeGG
redacts the configured key reference in diagnostics. For example:

```toml
[eggwork.nodes.build-linux]
node_id = "build-linux"
endpoint = "https://build-node.example:7443"
ca_cert_path = "/etc/codegg/eggwork-ca.pem"
client_cert_path = "/etc/codegg/eggwork-client.pem"
client_key_path = "/etc/codegg/eggwork-client-key.pem"
isolation_policy = "required"
network_policy = "unrestricted"
```

`isolation_policy = "required"` requires the named node to advertise the
qualified `workspace_rw` Landlock feature in both authenticated capability
and status responses. If the node cannot satisfy it, the attempt fails before
workspace upload; it never falls back to an unsandboxed run or another node.
The live Linux qualification uses Eggwork's `TrustedLandlockSetup` and checks
that workspace read/write succeeds, outside-workspace read/write is denied,
and terminal evidence is `SandboxResult::Applied { profile: "workspace_rw" }`.

`network_policy = "unrestricted"` is the explicit supported mode. CodeGG
continues to reject `network_policy = "disabled"` before upload because the
qualified Eggwork backend does not provide network isolation. Existing node
profiles default to `isolation_policy = "none"` and
`network_policy = "unrestricted"`; those choices remain visible in operator
posture diagnostics. A policy change applies to future attempts and does not
alter the durable target selected by an existing job.

## Migration trajectory

The deferred-domain-executor sites are documented compatibility
surfaces today. Their follow-up plans convert each into a scheduler
executor with a typed `JobKind` (Git, Python, Plugin, etc.) without
disrupting the canonical subsystem ownership:

| Domain | Status |
|---|---|
| Git (mutations, network, recovery, reads) | Typed mutations (Phase D), network/destructive (Phase E), conflicts/recovery (Phase F) — see `architecture/git.md` |
| Python script execution | Module-based scripting at `src/python_script/` — see `architecture/python_scripting.md` |
| External formatters | Deferred domain executor — tracked in `docs/execution-ownership.toml` |
| Plugin process lifecycle | Finite child lifecycle is canonical; plugin admission/lifecycle remains deferred — tracked in `docs/execution-ownership.toml` |

These domains are tracked in the execution-ownership TOML manifest and
documented in `architecture/`.
