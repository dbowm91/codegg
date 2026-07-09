# Command Intent and Python Scripting Validation/Polish Pass

## Objective

Perform a small targeted validation and polish pass after the tightening implementation. The prior pass corrected the large structural issues: duplicate Python scripting code was removed, shell-shape parsing was introduced, git/search classification was tightened, Python execution gained mode enforcement, and Python risk analysis moved toward AST-aware scanning. This follow-up should close the remaining narrow gaps before any active command routing is considered.

The goal is not broad feature expansion. The goal is to make the current guarded MVP internally consistent, test-backed, and explicit about what remains observe-only.

## Current state to preserve

The repo is now in a better posture and should preserve these properties:

- `src/python_script/` is the canonical Python scripting subsystem.
- `src/python_scripting.rs` has been removed.
- command classification now uses `ShellShape` and parsed argv for simple commands.
- complex shell remains raw shell rather than being routed to structured backends.
- git branch/tag/remote/stash classification is substantially safer than broad prefix matching.
- Python executor performs pre-execution capability checks, cwd validation, env clearing, all-mode snapshots, Analyze/Verify write violation detection, Transform diff generation, and output projection metadata.
- `CommandIntentConfig` should remain disabled/observe-only by default.

## Non-goals

- Do not enable active command routing by default.
- Do not implement full shell execution routing through native/test/search backends unless it is behind explicit test-only paths.
- Do not add Python network or dependency-install capabilities.
- Do not attempt perfect Python sandboxing.
- Do not expand supported command families beyond the current classifier scope.

## Workstream A: Replace process-cwd workspace authority with explicit workspace root

### Problem

Several safety checks currently anchor workspace containment to `std::env::current_dir()`. That is acceptable for local unit tests but insufficient for codegg sessions, where the authoritative workspace/worktree root may be session-specific and not equal to the process cwd.

Affected areas include:

- Python executor cwd validation.
- command-intent file/search path classification.
- absolute-path outside-workspace checks.
- any later managed-process/native routing decisions.

### Required changes

1. Introduce a small reusable `WorkspaceRoot`/`WorkspacePolicy` helper or pass an explicit root into relevant classifiers/executors.
2. For Python:
   - add `workspace_root: Option<PathBuf>` to `PythonScriptRequest`, or an execution context struct if that fits repo style better;
   - default to current dir only when no session/worktree root is available;
   - canonicalize both cwd and workspace root;
   - reject cwd outside workspace root.
3. For command intent:
   - keep `classify_command(command: &str)` as a compatibility wrapper;
   - add `classify_command_with_context(command, CommandIntentContext { workspace_root, cwd, ... })`;
   - use the contextual root for absolute-path classification.
4. Update BashTool routing metadata path to use available `workdir`/config context where possible.
5. Document that active routing must use contextual classification, not bare process-cwd classification.

### Acceptance criteria

- Python cwd containment tests use an explicit temp workspace root.
- A cwd outside the explicit root is denied even if it is inside process cwd.
- File/search absolute path classification uses explicit workspace root when supplied.
- Existing no-context tests continue to pass through the compatibility wrapper.

## Workstream B: Separate Python file-read and file-write capability semantics

### Problem

The AST risk scanner now distinguishes `has_file_read` and `has_file_write`, but the capability denial path still treats broad `has_file_io` as requiring `write_workspace`. This makes Analyze mode too restrictive: read-only analysis scripts such as `Path('Cargo.toml').read_text()` may be denied even though Analyze is intended to allow workspace reads.

### Required changes

1. Update `PythonCapabilityEnvelope::has_denied_capabilities()` to use:
   - `has_file_read` with `read_workspace`;
   - `has_file_write` with `write_workspace`;
   - destructive ops with `destructive_fs`;
   - outside-workspace reads/writes when that information exists.
2. Keep Analyze mode read-only:
   - allow workspace reads;
   - deny workspace writes;
   - still use all-mode snapshots to catch unpredicted writes.
