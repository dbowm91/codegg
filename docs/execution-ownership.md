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

`scripts/check_execution_ownership.py` greps every production Rust
file under `src/` and `crates/` for canonical spawn patterns:

- `tokio::process::Command::new(...)`
- `std::process::Command::new(...)`
- `JobStore::create_job(...)`
- `.spawner().send(...)` / `.spawner().send_and_wait(...)`
- `BackgroundScheduler::...spawn_loop(...)`
- `test_runner::runner::resolve_and_run_test`
- `dispatch_to_test_runner`
- `hardened_git_command`

For each match, the guard requires either:

- a whole-file `[[site]]` classification in the manifest, OR
- an inline `// execution-ownership: <owner>` annotation on the
  matching line, OR
- the file's parent directory is in the whole-file exemption list
  (this only applies to files whose manifests are checked in).

Adding a new unclassified site fails CI.

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
| Plugin process lifecycle | Deferred domain executor — tracked in `docs/execution-ownership.toml` |

These domains are tracked in the execution-ownership TOML manifest and
documented in `architecture/`.
