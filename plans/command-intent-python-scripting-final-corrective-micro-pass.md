# Command Intent and Python Scripting Final Corrective Micro-Pass

## Objective

Close the remaining correctness gaps in the guarded command-intent and Python-scripting MVP before any future active-routing implementation. This pass is deliberately small and corrective. It should not expand the feature surface, enable active routing, or introduce new execution families.

The previous tightening and validation/polish passes brought the repo into a credible guarded-MVP state. The remaining issues are narrow but important:

- workspace containment still uses process `current_dir()` in safety-critical checks;
- Python Analyze mode still treats broad file I/O as write-like denial rather than distinguishing workspace reads from writes;
- the Python AST scanner is AST-based but does not resolve common import aliases/from-import call aliases;
- validation evidence needs to be explicit for the risky cases.

## Current state to preserve

Preserve the following behavior:

- `src/python_script/` remains the canonical Python scripting implementation.
- active command routing remains disabled/observe-only by default.
- complex shell remains raw shell.
- parsed argv is preserved for simple commands.
- BashTool execution backend does not change as part of this pass.
- Python execution retains timeout, env clearing, kill-on-drop, all-mode snapshots, Analyze/Verify write violation detection, Transform diff generation, and pseudo-local labels unless real artifact storage is explicitly implemented later.

## Non-goals

- Do not enable active command routing.
- Do not add Python network/dependency-install privileges.
- Do not implement a full sandbox.
- Do not add new command families.
- Do not replace the shell parser.
- Do not implement real artifact storage in this pass unless it falls out naturally from existing APIs; pseudo-local labels are acceptable if clearly documented.

## Workstream A: Introduce explicit workspace-root context

### Problem

Python cwd validation and command-intent absolute-path checks still use `std::env::current_dir()` as the workspace root. That is not reliable for codegg sessions, multiple worktrees, daemonized execution, tests running from non-workspace cwd, or future active routing.

### Required changes

1. Add a small explicit context type for command intent classification:

```rust
pub struct CommandIntentContext {
    pub workspace_root: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
}
```

2. Keep the existing compatibility API:

```rust
pub fn classify_command(command: &str) -> CommandIntent
```

This should delegate to:

```rust
pub fn classify_command_with_context(command: &str, context: &CommandIntentContext) -> CommandIntent
```

When no context is supplied, fallback to the current process-cwd behavior for compatibility.

3. Add helper logic:

```rust
fn canonical_workspace_root(context: &CommandIntentContext) -> Option<PathBuf>
fn path_is_inside_workspace(path: &Path, context: &CommandIntentContext) -> bool
fn absolute_path_outside_workspace(path: &str, context: &CommandIntentContext) -> bool
```

4. Use the contextual workspace root for:

- `classify_search` absolute path filtering;
- `classify_file_read` absolute path filtering;
- any future helper that decides whether a command path is workspace-contained.

5. Extend `PythonScriptRequest` with an optional workspace root, or introduce a `PythonExecutionContext` if that fits better:

```rust
pub struct PythonScriptRequest {
    pub code: String,
    pub mode: PythonExecutionMode,
    pub cwd: PathBuf,
    pub workspace_root: Option<PathBuf>,
    ...
}
```

6. Update Python `validate_cwd` to accept an explicit workspace root:

```rust
fn validate_cwd(cwd: &Path, workspace_root: Option<&Path>) -> Result<PathBuf, String>
```

Rules:

- canonicalize cwd;
- canonicalize explicit root if provided;
- if no explicit root, use process cwd as compatibility fallback;
- reject cwd outside canonical root;
- return canonical cwd.

7. Update serialization tests and constructors for `PythonScriptRequest` to include the new field with `#[serde(default)]` if necessary.

### Acceptance criteria

- Existing `classify_command()` tests continue to pass.
- New `classify_command_with_context()` tests prove explicit root is honored.
- `cat /absolute/path/inside/root/file` can classify as `FileRead` when explicit root contains it.
- `cat /absolute/path/outside/root/file` does not classify as safe read.
- Python cwd outside explicit root is denied even if process cwd would otherwise allow it.
- Python cwd inside explicit root is accepted even when process cwd differs in tests.

## Workstream B: Fix Python file-read vs file-write capability semantics

### Problem

`PythonRiskAssessment` tracks `has_file_read` and `has_file_write`, but `has_denied_capabilities()` still denies `write_workspace` for broad `has_file_io`. This makes Analyze mode reject legitimate workspace-read scripts.

### Required changes

1. Update `PythonCapabilityEnvelope::has_denied_capabilities()`:

Current problematic logic:

```rust
if risk.has_file_io && !self.write_workspace {
    denied.push("write_workspace".to_string());
}
```

Replace with semantics equivalent to:

```rust
if risk.has_file_read && !self.read_workspace {
    denied.push("read_workspace".to_string());
}
if risk.has_file_write && !self.write_workspace {
    denied.push("write_workspace".to_string());
}
```

2. Keep destructive filesystem separate:

```rust
if risk.has_destructive_ops && !self.destructive_fs {
    denied.push("destructive_fs".to_string());
}
```

3. Ensure Analyze mode:

- permits workspace reads;
- denies writes detected statically;
- still catches undetected writes post-execution through snapshot diff.

4. Ensure Transform mode:

- permits non-destructive workspace writes;
- denies destructive filesystem operations;
- denies network/dependency install/subprocess by default.

5. Ensure Verify mode:

- permits workspace reads;
- permits subprocess;
- denies writes statically and through post-execution snapshot.

6. Update projection/error text so denied capabilities accurately say `read_workspace`, `write_workspace`, or `destructive_fs`.

### Acceptance criteria