3. Transform mode should allow non-destructive workspace writes, but still deny destructive ops, network, dependency install, and subprocess by default.
4. Verify mode should allow workspace reads and subprocess, but deny writes unless a later explicit policy says otherwise.
5. Update projection messages so denied capabilities distinguish `read_workspace`, `write_workspace`, and `destructive_fs` instead of generic `write_workspace` for all file I/O.

### Acceptance criteria

- Analyze script using `Path('file').read_text()` is allowed if the file is under workspace.
- Analyze script using `Path('file').write_text()` is denied before execution when detected.
- Analyze script with an undetected write fails after snapshot comparison.
- Transform script using `write_text()` is allowed and reports changed files/diff.
- Transform script using `unlink()`/`rmtree()` is denied.
- Verify script with subprocess and no writes is allowed under Verify mode.
- Verify script with writes fails or is denied.

## Workstream C: Improve Python AST alias and from-import handling

### Problem

The AST scanner is now primary, but it does not appear to fully resolve aliases such as `import subprocess as sp; sp.run(...)`, `from subprocess import run; run(...)`, or `import os as o; o.remove(...)`. This leaves common bypass-style forms underclassified.

### Required changes

1. Extend the inline AST scanner to build alias maps:
   - import aliases: `import subprocess as sp` => `sp -> subprocess`;
   - from-import aliases: `from subprocess import run as r` => `r -> subprocess.run`;
   - module aliases: `import os as o` => `o -> os`.
2. Resolve call names through these aliases before classification.
3. Handle common `pathlib` forms:
   - `from pathlib import Path`; `Path('x').read_text()` / `write_text()` / `unlink()`;
   - `import pathlib as pl`; `pl.Path('x').write_text()`.
4. Ensure comments and string literals containing dangerous-looking snippets do not trigger AST risk unless used as arguments to dependency install or subprocess-like calls.
5. Keep string scanning fallback explicit and conservative for scanner failures.

### Acceptance criteria

Tests cover at least:

- `import subprocess as sp; sp.run(['ls'])` => subprocess risk.
- `from subprocess import run; run(['ls'])` => subprocess risk.
- `from subprocess import Popen as P; P(['ls'])` => subprocess risk.
- `import os as o; o.remove('x')` => destructive risk.
- `from pathlib import Path; Path('x').write_text('y')` => file write risk.
- `from pathlib import Path; Path('x').read_text()` => file read, not write.
- comment/string-only `subprocess.run(['rm'])` does not trigger subprocess risk.
- syntax error produces conservative fallback/parse-error behavior.

## Workstream D: Make Python artifact handles real or explicitly non-resolvable

### Problem

The Python executor now emits pseudo handles such as `python_run://{run_id}/stdout`, `stderr`, and `diff`. These are useful metadata, but unless they are registered in the existing context/artifact store they are not true expansion handles.

### Required changes

Choose one of two acceptable paths.

Preferred path: register real artifacts.

1. Thread an optional artifact store/context store into Python execution or projection.
2. Persist stdout, stderr, script hash/body metadata, and diff into the store.
3. Return handles that can be expanded by existing context-read/artifact tooling.
4. Add tests with an in-memory store.

Fallback path: make non-resolvable handles explicit.

1. Rename fields or metadata to `run_label`/`pseudo_handle`/`local_handle`.
2. Document that they are not yet expansion handles.
3. Avoid presenting them as `ctx://` or as guaranteed retrievable artifacts.

### Acceptance criteria

- Model-facing projection does not imply that unresolved handles are expandable.
- If real artifacts are implemented, `context_read` or the relevant expansion tool can retrieve them in tests.
- If fallback is chosen, docs and field names are explicit about non-resolvability.

## Workstream E: Reduce command-intent risk/coarse capability noise

### Problem

Generic `RiskAssessment::low()` and `RiskAssessment::medium()` still include `ExecutionCapability::Subprocess` by default. This may make planner permission metadata noisy or misleading, especially for commands that do not actually spawn additional subprocesses beyond their own managed execution.

### Required changes

1. Split generic constructors into more precise helpers:
   - read-only risk;
   - raw shell risk;
   - managed process risk;
   - git mutation risk;
   - destructive filesystem risk;
   - network/dependency risk.
