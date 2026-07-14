# Git Agent Integration Corrective Security Closure Pass

## Status

Planned corrective closure pass for the completed Git Agent Integration roadmap (Phases A-F).

This pass is intentionally narrow. The typed Git model, unified routing, structured reads, mutation executor, network/destructive operations, conflict recovery, projections, TUI integration, and RunStore provenance are already implemented. The remaining work is to close the two security findings recorded by the Phase F security review and to prove that the corrected invariants hold across typed, managed-argv, Bash-translated, and raw compatibility paths.

## Triggering Findings

The Phase F security review recorded two implementation-level findings:

1. `remote_set_url` can carry a credential-bearing URL through a path that does not consistently apply `redact_url_credentials()` before the value reaches logs, projections, errors, or RunStore persistence.
2. The raw/compatibility Git fallback in `src/tool/git.rs` does not use the same hardened process environment as typed Git network and mutation execution, leaving a weaker execution path for interactive prompts, editor invocation, credential/config inheritance, and environment exposure.

These are closure blockers because they violate two roadmap-wide invariants:

- Secrets and credential-bearing remote URLs must never enter durable or model-visible output unredacted.
- Falling back from a typed Git operation must not weaken the process-security boundary.

## Goals

This pass must:

1. Eliminate all credential-bearing URL leakage paths for remote add/set-url/list/show/error/projection/persistence operations.
2. Consolidate Git child-process environment construction so typed, managed Git argv, and raw Git compatibility paths share a documented minimum hardening baseline.
3. Preserve required user functionality for SSH agents, credential helpers, HOME/XDG configuration, locale, and executable discovery without restoring arbitrary ambient environment state.
4. Add focused unit, integration, adversarial, projection, and RunStore tests that fail on the pre-fix behavior.
5. Update the Phase F security review and handoff documentation from “open finding” to “resolved with evidence.”
6. Re-run the focused Git suites and capped workspace validation without broad unrelated refactors.

## Non-Goals

This pass must not:

- Redesign the typed `GitOperation` vocabulary.
- Expand worktree, submodule, bisect, apply-mailbox, or sequencer capabilities.
- Change default permission policy for network or destructive operations except where required to preserve an existing security invariant.
- Replace Git CLI execution with libgit2 or another implementation.
- Perform the proposed large file decomposition of `parser.rs`, `render.rs`, `git_service.rs`, or `tool/git.rs`.
- Add global Git configuration mutation support.
- Introduce a generic secret-management subsystem outside the Git execution boundary.

## Required Invariants

The implementation is complete only when all of the following are true.

### Secret handling

- A remote URL containing `scheme://user:password@host/path`, `scheme://token@host/path`, or equivalent credential syntax is redacted before it is logged, projected, returned in errors, stored in RunStore metadata, or retained in typed operation details.
- The raw secret may be passed only to the child Git process when necessary to execute the requested operation.
- Redaction is idempotent and does not corrupt anonymous HTTPS URLs, SSH URLs, SCP-like remotes, local paths, file URLs, IPv6 hosts, ports, query strings, or fragments.
- Redaction applies to both success and failure paths.
- Remote listings and state snapshots never re-expose credentials read back from `.git/config`.
- Debug formatting of request/result types must not expose raw credential-bearing URLs.

### Process environment

- Every Codegg-owned Git child process starts from an explicitly constructed environment rather than unconstrained ambient inheritance.
- `GIT_TERMINAL_PROMPT=0` is enforced for all noninteractive agent execution.
- Commit, merge, tag, rebase, and sequencer paths cannot launch an editor or pager unexpectedly.
- `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`, and pager behavior are pinned where applicable.
- The environment policy prevents unreviewed injection through `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_*`, `GIT_CONFIG_VALUE_*`, `GIT_CONFIG_PARAMETERS`, `GIT_SSH_COMMAND`, `GIT_ASKPASS`, `SSH_ASKPASS`, `GIT_PROXY_COMMAND`, and similar command-bearing variables.
- Required variables such as `PATH`, platform-appropriate HOME/config roots, locale, and SSH agent socket are included only by explicit policy.
- Typed and fallback execution paths use the same baseline policy; network paths may extend that baseline only through a named, reviewed policy layer.

