# Development Verification and Release Milestone 009 — Built-in Agent Test Contract Alignment

Status: implemented — closed; see `plans/closure/development-verification-release/009-status.md`

Repository baseline: `184fd07dd1c7e6c176aa176d9d12ce1d4f193b0d`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md#milestone-009--built-in-agent-test-contract-alignment`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/003-planning-process.md#7-corrective-passes`

Primary class: polish / verification

## 1. Objective

Align the stale built-in-agent unit-test expectations with the repository's
checked-in built-in-agent assets and generated output: ten built-ins, including
the hidden prompt-bearing `verifier` agent.

## 2. Why this milestone is ready

Hosted CI run `33839695302` on exact candidate `184fd07` passed all static
checks and Workspace Clippy, then failed eight existing agent tests because
they still expected nine built-ins and treated every hidden agent other than
`compaction` as prompt-free. The source assets, README, and generated Rust
already consistently define ten agents and the `verifier` prompt. This is
independent of memory-to-skill M005 and the M008 ReviewTool ordering fix.

## 3. Scope

In scope:

- updating stale test counts and expected built-in names;
- allowing the documented hidden prompt-bearing `verifier` exception;
- focused agent tests and the normal hosted verification lane.

Out of scope:

- built-in agent definitions, prompts, permissions, runtime behavior, or
  generated assets;
- agent-registry resolution logic;
- Clippy policy, CI configuration, lint suppressions, or toolchain changes.

## 4. Required verification

```bash
cargo fmt --all -- --check
cargo test -p codegg agent --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

The closure record must cite the hosted run on the exact accepted SHA and
record the downstream dependency audit.

## 5. Acceptance criteria

- The eight stale agent tests pass against the existing ten-agent assets.
- No production agent definition or behavior changes are made.
- Workspace Clippy and quick verification pass.
- The normal hosted `CI / verify` lane reaches and passes Workspace tests.

## 6. Stop conditions

Stop if alignment requires changing an agent definition, prompt, permission,
registry behavior, CI policy, or another subsystem boundary; register a new
follow-up instead.

## 7. Closure evidence required

Create `plans/closure/development-verification-release/009-status.md` with the
exact implementation commit, focused/local verification, hosted run/job,
invariant and compatibility review, and registry/dependency audit.
