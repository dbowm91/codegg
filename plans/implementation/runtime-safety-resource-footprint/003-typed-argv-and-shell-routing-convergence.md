# Runtime Safety, Resource Control, and Footprint Milestone 003 — Typed Argv and Shell-Routing Convergence

Status: closed

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 003

Interface dependency:

- M002 — the canonical bounded process executable/argv and managed-process interface is implemented and accepted for this milestone.
- Corrective C001 — production helper trust-channel corrections are implemented; the remaining supported-Linux Landlock run is an operational strict-closure condition and does not block M003 implementation.

Promotion disposition:

- `plans/closure/runtime-safety-resource-footprint/009-m003-promotion-disposition.md`

Repository baseline reviewed: `0f3bf0b78e5de2dd03742f542d990590f0a32833`

Primary class: command correctness and authority simplification

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/003-status.md`

## 1. Objective

Preserve a typed executable and argument vector from command planning through native execution, remove whitespace-based reparsing, and simplify `BashTool` routing so policy, risk, and execution route are selected once.

The target execution routes are:

```text
Native argv route
  executable + Vec<OsString>
  no shell parsing or string reconstruction

Explicit shell route
  selected shell + one shell program string
  existing high-risk policy/approval applies

Specialized deterministic route
  structured operation translated directly to a typed executor/tool
```

A command must not begin as structured argv, collapse to a display string, and later be reconstructed using `split_whitespace()` or another lossy tokenizer.

## 2. Explicit non-goals

This milestone must not:

- implement a complete POSIX shell parser;
- infer safe native argv from arbitrary shell programs;
- remove raw shell support;
- expand command authority or weaken approval/risk rules;
- redesign the broader tool registry, scheduler, Tool Programs language, Git subsystem, or Bash translation architecture;
- add a second process executor beside M002;
- add command aliases or user-facing shell features unrelated to correctness;
- normalize every command into UTF-8 strings when the platform accepts `OsString`;
- add broad parser fuzzing or a shell compatibility matrix;
- change release/CI policy.

## 3. Current implementation evidence

Inspect at minimum:

- `src/tool/bash.rs`;
- command planning/translation modules invoked by `BashTool`;
- scheduler/job request types carrying commands;
- Git/tool adapters that translate structured operations to commands;
- M002 canonical process request types;
- tests for quoted arguments, shell fallback, risk classification, and command display;
- static execution ownership/translation guards.

The reviewed baseline contains a native execution path that derives argv with `split_whitespace()`. That behavior cannot preserve:

- quoted arguments such as `"two words"`;
- escaped spaces;
- empty arguments;
- literal quotes;
- backslashes with shell-specific meaning;
- paths containing spaces;
- arguments containing newlines or tabs;
- non-UTF-8 platform arguments.

The same module also contains substantial routing, regex risk filtering, native dispatch, raw shell dispatch, sandbox selection, and output rendering. Overlapping decisions increase the chance that one route bypasses canonical execution or applies a different policy.

The implementation agent must trace the real entry points and identify where typed information is first available. Do not attempt to recover argv after it has already been flattened; move the typed boundary earlier.

## 4. Invariants that cannot regress

- approval and risk decisions remain at least as strict as the reviewed baseline;
- raw shell remains explicit and is never selected merely because native parsing failed;
- native execution never interprets shell metacharacters;
- typed argv reaches M002 without string round-tripping;
- cwd, environment, sandbox, timeout, cancellation, and output policies remain those selected by the owning request;
- command display/redaction is separate from the executable argv used for execution;
- structured Git/build/test operations continue using their typed ownership boundaries;
- unsupported or ambiguous command forms return a clear routing error or require explicit shell mode;
- no caller gains authority by selecting a different representation of the same command.

## 5. Required command model

Introduce or normalize a representation equivalent to:

```rust
enum CommandProgram {
    Native {
        executable: OsString,
        argv: Vec<OsString>,
    },
    Shell {
        shell: ShellKind,
        program: String,
    },
    Specialized {
        operation: StructuredOperation,
    },
}