### Routing and provenance

- Bash-translated simple Git commands and native Git tool calls receive equivalent redaction and environment guarantees.
- Managed Git argv fallback remains distinguishable in RunStore provenance but is not less hardened than typed execution.
- Raw shell commands containing Git remain under Bash/shell policy; Codegg must not claim Git-specific guarantees where the entire shell expression is intentionally executed by the shell.
- Any compatibility path that bypasses the unified Git service must be either removed, explicitly justified, or covered by an equivalent security wrapper.

## Workstream A — Trace and Classify All Remote URL Flows

Perform a code-level data-flow inventory before modifying behavior.

Inspect at minimum:

- `src/git_network_ops.rs`
- `src/git_network_policy.rs`
- `src/git_service.rs`
- `src/git_mutations.rs`
- `src/git_run_store.rs`
- `src/git_mutation_projector.rs`
- `src/tool/git.rs`
- `src/tool/commit.rs`
- `src/tool/bash.rs`
- `src/command_outcome.rs`
- `crates/codegg-git/src/operation.rs`
- `crates/codegg-git/src/render.rs`
- remote/ref/status primitives in `crates/egggit/`

For each operation that accepts or emits a URL, document:

- where the raw value enters;
- whether it is needed by the child process;
- where a sanitized copy is created;
- what is persisted in `GitExecutionRequest`, `MutationResult`, state snapshots, operation detail, and RunStore artifacts;
- what is included in tracing fields, error strings, projections, and tool output;
- whether `Debug`, `Display`, serialization, or deterministic argv rendering exposes the raw value.

The inventory must cover at least:

- remote add;
- remote set-url;
- remote get-url/list;
- remote rename/remove;
- fetch, pull, and push failure output;
- `git config --get remote.<name>.url`;
- structured ref/remote listing;
- managed Git argv fallback;
- native raw-subcommand fallback;
- Bash-translated Git commands.

Deliverable: a concise table added to `docs/validation/git-security-review.md` or a dedicated appendix showing each source, sink, and redaction boundary.

## Workstream B — Establish a Single Redaction Boundary

### B1. Introduce a secret-safe URL representation

Avoid passing a single untyped `String` through both execution and persistence layers. Introduce a small type or paired representation, for example:

```rust
pub struct SensitiveRemoteUrl {
    raw: secrecy::SecretString, // or a local non-Debug wrapper
    redacted: String,
}
```

A new dependency is not required if a local wrapper can provide the same guarantees. The critical requirements are:

- raw value is not exposed by `Debug`;
- raw value is available only at the final child-process argument construction boundary;
- redacted value is used by projections, tracing, errors, persistence, and typed operation metadata;
- cloning does not accidentally duplicate the raw secret across long-lived state;
- serialization of the wrapper is either forbidden or serializes only the redacted form.

If introducing a wrapper would cause excessive churn, implement an equivalent strict boundary using separate `execution_url` and `display_url` variables with tests that prove the raw value never crosses into durable structures. Document the tradeoff.

### B2. Fix `remote_set_url`

Correct the known gap in `src/git_network_ops.rs` so:

- Git receives the raw requested URL;
- the resulting typed operation detail contains only the redacted URL;
- success output and state deltas contain only redacted values;
- failure messages sanitize both argv-derived and Git-emitted URL text;
- RunStore persistence receives only redacted detail;
- tracing fields never use the raw URL.

Do not “fix” the leak by giving Git only a redacted URL; execution must remain correct.

### B3. Centralize output sanitization

Git itself may echo a URL in stderr or stdout. Add a final defense-in-depth sanitizer before Git output reaches:

- tool return strings;
- projector input;
- error conversion;
- RunStore artifact/detail persistence;
- tracing events.

This sanitizer should recognize credential-bearing URLs in arbitrary surrounding text, not only exact request values. At minimum cover HTTP(S), FTP-like schemes if supported by Git, and URLs embedded in standard Git failure messages.

Prefer sanitizing using the known request URL first, then a conservative URL credential pattern for Git-emitted text. Avoid broad substitutions that damage ordinary `user@host` SSH output.

