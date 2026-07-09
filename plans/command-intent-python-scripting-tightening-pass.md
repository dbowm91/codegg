# Command Intent and Python Scripting Tightening Pass

## Objective

Tighten the recently landed command intent, command routing, projection, and first-class Python scripting work before enabling active routing. The current repo has the right high-level shape, but several pieces are still prototype-grade: Python safety is mostly advisory, command parsing is stringly, routing is metadata-only, duplicate Python execution paths exist, and several broad classifiers would be unsafe if actual routing were turned on.

This pass should preserve the current conservative default posture while making the substrate correct enough to support future active routing.

## Current state summary

The recent implementation added:

- `src/command_intent/` with classifier, intent kinds, risk levels, capabilities, and planner types.
- `src/command_intent/plan.rs` with `CommandPlan`, `ExecutionBackend`, `ProjectorRoute`, permission requests, RTK eligibility metadata, and timeout selection.
- `src/command_routing.rs` with `RoutingDecision` resolution from command plans.
- `src/python_script/` with Python execution modes, risk analysis, sandbox/capability envelope, executor, snapshot diffing, projection, and model-facing `PythonScriptTool`.
- `src/python_scripting.rs`, an older or parallel Python scripting implementation that duplicates much of `src/python_script/`.
- BashTool routing metadata that classifies/plans commands but still executes through raw shell.
- `CommandIntentConfig` with all routing toggles defaulted off.

The direction is correct, but active execution routing should remain disabled until this tightening pass is complete.

## Non-goals

- Do not enable active command routing by default.
- Do not attempt full POSIX shell parsing.
- Do not claim perfect Python sandboxing on all platforms.
- Do not add network-enabled or dependency-installing Python modes.
- Do not route destructive git/filesystem operations to native tools.
- Do not broaden the model-facing tool surface beyond the already registered `python_script` tool unless required for correctness.

## Workstream A: Consolidate Python scripting modules

### Problem

The repo now exposes both `src/python_script/` and `src/python_scripting.rs`. They define overlapping modes, source types, risk analysis, run results, and execution functions. This creates ambiguity, duplicate behavior, and future maintenance risk.

### Required changes

1. Treat `src/python_script/` as the canonical implementation.
2. Search for all references to `crate::python_scripting`, `python_scripting::`, `PythonScriptMode`, `PythonScript`, and `run_python_script`.
3. If there are no production references, remove `src/python_scripting.rs` and remove `pub mod python_scripting;` from `src/lib.rs`.
4. If references exist, migrate them to `src/python_script::*` and then remove the duplicate module.
5. Keep the model-facing `PythonScriptTool` in `src/python_script/tool.rs` as the only Python script tool.
6. Update architecture docs to name `src/python_script/` as canonical.

### Acceptance criteria

- Only one canonical Python scripting implementation remains.
- `src/lib.rs` no longer exports duplicate Python scripting APIs unless an explicit compatibility shim is intentionally retained.
- No duplicated Python risk/executor tests remain in a second module.
- `cargo test -p codegg --lib python_script` covers the canonical subsystem.

## Workstream B: Make Python mode enforcement authoritative

### Problem

The current Python executor derives a capability envelope, but execution proceeds even when risk analysis indicates denied capabilities. Analyze mode also does not actually detect writes, because pre/post snapshots are only captured for Transform mode. The executor currently reports capabilities but does not consistently enforce them.

### Required changes

1. Run capability compatibility before script execution:
   - call `check_compatibility(mode, code)` or an equivalent function;
   - for denied capabilities, return a policy error before execution unless the mode explicitly permits them;
   - include denied capability names in the model-facing projection.
2. Take pre/post snapshots for Analyze and Verify as well as Transform:
   - Analyze: any workspace change should fail the run and report a policy violation;
   - Verify: any workspace change should fail unless a later explicit option allows generated artifacts;
   - Transform: changes are allowed but must be recorded.
3. Ensure Analyze and Verify do not silently accept writes even when static risk analysis missed the write.
4. Canonicalize and validate `cwd` before execution:
   - cwd must exist;
   - cwd must be a directory;
   - default should be current workspace/current dir;
   - outside-workspace cwd should be rejected unless a future policy explicitly allows it.
