# Runtime Safety, Resource Control, and Footprint Corrective C001 — Sandbox Helper Trust Channel and Roadmap Unblock

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Corrects:

- M001 — `plans/implementation/runtime-safety-resource-footprint/001-landlock-and-sandbox-contract-correction.md`
- M002 — `plans/implementation/runtime-safety-resource-footprint/002-canonical-bounded-process-execution.md`
- M001 closure record — `plans/closure/runtime-safety-resource-footprint/001-status.md`
- M002 closure record — `plans/closure/runtime-safety-resource-footprint/002-status.md`

Unblocks:

- M003 — typed argv and shell-routing convergence;
- M007 — binary topology and footprint reduction, after M003 closes;
- M008 — final planning and maintenance closure, after M007 closes.

Repository baseline reviewed: `719a670fdb12b74ca29fb3f28cb04f97382325d4`

Implementation commit: `013f157639b82d16a38aca2764c819b3d63bd355`

Pull request context: PR #72, branch `planning/runtime-safety-resource-footprint`

Primary class: security invariant and dependency-unblock corrective pass

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/009-status.md`

Independent security review: required for helper discovery, control-channel isolation, and fail-closed behavior.

## 1. Objective

Correct the remaining trust and signaling defects at the private sandbox-helper boundary, obtain one supported-Linux enforcement result, and revise milestone promotion so M003 is blocked only by substantive sandbox correctness rather than duplicated hosted-evidence bookkeeping.

The accepted end state is:

```text
CodeGG parent
  |
  |-- resolves a trusted sibling helper through production-owned logic
  |-- sends one bounded private launch specification outside the target cwd
  |-- creates one private setup-status channel
  v
sandbox helper
  |
  |-- validates the bounded specification
  |-- applies the complete Landlock policy
  |-- proves full enforcement and no_new_privs
  |-- reports setup state only through the private channel
  |-- prevents the target from inheriting the status writer
  v
target process