- `Analyze` with `from pathlib import Path; Path('Cargo.toml').read_text()` is not denied by pre-execution capability checks.
- `Analyze` with `Path('x').write_text('y')` is denied pre-execution.
- `Analyze` with a write pattern missed by static analysis fails after snapshot comparison.
- `Transform` with `write_text` succeeds and reports changed files/diff.
- `Transform` with `unlink`, `rmtree`, `os.remove`, or `Path.unlink` is denied.
- `Verify` with subprocess and no writes is allowed.
- `Verify` with writes is denied or fails post-run.

## Workstream C: Add AST alias/from-import resolution

### Problem

The Python AST scanner currently records base imports and raw dotted call names, but it does not resolve aliases. Common forms like these may be underclassified:

```python
import subprocess as sp
sp.run(['ls'])

from subprocess import run
run(['ls'])

import os as o
o.remove('x')

from pathlib import Path
Path('x').write_text('y')
```

### Required changes

1. In the inline AST scanner, build an alias map:

- `import subprocess as sp` => `sp -> subprocess`;
- `import os as o` => `o -> os`;
- `import pathlib as pl` => `pl -> pathlib`;
- `from subprocess import run` => `run -> subprocess.run`;
- `from subprocess import Popen as P` => `P -> subprocess.Popen`;
- `from pathlib import Path` => `Path -> pathlib.Path` or `Path -> Path` with special handling;
- `from os import remove as rm` => `rm -> os.remove`.

2. Resolve `ast.Name` and leading segments of `ast.Attribute` through the alias map before classification.

Examples:

- raw call `sp.run` resolves to `subprocess.run`;
- raw call `run` resolves to `subprocess.run`;
- raw call `o.remove` resolves to `os.remove`;
- raw call `Path(...).write_text` resolves sufficiently to mark file write;
- raw call `pl.Path(...).write_text` resolves sufficiently to mark file write.

3. Keep comments and string literals from triggering subprocess/network/destructive risk unless they are passed to suspicious subprocess/dependency-install contexts already covered by the scanner.

4. Syntax errors should remain conservative: parse-error flag plus fallback string scan.

5. Add unit tests for alias cases in the Rust test module that calls `analyze_python_risk()`.

### Acceptance criteria

The following cases are detected correctly:

- `import subprocess as sp; sp.run(['ls'])` => `has_subprocess`.
- `from subprocess import run; run(['ls'])` => `has_subprocess`.
- `from subprocess import Popen as P; P(['ls'])` => `has_subprocess`.
- `import os as o; o.remove('x')` => `has_destructive_ops`.
- `from os import unlink; unlink('x')` => `has_destructive_ops`.
- `from pathlib import Path; Path('x').write_text('y')` => `has_file_write`.
- `from pathlib import Path; Path('x').read_text()` => `has_file_read` and not `has_file_write`.
- `import pathlib as pl; pl.Path('x').write_text('y')` => `has_file_write`.
- A comment/string containing `subprocess.run(['rm'])` alone does not set `has_subprocess`.

## Workstream D: Add targeted validation evidence

### Problem

The repo has many tests, but this line of work needs targeted coverage that proves the remaining safety semantics. The next implementer should add specific tests and record what was run.

### Required tests

Add or update tests for:

#### Command intent workspace root

- explicit root inside/outside cases;
- absolute path inside explicit root accepted;
- absolute path outside explicit root rejected from safe read/search;
- compatibility wrapper behavior unchanged.

#### Python mode/capability semantics

- Analyze read allowed;
- Analyze write denied pre-execution;
- Analyze undetected write caught post-execution;
- Transform write allowed and diff generated;
- Transform destructive denied;
- Verify subprocess allowed;
- Verify write denied or post-run violation.

#### Python AST aliasing

- subprocess aliases;
- os aliases;
- pathlib aliases;
- from-imports;
- string/comment false-positive avoidance;
- syntax-error fallback.

#### Routing remains observe-only

- BashTool metadata does not alter execution backend;
- default config does not enable active routing.

### Validation commands

Run the narrow validation set first:

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_intent::shell_shape
cargo test -p codegg --lib python_script
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib command_routing
```

Then run the broader known-relevant set:

```bash
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib test_runner::custom
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

Finally, if targeted tests pass, run the capped full suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

If any of the above cannot be run due to environment constraints, document exactly which commands were run and which were skipped.

## Workstream E: Keep active routing explicitly deferred

### Problem

The substrate is getting close to usable for routing, but active command routing still should not be enabled in this pass.

### Required changes

1. Ensure documentation states that this pass is still pre-active-routing.
2. Ensure default config remains off/observe-only.
3. Ensure any routing tests validate decision metadata, not actual backend switching from BashTool.
4. If an active-routing config mode exists, leave it gated and undocumented as production-ready.

### Acceptance criteria

- No default config enables active routing.
- BashTool backend behavior remains raw shell unless a future explicit routing pass changes it.
- Architecture docs identify the next step as an active-routing implementation pass, not as complete.

## Recommended implementation order

1. Add explicit workspace-root context types and Python request field.
2. Update command-intent path checks to use context.
3. Update Python cwd validation to use explicit root.
4. Fix Python file-read/file-write capability denial semantics.
5. Extend AST scanner alias resolution.
6. Add targeted tests for all acceptance criteria.
7. Update docs to record that pseudo-local Python labels remain non-resolvable and routing remains observe-only.
8. Run targeted validation commands and document results.

## Done criteria

This micro-pass is complete when:

- safety-critical path checks no longer depend solely on process cwd when explicit root is available;
- Python Analyze can perform workspace reads without being denied as a write operation;
- Python writes/destructive operations are denied or caught according to mode;
- AST alias/from-import cases are covered;
- command-intent risk metadata remains non-noisy;
- routing remains observe-only by default;
- targeted tests are present and validation commands have been run or explicitly documented as skipped.