5. Add minimal environment isolation similar to BashTool:
   - use `env_clear()`;
   - restore only required development variables such as `PATH`, `HOME`, `LANG`, `LC_ALL`, `VIRTUAL_ENV`, `PYTHONPATH`, and perhaps platform-required variables;
   - record preserved environment policy in docs.
6. For network/dependency install/outside-workspace/destructive filesystem risk, deny in MVP rather than ask.
7. For subprocess risk:
   - deny in Analyze and Transform;
   - allow in Verify only if this remains an explicit mode capability;
   - document that subprocess mediation is not complete yet and that Verify is still bounded by timeout/env/cwd policy.
8. Preserve `kill_on_drop(true)` and timeout behavior.

### Acceptance criteria

- Analyze scripts that write files fail even if static analysis did not predict the write.
- Verify scripts that write files fail unless explicitly allowed by a tested policy path.
- Transform scripts record changed files.
- Scripts with network/dependency/destructive risk are denied before execution.
- Analyze/Transform scripts with subprocess risk are denied before execution.
- Executor clears environment and preserves only a documented allowlist.
- Tests use temp directories and do not rely on global packages beyond Python stdlib.

## Workstream C: Improve Python risk analysis from substring scanning to AST-aware scanning

### Problem

The current risk scanner is string/line based. It detects simple patterns, but misses aliases and common forms such as `from subprocess import run`, `Path.write_text`, `p = __import__('os')`, alias calls, and indirect imports. It can also false-positive on comments or strings.

### Required changes

1. Add an AST-based scanner using a tiny internal Python stdlib script invoked with `python -I` or equivalent where possible.
2. Scanner input should be the script source, passed via stdin or a temp file.
3. Scanner output should be JSON with at least:
   - imports;
   - from-imports;
   - calls by dotted name when resolvable;
   - attribute calls such as `Path(...).write_text(...)` when detectable;
   - dynamic execution indicators;
   - subprocess indicators;
   - network indicators;
   - destructive filesystem indicators;
   - dependency installation indicators;
   - parse errors.
4. Keep the existing substring scanner as fallback only if AST scanning fails, and mark the risk assessment as fallback-derived.
5. Update `PythonRiskAssessment` to distinguish at least:
   - `has_file_read` vs `has_file_write` if practical;
   - `has_subprocess`;
   - `has_network`;
   - `has_dependency_install`;
   - `has_destructive_ops`;
   - `has_dynamic_execution`;
   - `parse_error`.
6. Add tests for aliases and common bypasses:
   - `from subprocess import run; run(['ls'])`;
   - `import subprocess as sp; sp.run(['ls'])`;
   - `from pathlib import Path; Path('x').write_text('y')`;
   - `import os as o; o.remove('x')`;
   - `eval('1+1')`;
   - comments/strings containing `subprocess.run` should not alone trigger if AST scanner is active;
   - syntax error should produce a conservative risk assessment.

### Acceptance criteria

- AST scanner is primary.
- Fallback scanner is explicit and tested.
- Risk classification no longer relies only on substring matching.
- Denied capability checks use the improved risk assessment.

## Workstream D: Replace stringly command parsing with strict argv/shell-shape handling

### Problem

The command classifier and planner currently rely on `starts_with(...)`, simple character scanning, and `.split_whitespace()`. This is acceptable for metadata, but unsafe for real routing. Quoted arguments are mangled; redirection is not consistently classified as complex shell; broad prefixes such as `git branch` and `find` can be unsafe.

### Required changes

1. Add a `ShellShape` or equivalent parser result:
   - `Empty`;
   - `SimpleArgv(Vec<String>)`;
   - `ComplexShell { reasons: Vec<ShellComplexityReason> }`.