Target stdout/stderr are ordinary target output and cannot alter sandbox state.
```

When C001 closes, M001 and M002 may retain their existing historical closure records with a linked corrective disposition, and M003 must be promoted to `ready`. M005 and M006 operational hosted-evidence conditions must not independently block M007 once their production dispositions and the final combined revision are accepted.

## 2. Discovered defects and why the original verification missed them

### 2.1 Production helper substitution through inherited environment

Current helper discovery accepts `CODEGG_SANDBOX_HELPER` from the inherited process environment. A substituted executable can imitate the expected marker and launch the target without applying Landlock.

This is a trust-boundary defect because sandbox enforcement depends on the identity of the helper, not merely its output format.

Original verification exercised helper success and failure behavior but did not include an adversarial production-resolution test proving that an inherited environment variable cannot replace the helper.

### 2.2 Sandbox state shares target stderr

Current parent logic scans collected stderr for reserved strings such as an enforced ABI marker and a setup-error marker. A target can emit the same strings. This can forge enforcement metadata, hide or strip ordinary target output, or produce a false sandbox failure.

Original tests validated marker parsing but did not treat the target as an adversarial writer to the same stream. The transport and the user-output channel were incorrectly assumed to be equivalent.

### 2.3 Helper specification is created inside the target cwd

The canonical executor currently creates the helper specification with a temporary file rooted in the requested working directory. A valid read-only workspace or cwd can therefore prevent sandbox launch before Landlock is attempted.

Original tests used writable temporary workspaces and did not exercise an executable target with a read-only cwd.

### 2.4 Supported-Linux runtime evidence remains absent

M001 and M002 were implemented and reviewed on Darwin. The Linux fixture exists, but no accepted run records a supported kernel, effective Landlock ABI, read-only denial, allowed read, workspace write behavior, outside-root denial, and required-mode failure behavior.

Original verification was intentionally host-limited. This corrective pass does not add a platform matrix; it requires one bounded run on one supported Linux host.

### 2.5 Planning dependencies conflate correctness and operations

M003 is blocked on strict M001/M002 closure, including hosted evidence that does not affect the stability of the executable/argv interface. M007 is also described as blocked by separate strict M005 and M006 hosted outcomes even though those milestones report completed production work and their remaining conditions are runner-cache or trigger availability.

This happened because closure status was used as a single undifferentiated dependency class. The planning process already distinguishes hard, interface, soft, and operational dependencies; the registry must apply those categories.

## 3. Explicit non-goals

C001 must not:

- redesign Landlock policy semantics already implemented by M001;
- add seccomp, namespaces, containers, cgroups, virtual machines, or remote execution;
- create a public helper protocol, daemon endpoint, or persistent service;
- implement M003 typed argv or Bash routing work;
- redesign `ManagedProcessService` outside the helper launch/status boundary;
- implement Windows process-tree or sandbox support;
- add a CI matrix, new hosted workflow, scheduled workflow, release gate, artifact bundle, or benchmark suite;
- require a separate hosted run for every conditionally closed predecessor;
- split PR #72 or perform the M007 binary-topology decision;
- weaken explicit sandbox failure behavior or introduce a best-effort bypass;
- treat local installation tampering by an actor who can replace the CodeGG binaries as a threat solved by this helper protocol.

## 4. Invariants that cannot regress

- An explicit sandbox request fails closed when the trusted helper cannot be resolved, its specification cannot be transported, setup status is missing or malformed, Landlock cannot be fully enforced, or target exec fails.
- Production helper identity is not selected by an inherited environment variable, PATH lookup, target-controlled cwd, or target-provided argument.
- The target cannot forge, suppress, or modify the parent-observed sandbox setup outcome through stdout or stderr.
- The helper status writer is unavailable to the target after successful `exec`.
- Target stdout and stderr are preserved as target output except for existing bounded-output behavior; no reserved marker text is stripped.
- The private specification and control channel are bounded and are not durable daemon or network protocols.
- The target cwd may be read-only. Temporary helper plumbing must not require write access there.
- The daemon process remains unsandboxed; Landlock applies only in the one-shot helper child.
- M002 retains timeout, cancellation, bounded output, process-group cleanup, environment, stdin, and provenance ownership.
- No milestone is promoted merely because a document changed. C001 closure evidence must exist first.
- Operational runner/cache conditions do not masquerade as production correctness dependencies.

## 5. Required trust and transport contract

### 5.1 Trusted helper discovery

Production helper resolution must use one installation-owned rule:

1. derive the expected helper location from the running CodeGG executable or an immutable installation layout;
2. canonicalize the helper and expected installation parent;
3. require a regular executable file at that location;
4. reject missing, ambiguous, PATH-resolved, cwd-relative, or inherited-environment-selected helpers;
5. fail the sandbox request before target execution when resolution fails.

The exact packaging layout may remain the existing sibling-binary layout. The threat model may assume that an actor able to replace files in the CodeGG installation can replace CodeGG itself; C001 does not add binary signing.

Tests must not retain a production environment-variable override. Use dependency injection, an internal test-only resolver, or a test constructor compiled only for tests. Do not add a user-facing configuration setting that selects an arbitrary helper.

### 5.2 Private status channel

Replace stderr marker parsing with a dedicated parent/helper control channel.

The preferred Unix implementation is an anonymous pipe or socket pair with these properties:

- the parent owns the read endpoint;
- the helper alone receives the write endpoint;
- the status payload is length-bounded, versioned locally, and contains only typed setup state;
- after full Landlock enforcement, the helper reports an enforced outcome and prepares the descriptor to close on successful target exec;
- if target exec fails, the helper reports a typed exec failure and exits through the reserved helper failure path;
- successful target exec causes control-channel EOF without the target retaining the writer;
- missing outcome, duplicate terminal outcome, malformed frame, oversized frame, unexpected EOF, or timeout is a sandbox failure;
- target stdout/stderr are never inspected for control messages.

An equivalent private one-shot mechanism is acceptable only when tests prove the target cannot write to it and cannot inherit authority over it. A shared output stream, environment-only assertion, or target-visible writable file is not acceptable.

The control payload should remain small. A practical limit is 4–16 KiB; do not build a general RPC protocol.

### 5.3 Private specification transport

The helper launch specification must not be created in the target cwd.

Use one of:

- a bounded anonymous descriptor/pipe dedicated to the specification; or
- a private temporary file under the CodeGG runtime directory or system temporary directory with owner-only permissions and deterministic cleanup.

Requirements:

- maximum serialized size remains bounded at or below the existing 64 KiB contract;
- the file or descriptor is created before helper launch;
- the target does not receive the specification path or descriptor after successful exec;
- cleanup runs on success, setup failure, exec failure, timeout, and cancellation;
- a read-only target cwd works;
- no secret is newly added to the specification.

Do not introduce a persistent cache or durable helper queue.

## 6. Required production-code changes

Inspect and change only the minimum required areas:

- `src/security/sandbox.rs`
  - remove production environment-based helper selection;
  - provide trusted production resolution and test-only injection;
  - retain typed `SandboxLaunchSpec` and maintained `landlock` ownership;
  - remove stderr marker constants after callers migrate.

- `src/bin/codegg-sandbox-helper.rs`
  - consume the private specification transport;
  - write bounded typed setup/exec status to the private channel;
  - close or close-on-exec the status writer so the target cannot inherit it;
  - preserve required-mode fail-closed behavior.

- `src/managed_process.rs`
  - create and supervise the private status channel;
  - move temporary specification creation outside the target cwd;
  - remove `parse_sandbox_result` or equivalent stderr scanning;
  - preserve target stderr byte-for-byte subject only to bounded retention;
  - distinguish setup failure, exec failure, target exit, timeout, cancellation, and cleanup diagnostics.

- focused tests and fixtures
  - extend existing managed-process and sandbox integration targets rather than creating a broad new framework.

- `scripts/check_sandbox_contract.py`
  - reject production `CODEGG_SANDBOX_HELPER` lookup;
  - reject reserved sandbox-marker parsing from stdout/stderr in governed paths;
  - include a negative self-test.

- architecture documentation
  - update `architecture/security.md`, `architecture/jobs.md`, and `docs/execution-ownership.md` only where needed to describe trusted helper resolution and the private status channel.

- planning
  - create `plans/closure/runtime-safety-resource-footprint/009-status.md`;
  - link the corrective result from M001 and M002 closure records without creating new ratification plans;
  - promote M003 to `ready` only when C001 closes;
  - classify M005/M006 remaining hosted conditions as operational rather than hard implementation blockers;
  - leave M007 blocked on M003 and M008 blocked on C001 plus the production milestones.

## 7. Ordered work packages

### Work package A — Reproduce and freeze the defects

1. Add a test proving an inherited `CODEGG_SANDBOX_HELPER` value cannot alter production helper resolution.
2. Add a target fixture that prints all former reserved marker strings to stderr.
3. Demonstrate that those strings currently affect or are removed from sandbox outcome processing before replacing the transport.
4. Add a read-only-cwd fixture showing the existing specification placement failure.
5. Record the exact governed source locations in the closure record.

Do not keep a test that requires production code to honor an arbitrary helper environment variable.

### Work package B — Make helper identity production-owned

1. Extract helper resolution behind a small internal resolver contract if needed.
2. Implement production sibling/install-root resolution.
3. Add test-only injection without a production environment/configuration override.
4. Reject missing and non-regular helper files before target spawn.
5. Document the installation-integrity threat-model boundary.

### Work package C — Replace stderr markers with a private channel

1. Define a minimal typed local status frame.
2. Create the parent/helper channel before spawn.
3. Pass only the helper endpoint to the helper.
4. Report setup success only after full Landlock enforcement and `no_new_privs` confirmation.
5. Ensure successful target exec closes the helper endpoint.
6. Report exec failure separately when `exec` returns.
7. Remove marker parsing and marker stripping from bounded stderr.
8. Fail closed on malformed, missing, oversized, duplicate, or timed-out status.

### Work package D — Move specification plumbing outside cwd

1. Select anonymous or owner-private temporary transport.
2. retain the 64 KiB bound;
3. set owner-only file permissions where a file is used;
4. ensure cleanup on every result path;
5. verify execution in a read-only cwd;
6. confirm the target cannot observe or retain the private transport after exec.

### Work package E — Supported-Linux enforcement run

On one Landlock-capable Linux host:

1. build the accepted corrective revision;
2. run `cargo test --test sandbox_landlock -- --test-threads=1`;
3. run the focused private-channel and helper-resolution tests;
4. record kernel version, effective ABI, and whether any test skipped;
5. require pass evidence for allowed read, read-only write denial, workspace write, outside-root denial, setup failure, and daemon-parent nonrestriction;
6. investigate any substantive skip or failure rather than converting it to documentation-only closure.

One supported host is sufficient. Do not add a target matrix.

### Work package F — Dependency and registry promotion

After production changes, focused tests, independent review, and supported-Linux evidence pass:

1. create the compact C001 closure record;
2. link it from M001/M002 closure records;
3. promote M003 to `ready` because the M002 executable/argv interface is stable and its substantive sandbox dependency is accepted;
4. record M005 and M006 remaining hosted conditions as operational evidence, not hard implementation dependencies;
5. keep M007 blocked only until M003 closes and final representative measurements can begin;
6. keep M008 blocked until C001 and M001–M007 have accepted dispositions;
7. require only one existing hosted `verify` result on the final combined PR revision when the normal trigger is available.

Absence of a manual workflow trigger must not create another corrective plan when focused local and supported-Linux mechanism evidence are green. Hosted verification remains merge evidence, not an independent architecture milestone.

## 8. Focused verification

Required focused cases:

- inherited helper override is ignored or rejected by production resolution;
- test-only helper injection remains possible without shipping an arbitrary override;
- missing helper fails before target execution;
- target stderr containing former enforced/error marker strings is preserved and cannot change sandbox outcome;
- status channel reports enforced ABI from the helper;
- target cannot retain or write the helper status endpoint after exec;
- missing status frame fails closed;
- malformed and oversized status frames fail closed;
- duplicate or contradictory terminal status fails closed;
- helper setup failure is distinct from target nonzero exit;
- target exec failure is distinct from setup failure;
- read-only cwd succeeds when policy and executable allow it;
- private specification cleanup occurs after success and failure;
- cancellation and timeout still terminate the managed process group;
- bounded target stderr behavior remains unchanged apart from removal of marker stripping;
- supported-Linux read/write/outside-root enforcement passes.

Expected command shape:

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets --locked
cargo clippy -p codegg --all-targets --locked -- -D warnings
cargo test -p codegg managed_process --lib -- --test-threads=1
cargo test -p codegg sandbox --lib -- --test-threads=1
cargo test --test managed_process_descendants -- --test-threads=1
cargo test --test sandbox_landlock -- --test-threads=1
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

Use current target names if they differ. Do not add a duplicate full-workspace run to this corrective pass.

## 9. Static guard requirements

Extend the existing sandbox contract guard rather than adding another script.

The guard must reject in governed production sources:

- reads of `CODEGG_SANDBOX_HELPER` or another arbitrary helper-path environment variable;
- helper resolution through PATH or target cwd;
- parsing sandbox outcome markers from target stdout or stderr;
- shared status/output marker constants used as the authoritative setup channel;
- helper specification creation rooted in the target cwd;
- an enabled sandbox path that continues after missing or malformed setup status.

The guard self-test must create temporary negative fixtures and prove each relevant pattern is detected. Keep the matcher narrow enough not to reject documentation, tests, or unrelated environment use.

## 10. Storage, protocol, migration, and compatibility effects

Storage:

- no database, RunStore, memory, job, or configuration migration;
- private helper specification files are ephemeral and owner-only;
- no historical data rewrite.

Protocol:

- no public daemon, frontend, ACP, tool, or provider protocol change;
- the private status frame is local child-process plumbing and is not a supported external API;
- no durable version negotiation is required beyond a small local frame version or enum discriminator.

Compatibility:

- ordinary unsandboxed execution is unchanged;
- explicit Linux sandbox execution remains fail-closed;
- non-Linux behavior remains explicitly unavailable/fallback according to the existing policy;
- target stdout/stderr become more faithful because former reserved strings are no longer consumed;
- read-only workspaces no longer fail merely because helper plumbing requires a cwd write;
- packaging must install the private helper at the production-owned expected location.

## 11. Security review requirements

The independent reviewer must inspect:

- production helper resolution and all test-only injection gates;
- descriptor/file ownership and lifecycle;
- whether the target can inherit or reconstruct the status writer;
- success-before-exec and exec-failure sequencing;
- malformed/missing status fail-closed paths;
- temporary specification permissions and cleanup;
- target stderr preservation;
- Linux Landlock outcome and ABI evidence;
- whether any new environment, cwd, PATH, symlink, or race condition can substitute the helper or status source.

No unresolved critical, high, or medium trust-channel finding may remain at closure.

## 12. Acceptance criteria

C001 is complete only when:

- production helper identity cannot be selected through inherited environment, PATH, or target cwd;
- tests use an internal test-only injection path rather than a shipped arbitrary override;
- sandbox setup status uses a private bounded channel that the target cannot write or retain after exec;
- target stdout/stderr are not parsed or stripped for sandbox control messages;
- helper setup failure, target exec failure, target exit, timeout, cancellation, and cleanup remain distinguishable;
- helper specification transport does not require a writable target cwd;
- specification and status resources are bounded and cleaned on every path;
- the existing supported-Linux fixture passes on one Landlock-capable host with kernel and ABI recorded;
- adversarial helper-substitution, status-spoof, malformed-status, missing-status, and read-only-cwd tests pass;
- the sandbox static guard and negative self-test pass;
- focused check, Clippy, tests, and `scripts/verify.sh quick` pass;
- one independent security review finds no unresolved critical/high/medium defect;
- `plans/closure/runtime-safety-resource-footprint/009-status.md` records the result;
- M003 is promoted to `ready` in `plans/registry.md`;
- M005/M006 operational hosted evidence is not retained as an independent M007 implementation blocker;
- no new CI lane, matrix, release automation, public protocol, or broader sandbox framework is introduced.

## 13. Stop conditions

Stop and report blocked when:

- a production-owned helper location cannot be defined without changing the installation contract;
- the target necessarily inherits the private status writer and no equivalent isolated one-shot transport is available;
- a supported-Linux run reveals a substantive Landlock policy or enforcement defect beyond this trust-channel scope;
- packaging does not install the helper and correcting packaging requires a separate release/distribution decision;
- the change requires a public daemon/protocol migration;
- preserving current target stdin requires a second private transport design that cannot remain bounded and local;
- tests demonstrate a process-group or cancellation regression owned by M002 rather than this helper boundary.

Create only the smallest follow-up for a genuinely separate blocker. Do not convert unavailable hosted workflow dispatch or runner-cache cleanup into another implementation milestone.

## 14. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/009-status.md` must include:

- accepted commit and PR revision;
- before/after helper trust and status-channel summary;
- production helper-resolution rule and test-only injection disposition;
- private specification and status transport description with byte bounds;
- adversarial substitution and stderr-spoof results;
- malformed, missing, duplicate, and oversized status results;
- read-only-cwd result;
- timeout/cancellation/process-group regression result;
- supported-Linux kernel, Landlock ABI, fixture outcomes, and any skips;
- static guard negative proof;
- focused commands and `scripts/verify.sh quick` result;
- independent security-review findings by severity;
- exact registry promotion changes for M003, M007, and M008;
- confirmation that no per-milestone hosted rerun or new CI lane was added;
- unresolved findings and final recommendation: closed, corrective pass required, or blocked.
