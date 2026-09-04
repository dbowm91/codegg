# Architecture Convergence M001 — Context and Compaction Ownership Closure

Status: conditionally closed

## 1. Executive finding

M001's production implementation is complete. CodeGG now has one canonical
CodeGG-specific context/compaction owner at `src/context/compaction.rs`, with
`eggcontext` retained as the dependency-free tokenization primitive. AgentLoop
now submits a typed request and consumes a typed result; the historical
`agent::compaction` path is a compatibility-only re-export.

The status is conditionally closed because the focused `tests/compaction.rs`
binary could not be linked on this host: the default pkg-config path selects
an arm64 MacPorts `liblzma` for the x86_64 Rust target, and an isolated retry
using the available x86_64 library reached an Apple clang linker crash while
building `sqlx-macros`. Workspace target checking, the owning crate tests,
quick verification, and targeted clippy all provide compile/static evidence;
the exact focused test binary should be rerun in CI or on a corrected host
toolchain.

## 2. Implementation revisions and dependency review

- The plan was activated and moved to `closing` when production wiring landed.
- Implementation: [`0809a64cc22e0ba547de078493066c34aa4d0fad`](https://github.com/dbowm91/codegg/commit/0809a64cc22e0ba547de078493066c34aa4d0fad)
  — `context: converge compaction ownership`.
- The implementation plan is moved from `closing` to `implemented` by the
  closure commit that adds this record and updates the planning controls.
- Reviewed dependencies: runtime consolidation M010, agent-runtime
  correctness M013, and provider session-context M009 are already closed.
  No dependency or ADR was required.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One canonical capacity and trigger owner | `ContextCapacity`, `context_tokens`, `needs_context_compaction`, and `compact_context` in `src/context/compaction.rs`; `AgentLoop::compact_if_needed` delegates to them | pass |
| Reserved output/tool budget is applied once | `context_capacity_subtracts_reserved_output_once`; typed result exposes `available_context_tokens` | pass |
| Typed compaction outcomes cover required states | `CompactionStatus`: Ready, CompactionRequired, Compacted, InsufficientCapacity, ProviderFailure, InvalidHistoryOrBudget, Cancelled | pass |
| AgentLoop consumes typed results rather than selecting policy | `src/agent/loop.rs::compact_if_needed`; no production calls to legacy compaction helpers remain in AgentLoop | pass |
| Legacy and hybrid behavior remain available under one owner | `compact_context` dispatches bounded drop-middle, legacy async, and hybrid/programmatic paths; old module is a re-export | pass |
| Provider session context is preserved | Existing `llm_compaction_uses_supplied_session_context` in `tests/compaction.rs`; canonical request carries `ProviderRequestContext` | pass by source and compile evidence; runtime test blocked by host linker |
| Cancellation is structured and non-mutating | `compact_context` cancellation test; cancellable hybrid/legacy wrappers race provider work and return unchanged `Cancelled` results | pass by source and compile evidence; runtime test blocked by host linker |
| History invariants and conservative fallback remain enforced | `validate_message_invariants`, typed invalid/provider-failure outcomes, and existing compaction invariant tests | pass by source and compile evidence; runtime test blocked by host linker |
| No persistence/protocol migration or sensitive-data expansion | No schema/protocol changes; compaction returns owned in-memory results and retains existing history/artifact boundaries | pass |

## 4. Before/after ownership map

| Concern | Before | After |
|---|---|---|
| Token primitive | `eggcontext` plus local estimates | `eggcontext`, consumed by the canonical context owner |
| Capacity/trigger policy | AgentLoop tracker checks plus compaction helpers | `ContextCapacity` and `needs_context_compaction` in `context::compaction` |
| Compaction selection | AgentLoop chose pruning, legacy, or hybrid branches | `compact_context(ContextCompactionRequest)` owns all selection |
| Provider-backed phases | Direct legacy/hybrid calls from AgentLoop | Canonical owner passes the supplied `ProviderRequestContext` |
| Result/failure state | Mutations and implicit fallbacks | `ContextCompactionResult` with explicit typed status and diagnostics |
| Context policy state | Defined in `agent/context_runtime.rs` | Defined in `context/policy.rs`; AgentLoop only stores turn-lifetime runtime state |
| Historical API | Implementation lived at `agent::compaction` | `agent::compaction` is a bounded re-export for source compatibility |

The root `src/context` adapter is intentional. Moving the full CodeGG
compaction engine into the dependency-free `eggcontext` crate would create a
crate-boundary cycle through provider/config/agent context-frame types. The
adapter is the single CodeGG policy owner and uses `eggcontext` only for its
tokenization primitive; no second policy implementation was introduced.

## 5. Production implementation

- Moved the production compaction implementation to
  `src/context/compaction.rs` and registered it from `src/context/mod.rs`.
- Added `ContextCapacity`, `ContextCompactionRequest`,
  `ContextCompactionResult`, `CompactionStatus`, model-aware token counting,
  canonical trigger evaluation, and cancellation-aware execution.
- Preserved legacy strategy behavior, hybrid/programmatic evidence extraction,
  invariant validation, emergency fallback, diagnostics, and provider model
  selection under the new owner.
- Reduced `AgentLoop::compact_if_needed` to policy invocation, hook handling,
  result application, frame/todo injection, and projection/event sequencing.
- Moved `ContextPolicyRuntimeState` to `src/context/policy.rs` and updated the
  TUI token helper to use the canonical module.
- Added `architecture/context-compaction-ownership.md` with the bounded
  production-path inventory and compatibility map; updated the agent,
  compaction, and cache-aware-context architecture contracts.

## 6. Deleted and retained compatibility paths

The former implementation body at `src/agent/compaction.rs` was deleted from
that owner and replaced with a re-export of `crate::context::compaction::*`.
It remains because existing integrations and tests use the historical public
module path. It contains no policy or provider implementation. The existing
`ContextPlan`, `ContextFrame`, volatile-tail, packer, and projection modules
remain because they own provider chronology, CodeGG evidence/frame state, or
separate bounded context representations; the architecture documentation now
explicitly classifies them as adapters rather than compaction owners.

## 7. Verification executed

Successful commands:

```text
rtk cargo test -p eggcontext
rtk cargo check -p codegg --all-targets
rtk cargo clippy -p codegg --lib -- -D warnings
rtk cargo fmt --all -- --check
rtk scripts/verify.sh quick
rtk git diff --check
```

Results:

- `eggcontext`: 18 tests passed.
- CodeGG all-target check: passed after the final fixture correction.
- CodeGG library clippy with `-D warnings`: passed.
- Formatting, generated-agent freshness, core-boundary, sandbox-contract,
  execution-ownership, and workspace quick verification: passed.
- `scripts/verify.sh quick`: passed, including locked workspace all-target
  checking.

The required strict command was also attempted:

```text
rtk proxy env CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
```

It reached the root crate but stopped on two unrelated pre-existing errors in
`src/server/ws.rs` where `queue_message(...)` returns `bool` but callers invoke
`.is_err()`. No diagnostic was emitted for the changed context code. The
focused `rtk cargo test -p codegg --test compaction` attempt was blocked at
link time by the host's x86_64/arm64 `liblzma` mismatch; the isolated
`PKG_CONFIG_PATH=/usr/local/lib/pkgconfig` retry reached an Apple clang
segmentation fault in `sqlx-macros` before executing tests.

## 8. Invariant, failure, and recovery review

- Provider-backed compaction receives the owning session context and does not
  detach work when a cancellation token is supplied.
- Cancellation returns unchanged input messages and a typed `Cancelled`
  result; provider failure records diagnostics and retains conservative legacy
  fallback behavior.
- Reserved capacity exhaustion returns `InsufficientCapacity` without
  discarding history. Results that remain over budget return
  `CompactionRequired` rather than silently claiming success.
- Tool-call/result invariants are revalidated after compaction and classify
  invalid history explicitly.
- No hidden reasoning, credentials, raw sensitive tool arguments, or new
  persistence records are added by this migration.

## 9. Migration and compatibility

No storage schema, protocol, or user-visible context-limit migration was
required. Existing session/history data remains readable. Existing callers of
`agent::compaction` continue to compile through the re-export and can migrate
to `context::compaction` incrementally. No historical transcript was rewritten.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| critical/high/medium | None in the changed context/compaction path | closed |
| low | Focused compaction binary cannot execute on this host because of native-linker/toolchain defects | named condition for this conditional closure; rerun on CI/corrected host |
| low | Full all-feature clippy is blocked by existing `src/server/ws.rs` bool/`is_err` errors | outside M001 scope; no changed-file diagnostic |

## 11. Roadmap disposition and downstream audit

M001 is conditionally closed and the architecture-convergence roadmap remains
active because M002, M003, and M008 are still independently ready. The
roadmap's M001 section now points to this closure record.

The registry and all architecture-convergence implementation-plan dependency
declarations were audited. M001's closure removes only the M001 prerequisite:

- M004 remains blocked on M002 and M003; M001 is no longer a blocker.
- M005 remains blocked on M003.
- M006 remains blocked on M004.
- M007 remains interface-blocked on M002.
- M008 remains independently ready.

Therefore no future plan becomes fully dependency-ready from M001 alone, and
no blocked plan status is changed. The next eligible work remains M002, M003,
or M008; M004 may start only after M002 and M003 close.

## 12. Registry updates

The closure commit:

- marks this implementation plan `implemented`;
- removes M001 from the dependency-ready implementation table;
- records the architecture-convergence roadmap as `active` with M001 closed;
- updates M004's blocker text to M002/M003;
- adds this record to the recently completed control points; and
- records that no future plan was unblocked by this closure.

Final disposition: conditionally closed pending only the named host-toolchain
focused-test rerun; no corrective implementation pass is required.