2. Use a shell-word parser or conservative internal parser for simple argv. If parser fails, classify as complex shell.
3. Mark the following as complex shell unless explicitly supported:
   - `;`, `&&`, `||`, `&`, `|`;
   - `<`, `>`, `>>`, `2>`, `2>&1`;
   - `$(`, `${`, backticks, `$VAR` expansion if it affects routing;
   - heredocs;
   - newlines;
   - shell grouping;
   - unbalanced quotes;
   - env assignment prefixes unless intentionally supported.
4. Change `CommandIntent` to carry parsed argv for simple commands, or add an internal classifier result that planner receives.
5. Stop using `.split_whitespace()` for executable managed argv routes.
6. Ensure tests validate real argv vectors, not only route labels.

### Acceptance criteria

- `rg 'fn main' src/` preserves the pattern as one argv argument.
- Commands with redirection do not route to read-only/native/test backends.
- `cargo test && rm -rf .` cannot route to test runner.
- `python -c 'print(1)'` can be recognized as Python source only through an explicit safe extraction path, not by blind whitespace splitting.
- Complex shell remains raw shell with existing BashTool security policy.

## Workstream E: Tighten git classification before native routing

### Problem

The current classifier treats all `git branch...` commands as read-only. This is incorrect; `git branch new`, `git branch -d foo`, and `git branch -D foo` mutate refs. Similar issues may exist around `git remote`, `git tag`, `git stash`, and `git show` edge cases.

### Required changes

1. Implement git classification from parsed argv, not string prefixes.
2. Treat only specific argv forms as read-only:
   - `git status [safe flags]`;
   - `git diff [safe flags/refs/paths]`;
   - `git log [safe flags/refs/paths]`;
   - `git show [safe refs]`;
   - `git branch --show-current`;
   - `git branch --list` and `git branch -l` if desired;
   - `git stash list`;
   - `git remote -v` and `git remote show` only if confirmed read-only;
   - `git tag --list` or `git tag -l` only.
3. Classify these as mutating/high-risk:
   - `git branch <name>`;
   - `git branch -d/-D/...`;
   - `git tag <name>` and tag deletion;
   - `git remote add/remove/set-url`;
   - `git stash`, except `git stash list`;
   - `git add`, `commit`, `restore`, `checkout`, `switch`, `reset`, `clean`, `push`, `pull`, `merge`, `rebase`, `cherry-pick`, `revert`.
4. Add tests for each read-only and mutating case.
5. Keep actual native git routing disabled until these tests pass.

### Acceptance criteria

- No git mutating command is classified as read-only.
- Read-only git commands remain low-friction.
- Native git route is only selected for explicitly read-only forms.

## Workstream F: Tighten search/read classification before managed-process routing

### Problem

Search/read classification currently broadly marks `find`, `cat`, `head`, `tail`, `which`, `whereis`, and `tree` as safe. Some of these can read outside the workspace, execute commands, or produce excessive output. BashTool’s blocklist tests even allow `find ... -exec rm ...`, which must never be treated as safe routing.

### Required changes

1. Classify search/read commands from argv.
2. Only route `rg`, `grep`, `fd`, `ls`, `pwd`, `cat`, `head`, `tail`, and simple `find` when all paths are workspace-contained or pathless.
3. For `find`, reject or keep raw shell if argv contains:
   - `-exec`;
   - `-delete`;
   - `-ok`;
   - redirection/shell syntax;
   - absolute outside-workspace paths.
4. For `cat`, `head`, `tail`, reject or require permission for outside-workspace paths and known sensitive paths.
5. Do not route `which`/`whereis` as file reads initially; keep raw shell/projected output if desired.
6. Add bounded-output expectations for search routes.
7. Keep search routing disabled until tests pass.

### Acceptance criteria

- `find . -name '*.rs'` can classify as read-only.
- `find . -exec rm {} \;` does not classify as safe read-only.
- `cat /etc/passwd` does not classify as safe file read.
- `rg 'fn main' src/` preserves argv and routes only when routing is enabled.

## Workstream G: Keep BashTool routing observe-only until safe execution routes exist

### Problem

BashTool currently computes metadata but still executes raw shell. That is safe from a rollout standpoint, but config naming such as `route_safe_commands` may imply active routing. If active routing is later added without the above fixes, unsafe commands could be misrouted.