struct PlannedCommand {
    program: CommandProgram,
    cwd: PathBuf,
    env: EnvironmentPolicy,
    risk: CommandRisk,
    approval: ApprovalRequirement,
    sandbox: SandboxRequest,
    provenance: ExecutionProvenance,
}
```

Exact types may integrate with existing planner/request structures. The key requirements are:

- execution form is an enum, not an implicit string convention;
- risk/approval/sandbox decisions attach to the plan and are not recomputed differently in each executor;
- display rendering is derived and redacted, not reused as executable input;
- native argv uses `OsString` or an equivalent lossless platform representation.

## 6. Routing rules

### 6.1 Native route

Use native route only when the caller already supplies a typed executable/argv or a deterministic structured translator produces one.

Native route must:

- pass executable separately from argv;
- preserve empty and whitespace-containing arguments;
- avoid shell expansion, globbing, substitution, pipelines, redirects, and environment assignment syntax;
- reject interior NUL at the platform boundary with a typed validation error;
- render a safe diagnostic/display form without changing execution bytes.

### 6.2 Explicit shell route

Use shell route only when:

- the user/tool explicitly requested shell semantics;
- a plan contains shell operators that cannot be represented as one native process;
- the existing policy permits the route and required approval is satisfied.

The shell executable and invocation flags must be selected by existing platform/config policy. The program string is passed once to the shell through M002.

Do not parse an arbitrary shell program merely to classify it as native. A small conservative detector may reject obvious shell constructs from a native-only API, but uncertainty must not trigger a silent shell fallback.

### 6.3 Specialized route

Existing structured tools and translators should bypass shell strings. Examples may include:

- Git operations through `codegg-git`/`egggit` ownership;
- build/test operations represented as typed jobs;
- file operations represented by deterministic tools.

This milestone does not need to create new specialized tools. It should preserve or use current ones and remove string reconstruction where a typed operation already exists.

## 7. Risk and approval convergence

Identify the authoritative risk/approval decision point in `BashTool` or adjacent planning code.

Refactor toward:

1. parse or receive the requested representation;
2. determine route;
3. determine risk and approval requirement once;
4. produce `PlannedCommand`;
5. execute through M002 using the plan;
6. render result.

Avoid:

- one regex denylist in the router and another independent decision in the executor;
- recomputing risk from a redacted display string;
- changing route after approval;
- treating parse failure as permission to use raw shell;
- allowing specialized/native translators to bypass existing policy.

Keep emergency deny rules only for clearly defined cases that cannot be represented in typed policy yet. Each retained regex should have an owner, rationale, and focused test. Do not expand the denylist as a substitute for typed authority.

## 8. Expected production-code changes

Expected areas:

- `src/tool/bash.rs` split into clearer planning/routing and execution-adapter responsibilities where practical;
- command plan/request types;
- M002 adapter invocation;
- structured translators that currently emit strings and then reparse them;
- scheduler/job command serialization if it currently flattens argv;
- diagnostics/redaction helpers;
- focused tests and static guards;
- architecture/tool documentation.

Avoid broad module moves solely for aesthetics. A smaller model should prefer extracting only the minimum types/functions needed to make ownership clear.

## 9. Storage, protocol, migration, and compatibility effects

Storage:

- no database migration expected;
- durable jobs that currently store one command string may require a backward-compatible representation containing route plus argv;
- if historical queued jobs exist, retain a legacy decode path that maps old records to explicit shell or a conservative compatibility route. Do not reinterpret an old shell string as native argv with whitespace splitting.

Protocol:

- frontend/daemon request APIs may gain optional structured argv fields while retaining existing shell-string fields;
- conflicting fields must be rejected rather than resolved by precedence;
- public output text remains compatible.

Compatibility:

- existing explicit shell commands continue to work;
- native/structured callers gain correct quoting/empty-argument behavior;
- configuration that specifies a shell remains honored;
- old stored shell programs remain shell programs during migration;
- no documented command should change from shell semantics to native semantics without tests and release notes.

## 10. Ordered work packages

### Work package A — Representation inventory

1. find every command request and durable job representation;
2. identify where argv is available and where it is flattened;
3. classify callers as native, explicit shell, or specialized;
4. identify legacy persisted strings requiring compatibility decode.

### Work package B — Typed command plan

1. introduce/normalize the route enum and typed native argv;
2. attach cwd/env/risk/approval/sandbox/provenance once;
3. separate executable representation from display/redaction;
4. add validation for conflicting or malformed representations.

### Work package C — Native execution migration

1. remove `split_whitespace()` and equivalent reparsing;
2. pass typed argv directly to M002;
3. update deterministic translators to emit argv vectors;
4. preserve platform `OsString` handling;
5. add focused diagnostics for invalid executable/argument values.

### Work package D — Explicit shell path

1. make shell selection explicit in the plan;
2. route through M002 with the existing approval and sandbox policy;
3. reject silent fallback from native to shell;
4. preserve current shell configuration and output behavior.

### Work package E — Risk/policy cleanup

1. identify the single authoritative risk/approval decision;
2. remove duplicated or post-approval route decisions;
3. keep only justified emergency regex rules;
4. test that representation changes cannot lower risk.

### Work package F — Compatibility and documentation

1. implement legacy durable command decode if required;
2. update tool/job protocol docs;
3. document native versus shell semantics with examples;
4. update static ownership/translation guard configuration.

## 11. Focused verification

Required focused cases:

- executable path containing spaces;
- one argument containing spaces;
- empty argument;
- literal quote/backslash argument;
- newline/tab inside an argument where supported;
- argument beginning with `-` preserved after `--` decisions owned by the target, not CodeGG;
- native argument containing `*`, `$`, `;`, `|`, `>`, or parentheses remains literal and is not shell-expanded;
- explicit shell pipeline/redirection still executes through shell route;
- ambiguous/native parse failure does not silently select shell;
- risk/approval result is stable before and after rendering;
- legacy stored shell command remains shell-routed;
- cancellation/output/sandbox behavior still comes from M002.

Expected command shape:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test <bash/native routing target> -- --test-threads=1
cargo test <job command serialization target> -- --test-threads=1
cargo test <git or structured translator target> -- --test-threads=1
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
```

