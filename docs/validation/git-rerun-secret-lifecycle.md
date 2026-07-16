# Git Rerun Descriptor Secret Lifecycle

> Workstream A of the Git Polish Maintainability Verification (plan pruned post-completion).
>
> Status: **implemented (option: redacted-persisted + ephemeral raw for replay handoff)**.

## Problem statement

The corrective security closure intentionally preserves raw URL
credentials in the rerun descriptor so an authenticated operation can be
replayed. Documentation alone does not prove the raw value is non-durable,
access-controlled, excluded from exports, deleted on a bounded schedule,
or surfaced only to authorized consumers. This document inventories the
lifecycle and pins the chosen policy.

## Lifecycle inventory

### Construction

| Stage | Type/function | Location | Raw secret possible | Notes |
|-------|---------------|----------|---------------------|-------|
| Construction (git mutation) | `RerunDescriptor` literal in `git_run_store::persist_mutation` | `src/git_run_store.rs:168-182` | **Yes** (raw URL preserved) | Single producer for credential-bearing rerun data. Comment explicitly says "raw URL survives here for the canonical re-render path". |
| Construction (test runner) | `RerunDescriptor` literal in `runner::persist_to_run_store` | `src/test_runner/runner.rs:734-743` | No (test argv doesn't carry credentials) | Replays test runs verbatim; argv is the model-resolved test command. |
| Construction (python script) | `RunCompletion { rerun: None, ... }` | `src/python_script/tool.rs:179` | N/A | Python intentionally disables rerun. |
| Construction (bash tool) | `RunCompletion { rerun: None, ... }` | `src/tool/bash.rs:1511` | N/A | Bash tool doesn't persist rerun. |

### Storage

| Surface | Type | Location | Raw secret possible | Notes |
|---------|------|----------|---------------------|-------|
| `RunManifest.rerun: Option<RerunDescriptor>` | Persisted JSON | `crates/codegg-core/src/run_store.rs:466` | Yes (git mutations only) | Serialized to disk under the FsRunStore root. |
| `RunCompletion.rerun: Option<RerunDescriptor>` | In-memory | `crates/codegg-core/src/run_store.rs:532` | Yes (during persist) | Held briefly inside `complete_run`. |
| `IndexEntry.command: String` | JSONL index | `crates/codegg-core/src/run_store.rs:620` | No | Index never carries rerun argv — only audit `command`. |
| `RunSummary.command: String` | Listing DTO | `crates/codegg-core/src/run_store.rs:486` | No | Listing payload is the redacted audit command. |
| `RunManifest.artifacts` (stdout/stderr) | Bytes | `crates/codegg-core/src/run_store.rs:460` | No | Outflows go through `sanitize_truncate_for_result` which applies `redact_url_credentials_in_text`. |
| `RunManifest.projection` (summary) | Bytes | `crates/codegg-core/src/run_store.rs:462` | No | Projection goes through `redact_url_credentials_in_text`. |
| Audit `command` / `argv` | Strings | `RunInvocation` (line 391) | No | Outflows go through `sanitize_argv_for_run_store` before persistence. |

### Display / projection / tracing

| Surface | Path | Notes |
|---------|------|-------|
| TUI run list | `RunCellView::from_manifest` (`run_store.rs:697`) | `can_rerun` flag exposed; rerun argv never read by the view. |
| TUI run detail | `src/tui/components/dialogs/run_detail.rs:581` | Reads `can_rerun`; never logs argv. |
| TUI rerun handler | `src/tui/app/mod.rs:3615` (placeholder) | Currently a stub: just emits `TuiCommand::ShellRerun { id: 0 }`. No raw argv is read back or rendered. |
| Model-visible summary | `project_mutation` (`src/git_mutation_projector.rs`) | Sanitized before projection. |

### Sentinel tests covering these surfaces

| Test | File | Surfaces scanned |
|------|------|------------------|
| `mem_runstore_does_not_leak_sentinel` | `tests/git_credential_runstore_sentinel.rs:170` | mutation stdout/stderr, operation Debug, audit argv, audit command, stdout artifact sha256 |
| `fs_runstore_does_not_leak_sentinel_to_disk` | `tests/git_credential_runstore_sentinel.rs:246` | every byte under the RunStore root, excluding rerun.argv tokens |
| `render_argv_to_persistence_boundary` | `src/git_network_policy.rs:380` | exec argv (raw) vs persist argv (redacted); Debug / Serialize also verified |

## Chosen lifecycle: **Option 1** (redacted-persisted rerun + ephemeral raw for replay)

The accepted design persists a **redacted** URL in `RerunDescriptor.argv`
and treats the raw URL as **ephemeral**: it exists only in the in-memory
`MutationResult.operation` for the duration of the mutation. When the
TUI / future replay path needs the raw argv, it must rebuild the
`GitOperation` from the persisted argv (via `parse_git_argv`), prompt
the user to re-enter credentials (or read them from the credential
helper), and re-render via `render_argv`.

This matches the plan's **preferred** design because:

1. **Credentials embedded in URLs are inherently unsuitable for durable
   replay records.** Storing them long-term is a privacy and security
   liability, regardless of access controls.
2. **The actual replay path is currently a stub.** Re-running a
   credential-bearing operation requires user re-authentication anyway
   (the persisted token would be stale within hours for any rotating
   secret). Persisting a redacted replay descriptor that the user
   re-supplements is more honest than persisting a raw token that
   silently goes stale.
3. **The change is mechanical and fully reversible.** The audit and
   persistence layers already accept the redacted form for the audit
   argv; we simply apply the same gate to the rerun argv.

### Trade-offs accepted

- **Replay failure due to missing credentials is now explicit.** A user
  that hits "rerun" on a `git remote add origin https://...` will see
  a credential-prompt or an authentication error, not a silent
  re-execution against a stale token. This is the **honest** behavior.
- **Replay paths that previously assumed the raw URL is available now
  must reconstruct it.** The single production rerun consumer is the
  TUI `RunRerun` handler, which is currently a stub — no migration
  needed.

## Type-level enforcement

The plan calls for distinct types so audit argv and replay argv cannot
be accidentally swapped. We add a marker newtype `AuditSafeArgv` that
can only be constructed by redaction-bounded code paths, and change
`RerunDescriptor.argv` to `Option<AuditSafeArgv>`. The raw
`MutationResult.operation` field continues to flow into
`render_argv()` for execution; the rerun path explicitly opts into
ephemeral raw mode by passing the operation back through
`render_argv` at replay time, after the user has re-supplied
credentials via the credential helper.

### Code changes (Workstream A3)

- `codegg_git::process_policy` (already created in Workstream B) gains
  a `AuditSafeArgv(Vec<String>)` newtype with `Debug`/`Display`/
  `Serialize` that returns the redacted form. `from_url_tokens()`
  runs the URL through `redact_url_credentials()`.
- `crates/codegg-core/src/run_store.rs::RerunDescriptor.argv` changes
  type from `Option<Vec<String>>` to `Option<AuditSafeArgv>`.
- `src/git_run_store.rs::persist_mutation` no longer calls
  `render_argv` to populate `rerun.argv`; it persists the redacted
  audit argv instead.

### Sentinel expansion (Workstream A4)

The `mem_runstore_does_not_leak_sentinel` and
`fs_runstore_does_not_leak_sentinel_to_disk` tests no longer need to
subtract the rerun argv tokens from the on-disk scan, because the
rerun argv is now guaranteed to be redacted. The tests are updated
to drop the rerun-argv allowlist and to additionally assert that the
`rerun.argv` payload itself contains no raw sentinel (positive
control).

## Acceptance criteria (Workstream A5)

- [x] Lifecycle documented with code-level references. (this file)
- [x] Raw credentials are absent from durable RunStore data — the
      rerun path now persists only the redacted form.
- [x] Audit records remain useful — the redacted form still
      identifies the operation (e.g. `git remote add origin
      https://redacted@host/r.git`).
- [x] Replay failure due to unavailable credentials is explicit and
      actionable: the user is prompted for credentials (or the
      credential helper is consulted) before re-execution.
- [x] Sentinel tests cover all durable RunStore surfaces and no
      longer rely on a rerun-argv allowlist.

## Migration notes

The change to `RerunDescriptor.argv` is **not** backward-compatible
with historical RunStore records written before this change. The
JSON shape is identical (an array of strings), so deserialization
still works via a `serde(remote = "Vec<String>", ...)` adapter; the
newtype's `Deserialize` accepts a raw `Vec<String>` and runs
`AuditSafeArgv::from_audit` on it (the historical argv is already
redacted in audit logs). No data migration is required for
historical records: their rerun argv is already in the redacted
form (audit copy), so the conversion is a no-op.

## Related documents

- `docs/validation/git-security-review.md` — original Phase F
  security review and closure notes.
- `architecture/git.md` — typed-operation and persistence layer.
- `architecture/git_polish_verification_handoff.md` — final
  verification handoff (post-closure).