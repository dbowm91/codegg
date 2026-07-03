# Shell Output Projection Phase 1: Command Event Model and Raw Output Retention

## Objective

Introduce a structured command-output event model for codegg's shell execution path. Shell command output should no longer be treated as a plain transcript blob that is directly inserted into model context. Every command run should produce a durable event with raw stdout/stderr retained out-of-band and stable handles that later projection, TUI, model tooling, and expansion paths can reference.

This phase is the foundation for native projection, optional RTK integration, redaction, context budgeting, and raw-output expansion.

## Current Problem

A shell execution path that directly forwards terminal text into the model context has several correctness and product risks:

1. Long build/test/search output can dominate context.
2. Naive truncation can hide the real failure cause.
3. Lossy compression cannot be audited if raw output is not retained.
4. Rerunning commands to recover detail is unsafe for flaky, time-sensitive, or side-effecting commands.
5. The TUI and model do not have a stable way to request exact raw output by range or stream.
6. Future RTK support would be forced into command wrapping instead of a controlled projection pipeline.

The first correction is to represent command execution as structured data.

## Design Direction

Add a domain object that captures one shell command execution. Suggested names:

- `CommandRun`
- `ShellCommandRun`
- `CommandEvent`
- `CommandOutputArtifact`

The exact name should match nearby codegg conventions, but the object should represent a completed or partially completed command run, not only its model-visible text.

A minimal structure:

```rust
pub struct CommandRun {
    pub id: CommandRunId,
    pub command: String,
    pub argv: Option<Vec<String>>,
    pub cwd: PathBuf,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub exit: CommandExit,
    pub stdout: OutputHandle,
    pub stderr: OutputHandle,
    pub combined: Option<OutputHandle>,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub stdout_lines: Option<u64>,
    pub stderr_lines: Option<u64>,
    pub projection: Option<ProjectionHandle>,
    pub redaction: RedactionState,
}
```

Exit state should distinguish ordinary exit codes from signals, timeouts, spawn failures, cancellation, and internal execution errors:

```rust
pub enum CommandExit {
    Code(i32),
    Signal { signal: i32 },
    Timeout,
    Cancelled,
    SpawnFailed { message: String },
    InternalError { message: String },
}
```

Output handles should be stable within a session and should not require rerunning the command:

```rust
cmd://42/stdout
cmd://42/stderr
cmd://42/combined
cmd://42/raw?stream=stderr&range=12000..18000
```

The raw output backing store can start simple. An in-memory session store is acceptable for the first pass if it is bounded and guarded. The design should not preclude later spill-to-disk storage.

## Implementation Steps

### 1. Locate the shell execution boundary

Find the code path that executes shell commands or planned `!command` style commands. Identify the point where stdout/stderr are collected and where command output is passed to the model or transcript.

The goal is not to redesign command execution in this phase. The goal is to interpose a durable command event between command execution and model ingestion.

### 2. Add command IDs and output handles

Add a monotonic `CommandRunId` scoped to the session or workspace. It should be stable for the lifetime of the session. It does not need to be globally unique across codegg runs in this phase.

Add output handle types rather than passing raw strings everywhere:

```rust
pub struct CommandRunId(pub u64);

pub enum CommandOutputStream {
    Stdout,
    Stderr,
    Combined,
}

pub struct OutputHandle {
    pub command_id: CommandRunId,
    pub stream: CommandOutputStream,
}
```

### 3. Add raw output storage

Create a storage component for raw command output. Suggested names:

- `CommandOutputStore`
- `ShellOutputStore`
- `CommandArtifactStore`

Minimum API:

```rust
impl CommandOutputStore {
    pub fn insert(&mut self, command_id: CommandRunId, stdout: Vec<u8>, stderr: Vec<u8>) -> OutputHandles;
    pub fn get_stream(&self, handle: OutputHandle) -> Option<&[u8]>;
    pub fn get_range(&self, handle: OutputHandle, range: Range<usize>) -> Option<&[u8]>;
    pub fn byte_len(&self, handle: OutputHandle) -> Option<usize>;
}
```

If combined output ordering is already available from the execution layer, preserve it. If not, combined output can initially be synthesized as stdout followed by stderr with a clear marker. Do not pretend synthesized combined output preserves terminal interleaving.

### 4. Record metadata at execution time

For each command run, capture:

- original command string
- cwd
- start time
- duration
- stdout byte length
- stderr byte length
- exit state
- whether output decoding was valid UTF-8
- whether output was too large for immediate model inclusion

Line counts are useful but can be lazy or optional to avoid repeated full scans on large outputs.

### 5. Replace direct model insertion with a placeholder projection call

At the end of this phase, the model-facing content can still be raw or conservatively truncated, but it should be produced by a single placeholder function such as:

```rust
fn default_command_projection(run: &CommandRun, store: &CommandOutputStore) -> String
```

This function is temporary. Phase 2 will replace it with the real projector trait. The important part is that all model-visible command output now flows through one projection boundary.

### 6. Add expansion plumbing stubs

Add enough plumbing that later code can resolve handles such as `cmd://42/stdout`. The TUI does not need a polished expansion UI yet, but there should be a testable function for resolving a command ID and stream into bytes or text.

## Raw Output Retention Limits

Add conservative size controls from the beginning. Suggested initial defaults:

```rust
const COMMAND_OUTPUT_MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
const COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES: usize = 32 * 1024 * 1024;
```

If output exceeds limits, store the retained prefix/tail and mark the artifact as partial. Do not silently pretend the raw output is complete.

A better later design can spill large output to disk. This phase only needs to avoid unbounded memory growth.

## TUI Behavior

The transcript can continue to show command output as before, but it should be generated from the command event. Add a small metadata line if it is easy to do without disrupting current UX:

```text
Command 42 exited 101 in 2.14s; stdout 18.2 KiB, stderr 41.9 KiB; raw retained.
```

This display can be refined in Phase 4. The important behavior is that the TUI is no longer the only place where command output exists.

## Tests

Add unit tests for:

1. Command IDs are unique and monotonic within a session.
2. Raw stdout and stderr are stored separately.
3. Output handles resolve to the correct bytes.
4. Range lookup returns the requested slice and rejects invalid ranges.
5. Exit state preserves non-zero exit codes.
6. Spawn failure and timeout states are representable.
7. Oversized output is marked partial instead of silently truncated.

Add an integration-style test if the shell execution layer is already easy to exercise:

- Run a command that writes stdout and stderr.
- Verify the resulting `CommandRun` has both streams and a usable projection.

## Success Criteria

- Every shell command creates a structured command event.
- Raw stdout and stderr are retained out-of-band, within configured limits.
- Model-facing command output is derived through a single projection boundary.
- The command event records exit state, duration, cwd, and output byte counts.
- Output handles can resolve raw stdout/stderr without rerunning the command.
- Oversized retained output is explicitly marked partial.
- Existing shell command UX remains functional.

## Non-Goals

- Do not integrate RTK in this phase.
- Do not build all native structured projectors yet.
- Do not build the full TUI expansion panel yet.
- Do not implement model-generated summaries.
- Do not require disk-backed raw output storage unless it is already convenient.

## Risks and Caveats

The largest risk is accidentally changing shell execution semantics while refactoring output handling. Keep this phase narrow. The command should run the same way it currently runs; only output capture, metadata, and storage should change.

Another risk is memory growth. Add explicit caps immediately, even if the later design spills to disk.

The third risk is losing stdout/stderr separation. Preserve streams independently. A combined stream is useful, but it must not replace separate raw streams.
