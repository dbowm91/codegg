# TUI Phase 6: Human Shell UI Correctness and Polish

## Objective

Make the human shell execution UI trustworthy enough for iterative development. Shell command status, exit code, output digest, inclusion, rerun, kill, and ask flows should preserve the real command result and avoid misleading the user or model.

## Current Problem

The human shell path is useful, but the TUI-side handling has at least one correctness risk: shell ask/digest code can treat any `Exited` command as exit code `0` instead of preserving the actual exit code. This can cause failed commands to appear successful in promoted context. Current tests verify that shell output and questions are included, but they do not assert that failure status and exit codes survive.

The shell UI also uses toasts for listing recent commands. That is acceptable as an early implementation, but the command lifecycle should be clearer and should use stable metadata consistently.

## Design Direction

1. Preserve true exit code and status across all shell handling paths.
2. Make output digest behavior consistent for success, failure, timeout, failed-to-start, and killed commands.
3. Add tests that assert failure propagation, not only message inclusion.
4. Keep the current UI shape initially, but prepare the data model for a later shell history dialog.

## Shell State Requirements

Each shell entry should retain and expose:

- command ID
- command string
- cwd
- origin
- start time if available
- elapsed duration
- status: running, exited, timed out, failed to start, killed if represented
- actual exit code when available
- stdout preview/full captured bytes according to capture policy
- stderr preview/full captured bytes according to capture policy
- truncation state if applicable
- promotion state
- promote-after flag

If the existing store lacks any of these fields, add only what is needed for correctness first. The primary requirement for this phase is real exit-code propagation.

## Implementation Steps

### 1. Audit shell entry data model

Inspect `crate::shell::ShellOutputStore`, `ShellEntry`, `ShellStatus`, and runtime event handling. Confirm where exit code is stored when `mark_exited` is called.

If `ShellStatus::Exited` does not carry an exit code but a separate field does, use that field consistently. If exit code is not stored, extend the entry to store `exit_code: Option<i32>` and ensure all mark/insert paths populate it.

### 2. Fix `handle_shell_ask`

Replace logic that maps `ShellStatus::Exited` to `Some(0)` with actual entry exit code.

Target behavior:

```rust
let exit_code = entry.exit_code;
```

If timed out or failed-to-start status has no exit code, pass `None` and let the digest render status explicitly.

### 3. Audit all digest call sites

Check all TUI shell handlers that build `ShellDigest`:

- `handle_shell_include`
- `handle_shell_ask`
- any promote-after completion path
- any shell rerun/list/status display path

All of them should pass the actual exit code and status context. If `ShellDigest::build` only accepts exit code, consider extending it to accept status or adding `ShellDigest::build_from_entry(&entry)` so call sites cannot accidentally lie.

Preferred API:

```rust
impl ShellDigest {
    pub fn build_from_entry(entry: &ShellEntry) -> Self { ... }
}
```

Then TUI handlers should not manually reconstruct status.

### 4. Add failure-oriented tests

Add tests that fail before this fix and pass after it:

1. Insert completed entry with exit code `101`.
2. Call `handle_shell_ask`.
3. Assert promoted user message contains failure evidence and does not imply exit `0`.
4. Call `handle_shell_include` with `summary` or `failure-digest` mode.
5. Assert digest reports failure.
6. Insert successful command with exit code `0` and assert success is not marked as failure.
7. Insert timeout/failed-to-start entry if constructors exist and assert digest reflects status.

Avoid brittle full-string tests. Assert stable substrings such as `exit code 101`, `failed`, `FAILED`, or whatever `ShellDigest` is expected to render.

### 5. Improve shell list display metadata

For the current toast-based list, include stable compact metadata:

```text
[12] done exit=0 1.2s $ cargo check
[13] failed exit=101 3.4s $ cargo test
[14] running 8.1s $ sleep 999
[15] timeout 300s $ cargo test
```

Keep the output compact to avoid huge toasts. Later phases can move this to a scrollable shell history dialog.

### 6. Align shell kill behavior with store state

`handle_shell_kill` removes a running handle and calls `kill()`. Confirm that the store eventually records killed/terminated status. If the runtime emits a shell event after kill, ensure the TUI applies it. If no event is emitted, mark the store entry as killed or failed in the TUI path.

Do not leave killed commands permanently shown as running.

### 7. Ensure rerun keeps intended options

`handle_shell_rerun` should preserve command, cwd if applicable, promote-after behavior, timeout policy, and capture policy. If current rerun only sends command and promote-after, decide whether that is acceptable. For this phase, at minimum document the behavior and ensure rerun does not lose exit/status metadata for the original entry.

## Optional UI Polish

If small, add a simple shell detail dialog or source-preview-style panel for a single command. This is not required for acceptance. The minimum viable improvement is correct status/digest/list output.

## Testing Plan

Unit tests:

1. `handle_shell_ask` preserves nonzero exit code.
2. `handle_shell_include` summary/failure-digest modes preserve nonzero exit code.
3. Successful command does not render as failure.
4. Timeout or failed-to-start entries render as non-success if representable.
5. `handle_shell_list` includes status and exit code where available.
6. Killing a running command removes handle and updates visible/store status if current architecture permits.
7. Rerun dispatch preserves command and promote-after flag.

Manual verification:

1. Run `!cargo test` with a failing test and ask the model why it failed.
2. Confirm the included context reports the failure, not success.
3. Run a successful command and include summary.
4. Run a long command, kill it, and confirm shell list no longer shows it as indefinitely running.
5. Rerun a prior command and verify the new command gets a new ID and correct lifecycle.

## Acceptance Criteria

- Shell ask/include/promote paths use actual exit code, not `0` for every exited command.
- Failure digests for nonzero exit codes are tested.
- Shell list displays useful compact status metadata.
- Killed or terminated commands do not remain misleadingly running in the TUI.
- No shell handler invents success for unknown status.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Full shell history dialog.
- Persistent shell history across app restarts.
- Shell sandboxing/security policy changes.
- Changing the human shell feature's high-level product semantics.