Use existing/current target names and do not duplicate broad suites.

## 12. Static guards

Extend an existing cheap source/ownership guard to reject in governed production paths:

- `.split_whitespace()` used to build process argv;
- `shell_words`-style reconstruction unless an explicit compatibility parser is approved and tested;
- command display strings passed to native execution;
- direct shell fallback after a native-route error.

The guard should allow ordinary text parsing unrelated to process argv. Scope it by module/path and a small explicit allow-list.

## 13. Acceptance criteria

M003 is complete only when:

- native commands carry executable plus typed argv end to end;
- no governed native path uses `split_whitespace()` or equivalent lossy reparsing;
- explicit shell route is represented distinctly;
- native errors do not silently fall back to shell;
- risk, approval, sandbox, cwd, env, timeout, cancellation, and provenance are selected once and retained through execution;
- display/redaction cannot alter executable input;
- structured translators emit typed operations/argv where already feasible;
- old persisted shell strings remain shell semantics through compatibility decode;
- quoted, empty, special-character, and space-containing argument tests pass;
- the static negative fixture is detected;
- `scripts/verify.sh quick` passes;
- the existing hosted verification is referenced when the normal trigger produces a run, but unavailable dispatch is not an implementation blocker;
- no tool authority expansion, scheduler redesign, or new shell parser framework is introduced.

## 14. Stop conditions

Stop and report blocked when:

- the accepted M002 executable/argv or managed-process interface is absent, materially changed, or found defective;
- the durable job schema cannot represent typed argv without a breaking migration;
- existing public APIs ambiguously overload one field for both argv and shell programs and require a compatibility decision;
- a translator's ownership belongs to another subsystem and cannot be changed without reopening its public contract;
- implementation would require parsing arbitrary shell syntax to preserve behavior.

The outstanding supported-Linux C001 evidence condition is not a stop condition for this milestone. Prefer one compatibility adapter or narrow prerequisite plan over broad shell-parser work.

## 15. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/003-status.md` must include:

- accepted commit/PR;
- command representation before/after summary;
- inventory of native, shell, specialized, and legacy routes;
- focused argv fidelity results;
- explicit-shell and no-silent-fallback results;
- risk/approval stability evidence;
- durable compatibility disposition;
- static guard negative proof;
- focused commands and quick verification;
- hosted run reference when the normal trigger produces one, with absence classified operational rather than implementation-blocking;
- unresolved findings by severity.
