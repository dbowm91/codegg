# TUI Phase 2: Async File Diff and Sidebar Update Pipeline

## Objective

Remove synchronous file reads and text-diff computation from the TUI event loop. File-change events should update the sidebar immediately, but expensive diff-stat computation must happen in a bounded background pipeline with size caps, binary detection, and stale-result protection.

## Current Problem

`AppEvent::FileChanged` calls `sidebar_diff_stats`, which resolves the changed path, performs `std::fs::read_to_string`, and computes a `similar::TextDiff` inline. This is acceptable for tiny files on a fast local disk, but it is risky for generated files, lockfiles, large source files, remote filesystems, and rapid edit bursts. During this work the TUI cannot process input, resize, streaming redraws, or other events.

## Design Direction

On `FileChanged`, the event loop should perform only cheap state mutation:

1. Record the changed path and action.
2. Mark diff stats as pending or unknown.
3. Render the sidebar promptly.
4. Enqueue bounded background diff-stat work.
5. Apply the result only if it still matches the latest generation for that path.

The diff worker should never mutate `App` directly. It should return `TuiCommand::FileDiffStatsReady` or equivalent through `tui_cmd_tx`.

## Data Model Changes

Extend the changed-file state to represent pending/skipped diff stats. Current code stores additions/deletions as concrete counts. Prefer an enum or optional fields:

```rust
pub enum DiffStatsState {
    Pending { generation: u64 },
    Ready { generation: u64, additions: usize, deletions: usize },
    Skipped { generation: u64, reason: String },
    Error { generation: u64, message: String },
}
```

If a broad type change is too invasive, keep existing `additions`/`deletions` fields and add minimal fields:

```rust
pub diff_generation: u64,
pub diff_pending: bool,
pub diff_skipped_reason: Option<String>,
```

The enum is cleaner, but the smaller change may be easier to land.

## New Command Variant

Add a completion command:

```rust
TuiCommand::FileDiffStatsReady {
    path: std::path::PathBuf,
    generation: u64,
    result: FileDiffStatsResult,
}
```

Where:

```rust
pub enum FileDiffStatsResult {
    Ready { additions: usize, deletions: usize },
    Skipped { reason: String },
    Error { message: String },
}
```

Place these types in a TUI-side module such as `src/tui/file_diff.rs` or near the changed-file/sidebar state types.

## Worker Behavior

Create a helper such as `spawn_sidebar_diff_stats(app, path, old_content, generation)` that clones only immutable inputs:

- project directory
- changed path
- old content, if supplied by the event
- current generation
- `tui_cmd_tx`

The task should:

1. Resolve the absolute path safely.
2. Read metadata before reading file contents.
3. Skip directories and missing files with a clear reason.
4. Skip files larger than the configured threshold.
5. Read only a prefix first for binary detection.
6. Skip binary-ish files.
7. Read the full file only after checks pass.
8. Compute additions/deletions with `similar::TextDiff::from_lines`.
9. Send a completion command.

## Limits

Add constants initially; make them configurable later if needed.

Suggested defaults:

```rust
const SIDEBAR_DIFF_MAX_BYTES: u64 = 1_048_576; // 1 MiB
const SIDEBAR_DIFF_BINARY_PROBE_BYTES: usize = 8192;
```

Binary detection can be conservative:

- If the prefix contains NUL bytes, skip.
- If `std::str::from_utf8` fails for the prefix, skip or attempt lossy only if the project already tolerates lossy text rendering. For sidebar stats, skipping invalid UTF-8 is safer.

## Event Handling Changes

In `AppEvent::FileChanged`:

1. Increment a per-path diff generation.
2. Insert or update the changed-file entry with pending diff state.
3. Update the sidebar immediately with pending state.
4. Spawn the diff task.
5. Set `needs_render = true`.

In `TuiCommand::FileDiffStatsReady` handling:

1. Find the changed-file entry by path.
2. Compare generations.
3. Ignore stale completions.
4. Apply ready/skipped/error state.
5. Refresh `SidebarWidget` file changes.
6. Set `needs_render = true`.

## Sidebar Rendering

Update the sidebar file-change rendering to distinguish:

- Known stats: `+12 -3`
- Pending stats: `diff...`
- Skipped stats: `large`, `binary`, `missing`, or another compact reason
- Error stats: `diff err`

Do not spam toasts for skipped or failed diff stats. This is sidebar metadata, not an operational failure.

## Backpressure and Coalescing

Avoid unbounded diff work when many file events arrive. Start simple:

- Spawn one task per file-change event, but use generation checks to ignore stale results.
- Add a global atomic or semaphore limit if rapid edits cause too many tasks.

Preferred small implementation:

```rust
static DIFF_SEMAPHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(2)));
```

Each diff task acquires a permit before reading files. If the semaphore is unavailable and task count is high, either wait or skip with `queued` behavior. Waiting with a low concurrency limit is acceptable for this phase.

## Testing Plan

Unit tests:

1. Small UTF-8 file returns correct addition/deletion counts.
2. Missing file returns skipped or error without panic.
3. Directory path is skipped.
4. File larger than limit is skipped before full read.
5. Binary file with NUL prefix is skipped.
6. Invalid UTF-8 is skipped or handled according to chosen policy.
7. Stale completion with old generation does not update sidebar state.
8. Current-generation completion updates sidebar state.

Integration/manual tests:

1. Trigger agent edits to a normal source file and verify sidebar stats appear after pending state.
2. Modify a large lockfile and verify TUI remains responsive.
3. Run inside `zellij` and resize while diff stats are pending.
4. Confirm no debug/toast spam for skipped large or binary files.

## Acceptance Criteria

- `AppEvent::FileChanged` no longer performs synchronous `read_to_string` or full text-diff computation on the event loop.
- Large/binary/missing files do not block or panic the TUI.
- Sidebar updates immediately with pending state and later with ready/skipped/error state.
- Stale diff results are ignored.
- Tests cover ready, skipped, error, and stale-generation paths.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Full diff previews or hunk rendering. This phase only covers additions/deletions and sidebar metadata.
- Persistent diff cache.
- File watcher redesign.
- Global background task ownership cleanup, except for minimal bounded task spawning. Full lifecycle cleanup belongs in Phase 7.