### B4. Audit deterministic rendering and debug output

`codegg-git::render_argv()` is useful for execution and provenance, but credential-bearing arguments must not be used unchanged for human-visible or durable rendering.

Define two explicit render paths if needed:

- execution argv: exact raw values;
- display/provenance argv: redacted values.

Tests must prevent callers from accidentally using the execution renderer for logs or persistence.

## Workstream C — Unify Git Process Environment Hardening

### C1. Inventory current process builders

Locate every `Command::new("git")` or equivalent Git child creation site across the workspace. Classify each as:

- read-only `egggit` primitive;
- typed local mutation;
- typed network operation;
- managed Git argv fallback;
- native raw-subcommand fallback;
- commit/review helper;
- TUI/sidebar probe;
- tests or scripts.

The production inventory should result in a single list of allowed builders. Duplicate ad hoc environment setup should be removed or delegated.

### C2. Introduce a shared baseline policy

Create a shared builder or policy type, located where it can be used without creating circular crate dependencies. A likely shape is:

```rust
pub struct GitProcessPolicy {
    pub interaction: InteractionPolicy,
    pub network: NetworkPolicy,
    pub config_access: ConfigAccessPolicy,
}

impl GitProcessPolicy {
    pub fn command(&self, repo: &Path, argv: &[OsString]) -> Command;
}
```

The baseline must:

- call `env_clear()`;
- restore a reviewed executable-search path;
- restore locale variables needed for stable output or set `LC_ALL=C` where parsers require it;
- enforce `GIT_TERMINAL_PROMPT=0`;
- enforce `GIT_PAGER=cat` and `PAGER=cat`;
- disable editor invocation with `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` for agent-owned operations;
- remove command-bearing Git and askpass environment variables;
- use `kill_on_drop(true)`;
- apply timeout handling consistently;
- set the repository working directory explicitly.

### C3. Define controlled HOME/XDG and credential behavior

A completely empty environment can break legitimate Git use. Define and test an explicit policy for:

- `HOME`;
- `XDG_CONFIG_HOME`;
- `XDG_CACHE_HOME` if needed;
- `SSH_AUTH_SOCK`;
- `SSH_AGENT_PID` if required;
- Windows-specific profile/config variables;
- system certificate variables required by HTTPS transports.

The policy should preserve normal user authentication without allowing command injection through Git config or environment.

At minimum:

- allow SSH agent socket passthrough by explicit opt-in field in the network policy;
- allow credential helpers only through normal Git config resolution if the project’s existing behavior depends on them;
- reject or strip environment variables that directly specify helper commands or askpass programs;
- document that network operations remain noninteractive and fail rather than prompt.

### C4. Apply the baseline everywhere

Migrate all Codegg-owned production Git child creation to the shared policy. In particular, close the known raw fallback gap in `src/tool/git.rs`.

Typed network execution may extend the baseline with network-specific variables, but it must not replace the baseline with a separate weaker implementation.

Read-only `egggit` operations may use a read-only profile, but the same anti-interaction and anti-command-injection rules should apply.

### C5. Preserve shell boundary honesty

Do not silently rewrite complex raw shell expressions into direct Git execution. For commands that remain shell-owned:

- retain existing Bash sandbox and permission behavior;
- attach Git-aware classification where currently supported;
- do not represent them as `ActualBackend::Git` unless the Git service executed them;
- document that shell-owned commands inherit the shell execution policy, not the direct Git process policy.

## Workstream D — Error, Projection, and Persistence Hardening

### D1. Error types

Review every conversion into `ToolError`, `GitMutationError`, network failure structures, and anyhow/error strings. Ensure errors carry:

- operation kind;
- remote name where safe;
- classified failure kind;
- exit code and timeout state;
- redacted stderr/stdout only.

Raw argv containing secrets must not be included in `Display` implementations.

### D2. Projection

Add regression coverage to `src/git_mutation_projector.rs` and relevant shell projectors proving:

- credential-bearing URLs never appear in projected success output;
- credential-bearing URLs never appear in projected failure output;
- redaction markers are stable and understandable;
- ordinary SSH remotes such as `git@github.com:owner/repo.git` are not incorrectly redacted;
- remote names and non-secret host/path information remain visible.

