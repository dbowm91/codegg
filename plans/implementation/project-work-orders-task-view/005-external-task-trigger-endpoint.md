# Project Work Orders and Task View M005 — External Task-Trigger Capability and Endpoint

Status: blocked

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Applicable ADR:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`

Primary class: capability / security

Hard dependency: M002 closure. Scheduled after M004 for handoff clarity; M004 is not a semantic dependency on the trigger service.

## 1. Objective

Add a narrow external trigger capability so shell scripts, CI glue, cron wrappers, and other local/remote automation can satisfy a declared `ExternalTrigger` gate on one WorkOrder occurrence without receiving a general CodeGG principal token or broader project authority.

The endpoint must be authenticated, idempotent, revocable, bounded, auditable, and safe against browser/link-preview/crawler activation. Trigger firing only latches a gate and wakes the existing WorkOrder coordinator; it never starts an agent/session directly.

## 2. Current implementation evidence

- M001 defines WorkOrder/occurrence/gate identity and project authorization.
- M002 defines persisted external-gate latching, occurrence release, and exactly-once materialization.
- CodeGG already has authenticated HTTP/WebSocket server paths, transport-bound principals, personal tokens, audit, and server middleware.
- Existing authorization intentionally separates bearer/principal authentication from project capability checks.
- There is no reason to expose a general project token merely to satisfy one release condition.

## 3. Invariants that must not regress

- Firing a task trigger can do exactly one semantic thing: satisfy the trigger gate it is bound to.
- Trigger secret is not a CodeGG principal credential and grants no list/get/edit/cancel/session/project access.
- Triggering uses `POST` or another explicitly mutating method; GET must have zero side effect.
- Trigger secret never appears in query strings, projections, logs, audit metadata, error bodies, or stored plaintext.
- Duplicate/replayed fire cannot create duplicate occurrence/session/job execution.
- Trigger fire does not bypass WorkOrder All/Any gate semantics or current project/policy validation at execution.
- Trigger endpoint does not execute an agent directly; it updates canonical WorkOrder state and wakes M002 coordinator.
- Revoked/expired/exhausted triggers fail closed and reveal minimal existence information.

## 4. Scope

### In scope

- typed trigger record/ID and verifier storage;
- cryptographically strong one-time displayed secret generation;
- hash/verifier-only persistence;
- create/list-metadata/revoke operations under normal authenticated project authorization;
- narrow unaffiliated fire endpoint authenticated only by trigger locator + secret;
- optional expiry timestamp and maximum fire count;
- idempotency key support for caller retries;
- occurrence/gate binding and re-arm semantics for finite repeats;
- audit/event records without secret content;
- rate limiting and bounded request/body behavior;
- replay/race/restart tests;
- TUI/CLI display of the generated trigger invocation only if an existing safe secret-display surface is available; otherwise protocol response is sufficient for M005.

### Explicitly out of scope

- generic webhooks that ingest arbitrary payloads and prompt content;
- GitHub/Slack/Linear/PagerDuty integrations;
- trigger-token use as a normal authenticated principal;
- GET query-string trigger links;
- arbitrary task mutation from trigger payload;
- dynamic prompt templating from HTTP request bodies;
- unbounded event subscriptions.

## 5. Trigger record

Minimum durable shape:

```text
TaskTrigger
  trigger_id
  project_id
  work_order_id
  gate identity / repeat binding policy
  secret_verifier + algorithm/version
  state: active | revoked | expired | exhausted
  created_by principal/origin decision
  created_at
  expires_at?
  max_fires?
  successful_fire_count
  last_fired_at?
  revision