### Required changes

1. Rename or document current BashTool behavior as observe/metadata mode unless actual routing is implemented.
2. Add an explicit active-routing mode distinct from metadata:
   - `mode = "observe" | "route_safe"` or similar;
   - default observe/off.
3. Ensure `route_safe_commands = true` alone does not silently enable active execution routing until code paths are implemented and tested.
4. When metadata is appended to model-visible bash output, ensure it does not bloat small command outputs excessively. Consider gating metadata to debug/TUI if noisy.
5. Add tests proving BashTool still runs raw shell when in observe mode.

### Acceptance criteria

- Current behavior is unambiguous: observe/metadata only.
- Active routing cannot be enabled accidentally by partial config.
- BashTool raw execution remains unchanged unless explicitly routed in a later pass.

## Workstream H: Projection and artifact tightening for Python

### Problem

Python runs currently return stdout/stderr strings and changed-file lists, but they do not yet provide durable raw output handles, script-body handles, or true diff handles as specified in the roadmap. Projection exists, but it is not yet fully integrated with the shared artifact/context projection system.

### Required changes

1. Connect Python run output to the existing artifact/output handle system where available.
2. Persist or expose:
   - script body hash;
   - script body handle;
   - stdout handle;
   - stderr handle;
   - changed file list;
   - diff handle for Transform mode.
3. Generate actual textual diff for Transform changed files where feasible.
4. Python projection should include compact model-facing summary plus handles for expansion.
5. RTK eligibility metadata should be attached for large stdout/stderr/diffs but should not require RTK.
6. Ensure raw script body is not dumped into model context by default.

### Acceptance criteria

- Python transform result includes changed files and a diff or explicit reason why diff generation was unavailable.
- Large stdout/stderr is bounded in model-facing projection.
- Raw artifacts remain inspectable through handles or clearly identified temporary artifacts.
- RTK absence does not fail Python runs.

## Workstream I: Tests and validation matrix

### Required unit tests

Add or update tests for:

- duplicate Python module removal/no references;
- Python Analyze write detection by pre/post snapshot;
- Python Verify write detection;
- Transform changed-file detection;
- denied network/subprocess/destructive/dependency capability pre-execution;
- env clearing behavior;
- AST scanner aliases and fallback behavior;
- argv parsing and quoted argument preservation;
- redirection/pipe/heredoc/command-substitution complex shell fallback;
- git read-only vs mutating classification;
- search/read safety classification;
- BashTool observe-only behavior;
- route metadata correctness without active routing.

### Suggested validation commands

Run targeted tests first:

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_intent::plan
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib python_script
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib test_runner::custom
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

If all targeted tests pass, run the capped full suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

If Python plugin examples are touched:

```bash
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v
```

## Handoff sequencing

Recommended implementation order:

1. Remove/consolidate duplicate `src/python_scripting.rs`.
2. Make Python mode enforcement authoritative with snapshots for Analyze/Verify and pre-execution denial for incompatible risk.
3. Add env clearing and cwd canonicalization to Python executor.
4. Replace command parsing with `ShellShape` and parsed argv.
5. Tighten git classifier.
6. Tighten search/read classifier.
7. Clarify BashTool observe-only routing mode.
8. Improve Python risk analysis toward AST scanning.
9. Add Python artifact/diff projection improvements.
10. Run targeted and capped validation.

## Done criteria for this tightening pass

This pass is complete when:

- There is one canonical Python scripting subsystem.
- Python Analyze/Verify cannot silently mutate workspace files.
- Denied Python capabilities stop execution before the script runs.
- Python execution uses canonical cwd validation and minimal environment.
- Command classification uses parsed argv or conservative complex-shell fallback.
- Git and search/read classifiers do not mark unsafe mutations as read-only.
- BashTool active routing remains disabled/observe-only unless explicitly and safely implemented.
- Tests cover the high-risk command/Python cases listed above.
- The repo is ready for a later active-routing implementation pass without relying on broad prefix heuristics.