Add or update golden fixtures for:

- `remote_set_url` success with credentials;
- fetch authentication failure echoing a URL;
- push rejection with a credential-bearing URL;
- managed fallback output containing a URL.

### D3. RunStore

Create integration tests using the real in-memory and filesystem RunStore implementations. Execute representative operations with sentinel credentials such as `CODEGG_TEST_SECRET_...`, then recursively inspect:

- run manifest;
- backend detail;
- artifacts;
- index entries;
- serialized request/result metadata;
- persisted stdout/stderr.

The sentinel must not appear anywhere in the RunStore directory or returned records.

This should be implemented as a reusable secret-scanning assertion so future Git operations can opt into the same test.

### D4. Tracing and logs

Use a test tracing subscriber or capture layer where practical. Assert the sentinel credential does not appear in emitted events for success and failure paths.

Where tracing capture is impractical, ensure all tracing statements use redacted structured fields and add unit tests around the helpers feeding those fields.

## Workstream E — Adversarial and Cross-Path Tests

Add focused tests for the same logical operation through all supported origins:

1. Native typed Git tool call.
2. Bash-translated simple Git command routed to the Git backend.
3. Managed Git argv fallback.
4. Native raw-subcommand compatibility path.
5. Shell-owned complex Git expression, where only shell guarantees apply.

### Credential cases

Cover:

- HTTPS username/password;
- HTTPS token-only userinfo;
- percent-encoded credentials;
- IPv6 host and port;
- query and fragment components;
- SCP-like SSH remote;
- `ssh://user@host/path` without a password;
- local path remote;
- file URL;
- malformed URL passed to Git;
- Git stderr echoing the exact raw URL;
- Git stderr echoing a normalized URL.

### Environment attack cases

Before execution, populate the parent process with sentinel variables:

- `GIT_ASKPASS` pointing to a marker script;
- `SSH_ASKPASS`;
- `GIT_SSH_COMMAND`;
- `GIT_PROXY_COMMAND`;
- `GIT_EDITOR`;
- `GIT_SEQUENCE_EDITOR`;
- `GIT_CONFIG_COUNT` and matching key/value variables;
- `GIT_CONFIG_PARAMETERS`;
- `GIT_PAGER` and `PAGER`;
- hostile `GIT_DIR` and `GIT_WORK_TREE`.

The child process must not observe or execute these values unless a particular variable is explicitly permitted by the reviewed policy.

Use marker files to prove helper/editor commands were not invoked.

### Noninteractive behavior

Tests should verify:

- authentication failure returns promptly;
- no editor opens during commit/amend/rebase/merge paths;
- no pager stalls large log/diff output;
- timeouts kill child processes;
- cancellation does not leave a child process running.

### Platform coverage

At minimum, keep Linux tests mandatory and make environment/path tests portable across macOS and Windows where the project supports them. Avoid Unix-only assumptions in the shared policy API. Platform-specific exclusions must be documented in `docs/validation/git-cross-platform.md`.

## Workstream F — Policy and Documentation Reconciliation

Update:

- `docs/validation/git-security-review.md`
- `architecture/git.md`
- `architecture/git_phase_f_handoff.md`
- `AGENTS.md` if Git execution gotchas change
- the roadmap completion note if it currently implies unresolved security findings

Documentation must state:

- the exact redaction boundary;
- which structures may contain a raw URL transiently;
- the shared Git environment baseline;
- which ambient variables are retained and why;
- the distinction between direct Git execution and shell-owned commands;
- focused test commands and evidence;
- resolved finding identifiers and commit SHA placeholders to fill during implementation.

Do not erase the historical findings. Mark them resolved, explain the root cause, and reference the regression tests.

## Suggested Implementation Order

1. Inventory URL and process-builder flows.
2. Add failing regression tests for `remote_set_url` persistence/output leakage.
3. Introduce the secret-safe URL/redacted display boundary.
4. Fix `remote_set_url` and sanitize network output/error paths.
5. Add failing environment-injection tests for the raw fallback.
6. Introduce the shared Git process policy.
7. Migrate typed, managed, and raw direct-Git execution to the policy.
8. Add RunStore, tracing, projector, Bash-translation, and adversarial tests.
9. Run focused validation and repair any regressions.
10. Update security review, architecture, handoff, and roadmap documentation.

