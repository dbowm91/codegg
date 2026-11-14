# CodeGG Embedding Host Milestone 004 — Integration evidence and closure

Status: proposed

Repository baseline: `1d2d9dce2`

Source roadmap:

- `plans/subsystems/codegg-embedding-host-roadmap.md#7-milestones` (E004)

Long-term requirements:

- `plans/000-long-term-specification.md` — host-neutral execution, security of secrets
- `plans/002-long-term-roadmap.md` — headless capabilities

Applicable ADRs:

- ADR-0061 (accepted) — the embedding wire is a public contract.

Primary class: infrastructure

## 1. Objective

Prove the embedding contract on a real host and a real Linux build: a descriptor
with headers and a model outside the reviewed list, a streamed turn, a cancelled
turn, a typed provider error, and a clean restart. Produce the closure record
that lets a host plan consume the SDK.

## 2. Why this milestone is ready

E001–E003 closed: descriptor, host protocol and SDK exist and their conformance
suites pass. The only outstanding dependency is a real host run, which this
milestone owns.

## 3. Current implementation evidence

- E001–E003 closure records.
- The pinned Linux build used for the host integration (kernel, initfs, image
  digest) recorded in the closure.
- A host project already driving CodeGG over ACP; the migration plan on that side
  is blocked on this milestone.

## 4. Invariants that must not regress

- The evidence uses the shipped binaries and the pinned build; no local patch.
- The descriptor used in evidence carries a real header requirement, not a
  synthetic one, and its credential is never printed.
- ACP and `exec` remain unchanged and are re-run.
- No new surface is added by this milestone.

## 5. Scope

### In scope

- A real session on Linux with a descriptor-declared connection.
- A streamed turn with at least one tool call.
- A cancelled turn and a subsequent successful turn.
- A typed provider failure produced by a real upstream (for example a model
  without the required capability) and the class it maps to.
- A process restart and a fresh session.
- The closure record and the roadmap status update.

### Explicitly out of scope

- New protocol features.
- A second host.
- Performance benchmarking beyond the existing operational bounds.

## 6. Required production changes

None. This milestone produces evidence only. Any defect found is recorded and,
if it requires code, a corrective plan is created rather than patched in place.

### Documentation and static guards

- Update the roadmap's milestone table.
- Record the exact host sequence in `docs/embedding.md` as the worked example's
  verification reference.

## 7. Ordered work packages

### Work package A — Real descriptor session

Intent: prove the descriptor is usable, not merely parseable.

Required changes: none. Run the host with a descriptor whose endpoint needs a
header and whose model is outside every built-in list.

Acceptance evidence: the transcript shows the connection id, the model, the
header name (not value), and a completed turn.

### Work package B — Streaming, tool use and usage

Intent: prove the event contract on real traffic.

Acceptance evidence: ordered `turn/text` notifications, at least one
`turn/tool`, and a `turn/done` with reported usage.

### Work package C — Cancellation and restart

Intent: prove the operational bounds.

Acceptance evidence: cancel during a slow turn ends within the bound; the next
turn succeeds; a process restart opens a fresh session with no residual state.

### Work package D — Typed failure and closure

Intent: prove the taxonomy is useful and close the line.

Acceptance evidence: a real upstream failure surfaces the documented class; the
closure record recommends `closed`; the roadmap's milestone table is updated.

## 8. Failure, cancellation, restart, and contention semantics

The evidence must show the semantics the plans claim: a provider failure ends a
turn rather than the process, cancellation leaves the process usable, EOF exits
cleanly, and a restart loses only session state.

## 9. Compatibility and migration

No compatibility change. The evidence re-runs the ACP and `exec` suites to prove
they were not disturbed.

## 10. Required tests

### Integration tests

- The real descriptor session, streaming, cancellation and restart transcripts.

### Security and negative tests

- A scan of the captured logs proves no credential and no header value appears.

### Migration and compatibility tests

- ACP and `exec` suites re-run unchanged.

## 11. Required verification commands

```bash
cargo test -p codegg --bin codegg host
cargo test -p codegg --bin codegg exec
cargo test -p codegg-sdk
scripts/verify.sh scoped
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Plus the real host sequence on the pinned Linux build, recorded verbatim in the
closure.

## 12. Acceptance criteria

1. A real descriptor session completes on the pinned Linux build.
2. The descriptor's header requirement is satisfied by the descriptor, with no
   proxy and no config file.
3. A model outside the reviewed list works on the declared connection.
4. Streaming, tool use and usage are observed.
5. Cancellation is bounded and a later turn succeeds.
6. A real provider failure carries the documented class.
7. A restart opens a fresh session cleanly.
8. Logs contain no credential and no header value.
9. ACP and `exec` are unchanged.

## 13. Stop conditions

Stop and re-scope if:

- The real connection cannot be used without a transport outside the descriptor.
- A secret appears in any captured evidence.
- The failure taxonomy cannot classify a real upstream failure.

## 14. Required closure evidence

Closure record under `plans/closure/codegg-embedding-host/004-status.md`:

- the pinned build identity;
- the full host transcript (initialize → open → submit → events → done →
  cancel → restart);
- the descriptor's header name and model, with the credential redacted;
- the failure class evidence;
- the log scan;
- the ACP/`exec` regression results;
- the recommendation and the roadmap update.
