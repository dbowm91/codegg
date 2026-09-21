# Tool-Selection Advisor Milestone 004 — Consent-Gated Training Telemetry

Status: implemented

Repository baseline: `ed960f2ae7043acd816970f7d06e74dc69b09ae8`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-roadmap.md#m004--consent-gated-local-capture-and-remote-ready-telemetry`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

Hard dependency: M001 accepted closure.

## 1. Objective

Create a training-data lifecycle that can capture useful local tool-selection trajectories only when explicitly enabled, lets users inspect/export/purge them, and provides a remote-capable sink contract for future voluntary training contribution without enabling any remote destination by default.

## 2. Why this milestone is blocked

M001 must define the canonical case/prediction schema. M004 should extend that contract with runtime provenance/outcomes rather than create a second incompatible dataset representation.

## 3. Current implementation evidence

CodeGG has security/audit event machinery and some shell-specific redaction, but no general consent/analytics subsystem. Those audit streams have different retention and trust semantics and must not be repurposed as hidden training telemetry.

The existing `eggfetch-core` HTTP path may be reusable for an explicit remote sink after consent, but telemetry must not create another HTTP/retry authority or automatic background service.

## 4. Invariants that must not regress

- Telemetry/capture default is off.
- Advisor enablement does not enable capture.
- Local capture does not enable remote upload.
- Remote metadata consent does not imply task/repository content consent.
- No default remote endpoint.
- No credential, API key, raw secret, or unrestricted tool argument/result capture.
- Security audit logs and training events remain separate.
- Data can be inspected and purged locally.
- Remote-disabled tests prove zero telemetry network attempts.

## 5. Scope

### In scope

- Versioned `ToolAdvisorTrainingEvent` envelope.
- `NoopSink` default.
- Explicit `LocalSpoolSink` with size/age bounds and atomic records.
- Commands/API to status, inspect, export, and purge.
- Data classification: metadata-only vs training-content.
- Secret/redaction boundary before content-capable persistence/export.
- Remote sink interface and optional explicit HTTP implementation using the existing HTTP client ownership.
- User-supplied HTTPS endpoint/token; no built-in destination.
- Bounded retry/backoff only when remote mode is explicitly enabled, reusing existing HTTP/retry policy where applicable.
- Fake-local-server transport tests.
- Consent/config diagnostics suitable for TUI/CLI exposure.

### Explicitly out of scope

- Operating a CodeGG telemetry server.
- Automatic upload on installation/update.
- Inferring consent from account/provider login.
- Capturing full transcripts, file contents, tool outputs, or environment variables by default.
- Retrofitting unrelated product analytics.

## 6. Required production changes

### Event schema

Record only what is needed to reconstruct/evaluate a tool-selection example:

- event/schema version and random event ID;
- advisor artifact/version/mode if present;
- resolved surface fingerprint;
- candidate descriptors or stable descriptor hashes plus required semantic fields;
- advisor scores/abstention if present;
- main-agent discovery/invocation choices;
- coarse outcome/error class and latency;
- optional compact task/context projection **only** when training-content capture is enabled;
- provenance/consent flags.

Do not record chain-of-thought.

### Configuration

Use distinct gates, conceptually:

```toml
[tool_advisor.training_data]
capture = "off"            # off | local
include_content = false
max_bytes = ...
max_age_days = ...

[tool_advisor.training_data.remote]
enabled = false
endpoint = ""               # no default
include_content = false
```

Remote content must require both remote enabled and content enabled; configuration loading should make that explicit in diagnostics.

### Redaction

Before content is persisted/exported, apply a shared redaction boundary for obvious credentials/tokens. If shell redaction cannot be safely generalized, create a small shared secret-redaction utility and migrate only appropriate callers rather than coupling telemetry to shell code.

Redaction is defense in depth, not a substitute for consent/data minimization.

### Transport

Do not create a background daemon. A remote sink can flush at explicit sync points/commands or bounded existing lifecycle hooks. Queue state is durable and bounded only when remote mode is explicitly enabled.

## 7. Ordered work packages

A. Event/privacy schema and consent-state tests.
B. Noop/local bounded spool.
C. Inspect/export/purge commands.
D. Shared redaction boundary.
E. Remote sink interface + explicit endpoint HTTP adapter/fake-server tests.
F. Documentation and network-silence guard.

## 8. Failure, cancellation, restart, and contention semantics

Telemetry failure must never fail an agent turn. Local spool writes should be atomic/best-effort and bounded. Queue corruption should quarantine/drop affected telemetry with diagnostics rather than block CodeGG.

Remote failure leaves bounded queued events only when remote mode is enabled. Cancellation stops upload promptly. Restart may resume only explicitly opted-in queued remote work. Turning remote mode off must stop future attempts immediately.

## 9. Compatibility and migration

Event records carry schema versions. Unsupported old versions remain inspectable/exportable when feasible but must not be silently reinterpreted for training.

No existing audit database is migrated.

## 10. Required tests

- default/off -> no files and no telemetry network calls;
- local capture explicit -> bounded spool;
- remote disabled despite local capture -> no network;
- remote metadata-only excludes content;
- remote content requires explicit second gate;
- no default endpoint;
- HTTPS validation/config rejection;
- redaction of representative credentials;
- inspect/export/purge lifecycle;
- retention/size pruning;
- concurrent event writes;
- fake-server success/retry/failure;
- turning remote off halts queued attempts.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool_advisor
cargo test --test tool_advisor_training_data
cargo test --features server --test <telemetry-fake-server-target>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Also run a network-attempt spy/fixture proving default configuration never reaches the telemetry transport.

## 12. Documentation updates

- exact data fields and exclusions;
- local capture enable/disable;
- inspect/export/purge;
- remote opt-in and destination visibility;
- content-consent distinction;
- retention and redaction limitations.

## 13. Acceptance criteria

M004 closes only when training data is default-off, locally manageable, independently consented for remote use, bounded/redacted, network-silent without remote opt-in, and unable to affect agent execution on failure.

## 14. Stop conditions

Stop if implementation would reuse security audit retention semantics, add a default collection endpoint, infer consent, transmit raw secrets/tool payloads, or introduce a second generic HTTP/retry subsystem.

## 15. Closure evidence required

- data-field classification table;
- consent-state matrix;
- no-network-by-default test evidence;
- retention/concurrency/restart evidence;
- redaction tests and limitations;
- fake-server remote transport evidence;
- inspect/export/purge demonstration.

## 16. Handoff notes

Treat task/repository text as sensitive even when it looks innocuous. Metadata-only remote contribution should remain useful without requiring content, and content contribution must remain a visibly separate choice.