## Detailed Acceptance Criteria

### Credential closure

- [ ] `remote_set_url` passes the raw URL only to Git execution.
- [ ] Persisted operation details contain a redacted URL.
- [ ] stdout/stderr are sanitized before persistence and projection.
- [ ] error messages never contain the sentinel secret.
- [ ] tracing never contains the sentinel secret.
- [ ] structured remote listing never returns credentials.
- [ ] execution and display argv rendering are clearly separated.
- [ ] redaction tests cover success, failure, normalization, and malformed inputs.

### Environment closure

- [ ] All production `git` subprocess sites are inventoried.
- [ ] All direct Git subprocess sites use the shared policy or have an explicit documented exception.
- [ ] Raw/native fallback uses the same baseline as typed Git execution.
- [ ] command-bearing Git environment variables are stripped.
- [ ] interactive prompts, editors, and pagers are disabled.
- [ ] SSH-agent and required config/certificate behavior remains functional.
- [ ] timeouts and process cancellation remain consistent.

### Cross-path closure

- [ ] Native typed tool path is covered.
- [ ] Bash-translated route is covered.
- [ ] Managed Git argv fallback is covered.
- [ ] Native raw-subcommand compatibility path is covered.
- [ ] Shell-owned complex commands remain correctly classified and documented.
- [ ] RunStore planned/actual backend and ownership remain accurate.

### Validation closure

- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo check --workspace --all-features` passes.
- [ ] focused clippy on `codegg`, `codegg-git`, `egggit`, and `codegg-core` passes with no new warnings.
- [ ] `cargo test -p codegg-git` passes.
- [ ] `cargo test -p egggit` passes.
- [ ] focused Git tool, mutation, network, recovery, projector, routing, and RunStore suites pass.
- [ ] new credential sentinel scan tests pass for memory and filesystem stores.
- [ ] capped full workspace suite passes using the repository’s current documented thread limit.
- [ ] CI result or equivalent local evidence is recorded.

## Recommended Focused Test Commands

Adapt exact test names to implementation, but retain a reproducible validation block similar to:

```bash
cargo fmt --all --check
cargo check --workspace --all-features
cargo clippy -p codegg-git --all-targets --all-features
cargo clippy -p egggit --all-targets --all-features
cargo clippy -p codegg-core --all-targets --all-features
cargo clippy -p codegg --lib --tests --all-features

cargo test -p codegg-git
cargo test -p egggit
cargo test -p codegg-core run_store
cargo test -p codegg --lib git_network_policy
cargo test -p codegg --lib git_network_ops
cargo test -p codegg --lib git_mutation_projector
cargo test -p codegg --lib tool::git
cargo test -p codegg --lib tool::bash
cargo test --test git_network_integration
cargo test --test git_mutations_integration
cargo test --test git_recovery_integration
cargo test --test git_closure_matrix

CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=8
```

## Review Checklist

The reviewer should explicitly inspect:

- whether raw URLs live beyond the child-process call boundary;
- whether any `Debug` or serialization path exposes secret fields;
- whether errors sanitize Git-emitted output;
- whether `render_argv()` is used for persistence with raw values;
- whether every direct Git child uses the shared environment policy;
- whether the shared policy accidentally disables normal SSH-agent or HTTPS certificate behavior;
- whether Bash translation and native calls converge on the same protections;
- whether shell-owned commands are accurately represented in provenance;
- whether test sentinel values are searched through all durable artifacts;
- whether documentation records the findings as resolved rather than silently deleting them.

## Definition of Done

This corrective pass is complete when the two Phase F security findings are fixed in code, protected by regression tests across every relevant execution origin, validated against both in-memory and filesystem persistence, and marked resolved in the security and handoff documentation.

No credential-bearing remote URL may be observable outside the transient Git child argument boundary, and no direct Git fallback path may execute under a weaker environment policy than the typed Git subsystem.
