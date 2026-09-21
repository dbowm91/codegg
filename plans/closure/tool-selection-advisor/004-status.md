# Tool-Selection Advisor Milestone 004 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-selection-advisor/004-consent-gated-training-telemetry.md`
Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-roadmap.md#m004--consent-gated-local-capture-and-remote-ready-telemetry`
Repository baseline reviewed: `9681182`
Implementation commits: `0397b71` — consent-gated training data lifecycle; `18cd692` — retention/no-op evidence

## 1. Executive finding

M004 is complete. Training-data events now have a separate versioned lifecycle
from security/audit records, with default-off Noop behavior, explicit bounded
local capture, inspect/export/purge controls, content consent distinct from
metadata consent, redaction before content persistence, and an explicit HTTPS
remote sink contract. Advisor use and local capture do not imply remote upload.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Versioned event envelope | `ToolAdvisorTrainingEvent`, schema version 1, bounded field/candidate validation |
| Default Noop | `NoopSink`; default policy status `Disabled`; no-file test |
| Bounded local spool | `LocalSpoolSink`, atomic temp/rename writes, age/byte pruning, retention test |
| Inspect/export/purge | sink methods and `codegg tool-advisor data status|inspect|export|purge` |
| Independent consent | `TrainingDataPolicy` gates capture/content/remote/remote-content independently |
| Redaction/data minimization | `redact_sensitive`, metadata-only removal, credential-pattern tests through spool evidence |
| Remote-ready transport | HTTPS validation, existing Eggfetch builder, explicit endpoint/token, injectable fake transport |
| Audit separation | module has no security/audit store dependency or migration |
| Failure isolation | sink errors are returned to caller and are not wired into agent execution; queue corruption is quarantined |

## 3. Production implementation evidence

Events contain model/mode/surface/candidate/score/choice/outcome/latency
metadata, with optional context only under explicit content consent. Local spool
records are individually bounded and uniquely named. Remote payload creation
requires an explicit HTTPS destination; there is no built-in endpoint or
background uploader. The concrete HTTP transport reuses the repository's
Eggfetch construction seam rather than adding a second HTTP/retry owner.

## 4. Verification executed

Local verification:

- `cargo test --lib tool_advisor` — 15 advisor/data tests passed.
- `cargo test --lib tool_advisor::training_data` — 5 focused tests passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `cargo run --locked --bin codegg -- tool-advisor data status` — `state: Disabled`; managed spool path printed; no default endpoint or network call.
- Fake remote transport test — one call only after explicit HTTPS remote policy; metadata-only body excludes context.

These are local results; no hosted/CI result is claimed.

## 5. Invariant review

Default configuration is network-silent and file-silent. Local capture cannot
enable remote upload. Remote content requires both remote and content consent.
Captured data is not inserted into operational audit storage and no transcript,
raw tool result, environment, credential, or API key is captured by default.

## 6. Failure and recovery review

Malformed records are quarantined with a `.corrupt` extension. Writes are
atomic, pruning is bounded, and inspect/export/purge are restart-safe. Remote
transport errors remain explicit sink errors and cannot fail an agent turn when
the sink is used best-effort by a future caller. Turning remote policy off
creates no remote sink and therefore stops future attempts.

## 7. Migration and compatibility review

No database, provider, session, or audit migration. Event schema is independent
of model artifact schema and rejects unsupported versions rather than silently
reinterpreting them.

## 8. Security review

HTTPS is required for configured remote endpoints. User tokens are supplied to
the sink at construction and are not stored in event records. Common bearer,
API-key, password, `sk-`, `ghp_`, and `xoxb-` patterns are redacted as defense
in depth; this is not claimed to be complete secret detection.

## 9. Documentation and operations

`architecture/tool-advisor.md` documents fields, exclusions, consent gates,
redaction limitations, and CLI operations. The CLI default status command
exposes the effective disabled state and managed spool root.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Low: redaction is intentionally pattern-based and cannot guarantee removal
  of every secret; consent and content minimization remain the primary control.
- Low: no project-hosted collector exists by design; explicit remote transport
  qualification uses an injected fake transport rather than a real external
  service.

No critical, high, or medium findings remain.

## 11. Roadmap disposition

M004 is closed. M005 is dependency-ready: M001 benchmark, M002 runtime,
M003 training, and M004 data lifecycle all have accepted closure records.

## 12. Registry updates

The dependency audit checked every registered tool-selection advisor plan:

- M005 moved from `blocked` to `ready`.
- No other registered plan depends on M004.
- No corrective pass is required; remote service operation remains an explicit
  non-goal of this workstream.
