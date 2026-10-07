# Provider Connections Milestone 010 — Provider Connection Qualification Semantics

Status: ready for handoff

Repository baseline: `d85ed67bef970cfe99e320a7876e7a51373b7e37`

Source roadmap:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md#milestone-010--provider-connection-qualification-semantics`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Applicable ADRs:

- None. This milestone corrects semantics inside the existing daemon-owned connection boundary.

Primary class: capability

## 1. Objective

Make `/connect` and durable provider health truthful by separating catalog discovery from credential verification. A successful model listing must no longer mean "API key verified", and inability to perform a safe non-billable credential probe must produce an explicit unverified state rather than a false failure or false healthy result.

## 2. Why this milestone is ready

Provider Connections M009 is closed, including direct-call session-context correctness. The existing durable connection store, secret references, provisioning transaction, TUI flow, model catalog projection, and provider error taxonomy are available.

No EggPool extraction is required for this semantic correction. M011 will consume the shared provider-profile contract after it exists.

## 3. Current implementation evidence

- `crates/codegg-providers/src/setup_catalog.rs` assigns setup probe strategies including `DirectModels`.
- Provider-neutral provisioning in `src/core/eggpool.rs` uses `provider.models()` under that strategy as a connect-time health/authentication decision.
- `OpenAiProvider::models()`, `AnthropicProvider::models()`, `GoogleProvider::models()`, and `OpenRouterProvider::models()` return local/static model arrays and therefore perform no credential validation.
- `OpenAiCompatibleProvider::models()` is best-effort discovery and intentionally falls back on transport/status failures in ordinary runtime use.
- OpenCode Go's public `/zen/go/v1/models` endpoint returns a model list without requiring proof that an inference credential is accepted.
- The TUI success path currently renders a generic "connected" outcome with model count and closes the dialog, while failures are collapsed into one provisioning error path.
- Existing provider errors already preserve HTTP authentication classes sufficiently to distinguish 401/403 from transient transport errors.

## 4. Invariants that must not regress

- Credentials stay in daemon-owned secret storage and never return to TUI/protocol DTOs.
- Failed provisioning/rotation remains rollback-safe and secret-safe.
- Catalog discovery remains bounded and may succeed independently of credential verification.
- No provider's local/static `models()` implementation is ever treated as authentication proof.
- No automatic billable completion is sent only to verify a credential.
- A credential may be configured but unverified.
- Genuine authenticated inference success may promote unverified to verified.
- Genuine 401/403 may transition the relevant credential/connection to authentication-failed.
- Catalog/network outage must not be mislabeled "invalid key".
- Existing selected-session/provider semantics and revision checks remain stable.
- Health transitions must be revision-aware so an old in-flight response cannot mark a newly rotated credential healthy or failed.

## 5. Scope

### In scope

- Explicit separation of catalog discovery and credential-verification results.
- Connection status/health representation for configured-unverified versus verified/authentication-failed.
- Provider provisioning behavior and TUI messaging.
- Runtime inference outcome feedback into connection health where the current connection manager can attribute provider/credential revision safely.
- Focused fake-provider tests for static, public-catalog, authenticated-catalog, auth-failure, and transient-outage cases.
- Documentation/static guards preventing `models()` from becoming an auth probe again.

### Explicitly out of scope

- OpenCode Go multi-surface dispatch; M011 owns it.
- Shared EggPool provider-profile consumption; M011 owns it.
- Dummy/billable inference probes.
- Provider routing/fallback redesign.
- New credential stores or encryption.
- Broad provider-health scheduler redesign.
- New provider additions.

## 6. Required production changes

### Core qualification model

Replace the implicit "probe succeeded" boolean with typed semantics. Exact names may follow current domain conventions, but the behavior must distinguish at least:

```text
CatalogOutcome
  - available(models)
  - unavailable/transient
  - rejected/invalid-shape

CredentialVerification
  - verified
  - unverified
  - authentication_failed
  - no_credential_required
```

Do not overload one enum with both transport/catalog and authentication meaning if that makes state transitions ambiguous.

`SetupProbeStrategy::DirectModels` must stop meaning "authenticate by calling models". It may be removed or retained only as a catalog-discovery strategy with a name/contract that cannot be mistaken for credential verification.

### Provisioning semantics

During `/connect`:

1. validate endpoint/provider definition locally;
2. stage/store the secret according to the existing transactional contract;
3. perform bounded catalog discovery if supported;
4. perform a non-billable authenticated credential probe only when the provider has a real such contract;
5. otherwise persist the connection as configured + credential-unverified;
6. commit one durable connection result with separate catalog and credential states.

A provider with a public/static catalog must not fail provisioning merely because the catalog cannot validate its key.

A true authenticated metadata 401/403 may fail or persist as auth-failed according to existing provisioning transaction semantics, but the user-facing reason must explicitly be authentication rejection.

Transient DNS/TLS/timeout/5xx/catalog failures must remain distinct from auth rejection.

### Runtime inference feedback

At the daemon/provider-turn boundary where a request is already associated with a concrete provider connection and revision:

- first successful authenticated inference may mark that exact revision verified/healthy;
- 401/403 may mark that exact revision authentication-failed;
- transient/retryable provider failure must not become auth-failed;
- stale outcomes from a superseded credential revision are ignored.

Do not add provider-specific string matching in the agent loop. Use existing typed provider error/status classification.

### Persistence compatibility

Prefer extending the existing health/status representation without a schema rewrite. If current storage uses a closed enum/constraint that cannot represent unverified safely, use an additive migration with deterministic mapping from old records.

Existing records previously labeled healthy solely from a local/static model list must not be silently asserted as credential-verified after migration. Use a conservative mapping unless there is durable evidence of authenticated success.