2. Ensure `Subprocess` means the command or script may spawn child processes beyond the primary planned execution, not simply that codegg will execute a process.
3. Update planner permission request generation to avoid asking/recording irrelevant subprocess permission for safe read-only or known native routes.
4. Keep raw shell/complex shell higher-risk than managed argv/native routes.

### Acceptance criteria

- `git status` has no subprocess permission request.
- `rg 'foo' src/` does not receive misleading subprocess risk unless managed-process policy intentionally models primary process execution that way.
- complex shell remains medium/high risk with shell-eval style capability.
- git mutation includes `GitMutation`.
- destructive file commands include destructive capability.

## Workstream F: Clarify observe-only vs active routing in docs/config/tests

### Problem

BashTool currently attaches routing metadata while still executing raw shell. This is desirable until active routing is safe, but the config names and docs should make that explicit so implementers do not accidentally enable active routing prematurely.

### Required changes

1. Ensure `CommandIntentMode` or equivalent config clearly distinguishes:
   - off;
   - observe/metadata-only;
   - route-safe/active routing.
2. Default must remain off or observe-only, never active routing.
3. Add tests proving that BashTool still executes through raw shell in observe mode.
4. Ensure metadata emitted to model-visible output is bounded and not excessively noisy for trivial commands.
5. Add a short note in `architecture/command_routing.md` that active routing is intentionally deferred until workspace-root and artifact-handle polish is complete.

### Acceptance criteria

- No config combination defaults to active routing.
- Observe mode classification cannot change command execution backend.
- Docs explicitly state the current routing behavior.

## Workstream G: Targeted validation matrix

### Required tests

Add focused tests for the exact remaining risks. Suggested test groups:

#### Workspace root

- explicit workspace root accepted;
- cwd outside root denied;
- absolute path under root accepted;
- absolute path outside root not classified as safe read/search.

#### Python read/write policy

- Analyze read allowed;
- Analyze write denied or post-run violation;
- Transform write allowed and diff generated;
- Verify subprocess allowed;
- Verify write denied or post-run violation;
- network/dependency/destructive denied before execution.

#### AST aliasing

- subprocess aliases;
- os aliases;
- pathlib read/write/unlink;
- comment/string false-positive avoidance;
- syntax-error fallback.

#### Command parsing/classification

- quoted argv preservation;
- redirection/pipe/heredoc complex shell;
- git branch/tag/remote/stash safe vs mutating cases;
- find `-exec`/`-delete` unsafe;
- `cat /etc/passwd` unsafe;
- process cwd does not accidentally authorize outside-workspace paths when explicit root is supplied.

#### Routing posture

- BashTool observe mode emits metadata but still raw-shell executes;
- active routing remains disabled by default;
- route decisions remain inspectable in unit tests.

### Suggested validation commands

Run targeted suites first:

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_intent::shell_shape
cargo test -p codegg --lib command_intent::plan
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib python_script
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib test_runner
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

Then run the capped full suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

If Python plugin/example surfaces are touched:

```bash
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v
```

## Recommended implementation order

1. Add explicit workspace-root context types and compatibility wrappers.
2. Fix Python read/write capability semantics.
3. Add AST alias/from-import resolution.
4. Decide real artifact handles vs explicit pseudo-handle terminology.
5. Reduce command-intent risk/capability noise.
6. Clarify observe-only routing docs/config/tests.
7. Run the targeted validation matrix.

## Done criteria

This polish pass is complete when:

- workspace containment uses explicit root context in the safety-critical paths;
- Python Analyze supports workspace reads but not writes;
- Python Transform supports non-destructive workspace writes and reports diff;
- Python Verify supports subprocess but not writes;
- Python AST risk analysis handles common aliases and from-import forms;
- Python output handles are either real artifacts or explicitly documented as non-resolvable local handles;
- command-intent permission metadata is not materially misleading;
- routing remains observe-only by default;
- targeted tests pass and the capped workspace test command has been run or any failures are documented.