```

Do not store the secret itself after creation response. Use a format with a public locator and secret segment, e.g. `cggtr_<id>.<secret>`, so the verifier can find the row without scanning all hashes.

Use the repository's established password/token hashing/verifier policy if suitable. If existing personal token hashing is reusable at the primitive level, reuse the cryptographic helper without making task triggers PersonalAuthToken records.

## 6. Endpoint contract

Preferred shape:

```text
POST /api/v1/task-triggers/<trigger-id>/fire
Authorization: Bearer cggtr_<trigger-id>.<secret>
Idempotency-Key: optional caller-generated value
Content-Length: 0 or a tiny bounded JSON metadata envelope if explicitly needed later
```

The initial endpoint should need no body.

Responses should be intentionally narrow:

- accepted/latch recorded;
- already fired/idempotent replay;
- not active/invalid credential using privacy-safe shape;
- rate limited;
- server/work-order capability unavailable.

Do not return prompt, project details, session IDs, model, lane ordering, or current gate state to the unaffiliated trigger caller unless strictly needed. A successful response may return a stable opaque fire receipt/idempotency result.

## 7. Create/revoke/list semantics

Authenticated normal CodeGG clients create/revoke triggers through project-scoped CoreRequest/service operations. Creation requires authority to modify/schedule the WorkOrder. Listing returns metadata only—never secret verifier or secret.

The generated secret is returned exactly once. If lost, revoke/create a new trigger.

Revocation is monotonic. Rotation is revoke + create unless a strong existing token-rotation primitive maps cleanly without widening scope.

## 8. Fire transaction and idempotency

On fire:

1. parse bounded public trigger ID and bearer format;
2. fetch trigger record by public locator;
3. constant-time/appropriate verifier check;
4. validate active/not expired/not exhausted;
5. enforce rate limit;
6. resolve current WorkOrder/occurrence gate binding;
7. in one transaction, insert fire receipt/idempotency record if supplied, latch the target gate if not already latched, increment bounded fire count if this is a new accepted fire, and record structural audit/event metadata;
8. wake WorkOrderCoordinator after commit.

Duplicate delivery for the same occurrence/gate must be harmless even without Idempotency-Key because the gate latch itself is unique/monotonic. Idempotency-Key additionally gives stable caller retry semantics and should be scoped to trigger ID with bounded retention.

## 9. Repeat/re-arm semantics

Define explicitly whether one trigger can fire multiple finite WorkOrder occurrences.

Recommended initial behavior:

- trigger record binds to the WorkOrder's external gate template;
- each occurrence has its own latch;
- after an occurrence is terminal and a repeat occurrence is created, the gate is unsatisfied again;
- the same active trigger may fire the next occurrence until `max_fires`, expiry, revocation, or WorkOrder repeat exhaustion;
- a fire arriving while the previous occurrence is already latched/running does not pre-latch a future occurrence unless an explicit queue-one policy is later added.

This prevents one accidental repeated HTTP request from releasing multiple future repeats.

## 10. Security controls

- high-entropy secrets from CSPRNG;
- bounded trigger ID/headers/path;
- no secret query parameters;
- no GET side effects;
- never log Authorization header;
- generic invalid/not-found response for public endpoint;
- IP/rate limiting consistent with server architecture; local IPC-only deployments may still bind trigger HTTP only when server mode is enabled/configured;
- TLS expectations follow existing remote-server policy;
- audit stores trigger ID/fire receipt/source metadata as policy permits, never secret;
- request body disabled or tiny and ignored in initial version;
- CSRF is not relied upon as primary defense because endpoint uses bearer secret and no browser cookie auth, but GET remains inert;
- timing/error behavior should not make trigger enumeration materially easier than random guessing.

## 11. Authorization boundary

The fire endpoint's trigger bearer is a narrow capability, not an `AuthenticatedPrincipal`. Do not insert it into general team membership/principal resolution.

The *creation/revocation* operations are ordinary principal-authorized project mutations. The *fire* operation verifies the trigger capability and may record an actor kind such as `service-trigger:<id>` for audit without granting normal principal capabilities.

At eventual execution M002 still revalidates current project/model/approval/sandbox policy. A trigger cannot resurrect authority that has been revoked.

## 12. Failure/restart/race semantics

- verifier/store failure: no latch;
- audit/event append should follow the project's established failure policy; do not claim accepted if canonical latch transaction failed;
- wake failure after committed latch is recoverable via coordinator due/reconciliation scan;
- concurrent valid fires latch once and do not exceed max-fire count incorrectly;
- fire racing revocation/expiry uses transaction/revision ordering with one deterministic winner;
- daemon restart preserves active/revoked/exhausted state and accepted idempotency receipts;
- WorkOrder cancellation makes future fire fail or become inert without revealing additional details;
- already-running occurrence fire is idempotent/inert for that occurrence, not queued for the next repeat.

## 13. Ordered work packages

### A — Trigger domain/store/verifier

Add typed record, secret format/generation/verifier, migration if not included in M001, metadata listing, revocation, expiry/max-fire rules, and unit tests.

### B — Authorized management protocol

Add create/revoke/list-metadata CoreRequest/Response operations, authorization descriptors, origin/audit attribution, and capability negotiation.

### C — Narrow HTTP fire route

Add POST route/middleware path that verifies only task-trigger bearer, performs latch transaction, returns bounded privacy-safe result, and wakes coordinator.

### D — Idempotency/rate/repeat handling

Add receipt table/retention, duplicate/concurrent fire handling, max-fire counter, repeat re-arm behavior, and rate limiting.

### E — Security/restart qualification and docs

Add endpoint/security docs, curl example with header rather than URL secret, restart/race tests, and log-redaction/static checks.

## 14. Required tests

Crypto/record:

- generated secrets have required entropy/format and are never stored plaintext;
- wrong secret fails;
- verifier version round trips;
- revoke/expire/exhaust transitions.

HTTP:

- POST valid fire accepted;
- GET returns no side effect;
- missing/wrong bearer privacy-safe;
- query-string secret is ignored/rejected;
- oversized/body/header/path rejected;
- Authorization never appears in captured logs/errors;
- rate limit behavior.

Idempotency/races:

- same Idempotency-Key returns same receipt;
- different keys/concurrent requests still latch one occurrence once;
- fire vs revoke;
- fire vs expiry boundary;
- fire while occurrence already running does not arm next repeat;
- next repeat can be fired after re-arm;
- max-fire exhaustion exact under concurrency.

Recovery:

- committed latch + simulated wake failure still executes after reconciliation;
- restart preserves revoke/expiry/count/idempotency state;
- cancelled WorkOrder cannot be reactivated by trigger.

Authorization:

- trigger bearer cannot call normal Core APIs;
- unauthorized principal cannot create/list/revoke trigger metadata;
- list output excludes verifier/secret.

## 15. Required verification

```bash
cargo test -p codegg-core -- task_trigger
cargo test -p codegg --lib -- task_trigger
cargo test --test server -- task_trigger
cargo test --test authorization -- task_trigger
cargo test --test audit -- task_trigger
python3 scripts/check_authorization_matrix.py
python3 scripts/check_secret_boundaries.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Use current equivalent secret-boundary guard name if the repository differs; closure must record the exact command.

## 16. Acceptance criteria

- Authorized user can create a trigger and receives its secret once.
- External script can POST with the trigger bearer and latch only the intended gate.
- GET/query-string access cannot fire a task.
- Duplicate/concurrent fire cannot duplicate session/job execution or pre-release a later repeat.
- Trigger can be revoked/expired/fire-bounded and survives restart.
- Trigger bearer cannot access any other CodeGG API/resource.
- Secrets are absent from persistence projections/logs/audit/error text.
- A committed latch eventually reaches M002 coordinator even if immediate wake delivery fails.

## 17. Stop conditions

Stop if the endpoint requires turning trigger tokens into ordinary principals, accepts prompt/task mutation payloads, uses GET/query secrets, stores plaintext secrets, or starts agent/session execution directly from HTTP route code.

## 18. Closure evidence required

- implementation/migration commits;
- trigger secret/record schema with redacted sample;
- endpoint request/response contract;
- GET-no-side-effect and log-redaction evidence;
- idempotency/concurrency/max-fire/repeat matrix;
- restart/wake-failure evidence;
- authorization negative tests;
- exact verification commands and residual findings.