### TUI/operator surface

Keep the current `/connect` flow, but make the completion text truthful. Examples of semantic outcomes:

- "configured; credential verified; N models"
- "configured; credential not yet verified; N models"
- "authentication rejected"
- "configured; catalog unavailable; credential not yet verified" only if current product policy allows creation without catalog; otherwise preserve existing model-selection requirement and explain the catalog failure separately.

Do not expose raw credential or header values.

### Static guard

Add a focused guard/test that fails if provisioning again equates `Provider::models()` success alone with credential verification.

## 7. Ordered work packages

### Work package A — Introduce typed qualification outcomes

Separate catalog and credential-verification semantics in provider connection/provisioning code.

Acceptance evidence: unit tests covering impossible state combinations and redacted DTO projection.

### Work package B — Correct provisioning

Refactor current setup strategies so static/public/best-effort model lists cannot prove auth.

Acceptance evidence: fake-server tests for public catalog + invalid inference key, static catalog provider, authenticated catalog 401, and transient catalog outage.

### Work package C — Add revision-safe inference feedback

Feed typed authenticated success/401/403 back to the concrete connection revision.

Acceptance evidence: rotation/stale-result test and auth-vs-transient classification tests.

### Work package D — Correct TUI and diagnostics

Render verified/unverified/auth-failed separately, preserving secret clearing and stale-operation guards.

Acceptance evidence: reducer/dialog tests or current equivalent.

### Work package E — Documentation and closure

Update provider architecture and connection docs, add guard, run focused + quick verification.

## 8. Failure, cancellation, restart, and contention semantics

Cancellation during provisioning must retain the existing no-orphan/no-plaintext guarantees. If a secret/record is staged and the operation is cancelled before commit, roll back according to the current transaction contract.

Restart must preserve the durable qualification state. Unverified remains unverified until new evidence arrives.

Concurrent inference outcomes for one revision may converge monotonically: authenticated success can establish verified; a subsequent genuine 401/403 for the same revision may mark auth-failed according to current health policy. Outcomes from older revisions must never overwrite newer credential state.

Catalog refresh concurrency remains governed by existing connection refresh machinery and must not be coupled to auth state updates.

## 9. Compatibility and migration

Existing direct-provider configuration and connection IDs remain valid.

If an additive migration is necessary, document exact old-state mapping. Prefer conservative downgrade from historically ambiguous "healthy" to "unverified" only where the repository can identify that no authenticated evidence exists; do not globally degrade connections with durable real inference health evidence.

Protocol DTO changes must be additive/backward-compatible where possible. Old clients that only understand broad health should receive a safe projection rather than secrets or false verification claims.

## 10. Required tests

Focused unit/integration tests must include:

- static-model provider + arbitrary bad key: catalog available, credential not verified;
- public `/models` provider + arbitrary bad key: catalog available, credential not verified;
- authenticated metadata endpoint 401/403: auth rejection;
- transient timeout/5xx: not auth rejection;
- successful real/fake inference promotes exact revision to verified;
- inference 401/403 marks exact revision auth-failed;
- rotated credential ignores late old-revision success/failure;
- TUI success text differentiates verified vs unverified;
- secrets are absent from diagnostics/DTOs;
- provisioning cancellation leaves no committed partial record beyond current policy;
- existing lifecycle/rotation tests remain green.

## 11. Required verification commands

Use the smallest current package/test targets first, then:

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-providers --all-targets -- -D warnings
git diff --check
scripts/verify.sh quick
```

Also run the existing provider-connections lifecycle/provisioning suites that cover M005/M008/M009 semantics. Closure must record exact commands/results.

## 12. Documentation updates

- `architecture/provider.md`
- provider connection/operator documentation containing `/connect` semantics
- `plans/closure/provider-connections/010-status.md`
- `plans/registry.md`

## 13. Acceptance criteria

M010 may close only when:

1. `Provider::models()` success alone cannot mark a credential verified.
2. Static model lists cannot validate keys.
3. Public OpenCode Go catalog reachability cannot validate keys.
4. Providers without safe non-billable verification can be represented as configured/unverified.
5. No automatic billable dummy completion is used.
6. Real inference success can verify the exact active revision.
7. Real 401/403 can mark the exact active revision auth-failed.
8. Transient/catalog failures remain distinct from auth rejection.
9. TUI/operator output is truthful and secret-safe.
10. Rotation/stale-result correctness is preserved.
11. Focused provider/connection tests and quick verification pass.
12. No medium-or-higher unresolved finding remains.

## 14. Stop conditions

Stop and register a narrower follow-up if:

- representing unverified requires destructive connection-store migration;
- inference outcome attribution cannot be revision-safe without redesigning unrelated agent/session APIs;
- a provider requires a billable request to verify and product policy would need to authorize that spend;
- implementing this milestone requires the not-yet-landed shared EggPool profile crate;
- unrelated provider routing/fallback redesign becomes necessary.

## 15. Closure evidence required

Create `plans/closure/provider-connections/010-status.md` with:

- baseline and implementation commits;
- old `DirectModels` disposition;
- provider-by-provider qualification matrix for touched built-ins;
- public/static/authenticated catalog test evidence;
- inference promotion/auth-failure evidence;
- revision/cancellation/restart review;
- storage/protocol migration evidence;
- TUI redaction/wording evidence;
- verification commands/results;
- unresolved findings and final recommendation;
- unblock audit for M011.

## 16. Handoff notes

Do not "fix" OpenCode Go by weakening authentication handling or by sending a test completion. M010's purpose is to make uncertainty explicit. The model-wire execution defect is intentionally isolated to M011.
