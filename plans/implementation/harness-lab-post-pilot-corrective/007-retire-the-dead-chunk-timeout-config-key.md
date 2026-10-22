# Harness Lab Post-Pilot C007 — Retire the Dead `ProviderConfig.chunk_timeout` Config Key

Status: ready for handoff
Repository baseline: `plans/harness-lab-post-pilot-correctives@ff651895b` (2026-10-22; inspect latest at handoff).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`
Corrects C004 `plans/closure/harness-lab-post-pilot-corrective/004-status.md` §5 condition 5; preserves the closed C001–C003 records.
Long-term requirements: `plans/000-long-term-specification.md` §§24, 26–29; `plans/001-terminology-and-domain-model.md`.
Primary class: polish.

## 1. Objective

Remove a public configuration key that **no code reads**, together with the
plumbing that existed only to carry it, and close the static-guard hole that let
it stay dead and undocumented-as-dead.

`ProviderConfig.chunk_timeout` is declared in the configuration schema,
documented in `architecture/config.md`, merged across configuration layers, and
read by nothing. An operator who sets it gets no error, no warning, and no
effect. That is the worst of the three outcomes: a knob that looks live and is
not.

This milestone does not add a replacement knob. If the operator wants a
configurable stream deadline, that is a different decision — it belongs to the
layer that owns stream liveness, and it gets its own plan.

## 2. Evidence and readiness

### 2.1 The key is dead

- `crates/codegg-config/src/schema.rs:1432` declares
  `pub chunk_timeout: Option<u64>` on `ProviderConfig`.
- `crates/codegg-config/src/schema.rs:1484-1486` gives it a merge arm, so it
  participates in layer merging like every other field.
- `architecture/config.md:253` documents it as part of the provider config
  surface.
- **No provider reads it.** The only shape in which it could have been read is
  the `chunk_timeout: Option<Duration>` parameter threaded through
  `crates/codegg-providers/src/wire.rs:285,296,346,356,365`, where
  `shared_stream` applies `tokio::time::timeout(timeout, stream.next())` when
  the option is `Some` (`wire.rs:395-398`). **Every** call site passes `None`:
  the eight provider call sites (`google.rs:97`, `opencode_zen.rs:96`,
  `opencode_go.rs:392`, `openrouter.rs:97`, `openai_compatible.rs:597`,
  `azure.rs:88`, `anthropic.rs:110`, `openai.rs:181`) and every test in
  `wire.rs`.
- C004 recorded the fact directly: "`ProviderConfig.chunk_timeout` is documented
  and read by **no** provider, so the knob is dead"
  (`plans/closure/harness-lab-post-pilot-corrective/004-evidence.md:235-236`).
  It was consistent rather than contradictory at that point — no provider
  imposes a per-chunk deadline, because the agent layer owns the bound — but it
  was still a dead key, and removing a public config field is a schema change
  that C004 did not make.

### 2.2 The key was never live

The knob has never affected behaviour. `opencode_go` used to hard-code its own
30 s `CHUNK_TIMEOUT`; that constant was removed in C004 because a provider
deadline below the agent's own idle timeout **preempts** the agent's bound
(`STREAM_SETUP_TIMEOUT` 120 s, `STREAM_IDLE_TIMEOUT` 90 s) and reports a slow
but valid stream as a provider error. Nothing ever wired the configuration
value into that position. Removing the key therefore removes a setting that
could not have been in effect.

### 2.3 The guard hole that let it stay dead

`scripts/check_config_merge_coverage.py` derives the field set from
`pub struct Config` and the merged field set from `merge_configs`, and
requires the two to be equal. Its own docstring states the failure it exists to
prevent: "A `Config` field with no arm is discarded for every config layer, not
just for multi-layer loads. The setting then parses without error and silently
does nothing."

The guard covers only the **top-level** `Config` struct. It does not descend
into nested configuration structs that own their own `merge` method, and
`crates/codegg-config/src/schema.rs` contains **sixteen** of them
(`RemoteResourceLimits`, `EggworkConfig`, `EggworkNodeProfile`, `ServerConfig`,
`ProviderConfig`, and eleven `merge(&mut self, other: &Self)` owners).

`ProviderConfig::merge` is on the live path: `merge_configs` calls it per key
at `crates/codegg-config/src/paths.rs:299-312`
(`existing.merge(v)`), and `merge_configs` is the only path from a parsed
config file to `Config` (`Config::load` and `ConfigWatcher::reload_config` both
call it).

So a `ProviderConfig` field with no merge arm would parse without error and be
silently discarded for every layer — the exact failure the guard was written
for, unguarded. The compiler catches the reverse mistake (an arm referencing a
removed field does not compile), but the dangerous direction — keeping a field
and dropping its arm — is invisible today.

## 3. Invariants and non-goals

- **No provider imposes a per-chunk deadline, before or after this change.**
  The agent layer owns stream liveness. The test that pins this —
  `opencode_go::chunk_timeout_tests::the_provider_imposes_no_per_chunk_deadline`
  — must survive, and its assertion must not be weakened. If the test is
  renamed, the rename must not lose the assertion.
- **No replacement knob is added.** Not in the provider layer, not in the
  config layer, not as an environment variable.
- **`ProviderConfig.timeout` is a different, live key and is not touched.**
- **No provider behaviour change.** Every provider's request path must be
  byte-for-byte equivalent in behaviour after the change.
- **No change to the shared eggpool wire map.** That contract lives in the
  gorouter repository and is not CodeGG's to edit.
- **No change to the provider wire boundary's ownership.** `eggpool-wire`
  remains the codec authority; this milestone removes a CodeGG-local
  parameter, and does not re-implement or relocate anything.
- **An existing config file that still contains `chunk_timeout` must keep
  working.** The key becomes inert; it must not become an error.
- The guard extension must be **future-proof** (a covered field passes without
  editing the guard) and **sensitive** (an uncovered field fails), and must
  prove both by self-test, as the existing guard does.

## 4. Work packages

### 4.1 Remove the key and its merge arm

*Intent:* the schema no longer advertises a setting that cannot work.

*Changes:* delete `pub chunk_timeout: Option<u64>` from `ProviderConfig` and
delete its arm from `ProviderConfig::merge`. Delete the field from the
`ProviderConfig` listing in `architecture/config.md`.

*Acceptance evidence:* the config crate's focused tests pass; the guard in
§4.3 passes; and a test proves that a config file containing the key still
parses and still produces the same effective configuration as one without it.

### 4.2 Remove the dead plumbing

*Intent:* the parameter that existed only to carry the key must not survive as
a second dead knob.

*Changes:* after §4.1, the `chunk_timeout: Option<Duration>` parameter of
`openai_responses_stream`, `openai_chat_stream` and `shared_stream`
(`wire.rs:285,346,365`) has no possible source — every call site passes
`None`, and no caller can pass `Some` any more. Remove the parameter and the
`tokio::time::timeout` arm it guards (`wire.rs:395-398`).

This is a **local** change: `codegg-providers` is an internal workspace member
(`version = "=0.1.0"`, pre-1.0), and this repository contains no caller of
`codegg_providers::wire::*` outside the crate itself. The implementer must
confirm both facts at the baseline rather than rely on this plan's statement of
them, and must record the confirmation; if the crate is published or has an
out-of-repository consumer, the signature change is a semver-major event and
the milestone must stop and report instead.

*Acceptance evidence:* `cargo test -p codegg-providers` passes; the
`opencode_go` no-deadline test still passes; the provider wire boundary guards
(`scripts/check_provider_wire_boundary.py`,
`scripts/check_provider_multi_surface_dispatch.py`,
`scripts/check_provider_wire_cutover.py`) stay green.

### 4.3 Extend the merge-coverage guard to nested config structs

*Intent:* close the hole that let a dead key survive in a nested struct.

*Changes:* `scripts/check_config_merge_coverage.py` must additionally derive,
for every configuration struct that owns its own `merge` method and is
reachable from `Config`, the struct's field set and its merge arm set, and
require them to be equal. The guard must:

- stay **future-proof** — a field added to both the struct and its merge must
  pass without editing the guard;
- stay **sensitive** — a field added to the struct with no merge arm must
  fail;
- keep its current top-level behaviour unchanged;
- carry a `--self-test` that injects an uncovered **nested** field and proves
  the guard rejects it, alongside the existing self-test cases.

The guard must enumerate the nested structs from the source rather than from a
hand-written list, so a new nested config struct is covered automatically.

*Acceptance evidence:* the extended guard's `--self-test` passes; the guard
fails when an uncovered nested field is injected and passes when it is covered;
`scripts/verify.sh quick` stays green.

### 4.4 Documentation

*Intent:* the documented surface must match the schema.

*Changes:* `architecture/config.md` drops the field from the
`ProviderConfig` listing and states, in one sentence, that per-chunk stream
deadlines are owned by the agent layer and are not configurable per provider —
so a reader who was told this knob existed learns where the bound actually
lives. `codegg.example.jsonc` does not mention the key and must continue to
validate: `codegg validate --config codegg.example.jsonc` must pass.

*Acceptance evidence:* the validation command passes; a grep of the
repository's documentation and example configs finds no remaining mention of
the key outside the closure records that record its removal.

## 5. Failure, restart and contention semantics

Not applicable in the usual sense: this milestone changes a configuration
schema, not a runtime path. Two rules still apply.

- A configuration file that contains the key must parse successfully. If the
  implementation makes an unknown key an error, the milestone has failed its
  migration requirement, not merely broken a user.
- The configuration watcher must reload a file containing the key without
  error, and without the key's presence changing the effective configuration.
  The key is inert, not merely ignored on the first load.

## 6. Required tests and verification

- `cargo test -p codegg-config` — the config crate's focused suite.
- `cargo test -p codegg-providers` — including
  `opencode_go::chunk_timeout_tests::the_provider_imposes_no_per_chunk_deadline`,
  which must still pass and must still assert the same thing.
- `cargo fmt --all -- --check`;
  `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `python3 scripts/check_config_merge_coverage.py --self-test` — the extended
  guard's self-test.
- `python3 scripts/check_provider_wire_boundary.py` and the sibling provider
  wire guards.
- `scripts/verify.sh quick` passes.
- `codegg validate --config codegg.example.jsonc` passes.

## 7. Acceptance criteria

1. `ProviderConfig` no longer declares `chunk_timeout`, and `ProviderConfig::merge`
   no longer has an arm for it.
2. `crates/codegg-providers/src/wire.rs` no longer carries a `chunk_timeout`
   parameter, and no provider imposes a per-chunk deadline.
3. The test that pins "no per-chunk deadline" still exists and still asserts
   the same thing.
4. A configuration file that contains `chunk_timeout` parses successfully and
   produces the same effective configuration as one that does not. Proven by
   execution, not by assertion.
5. `scripts/check_config_merge_coverage.py` covers the nested configuration
   structs, enumerates them from source, and its `--self-test` proves the
   coverage is both future-proof and sensitive.
6. `architecture/config.md` no longer documents the key and says where the
   stream-liveness bound actually lives.
7. `codegg.example.jsonc` still validates.
8. The closure record names the confirmed publication status of
   `codegg-providers` and the confirmed absence of out-of-repository consumers
   of `codegg_providers::wire::*`.

## 8. Compatibility and migration

`ProviderConfig` derives `Deserialize` with `#[serde(default)]` and **no**
`deny_unknown_fields` (`crates/codegg-config/src/schema.rs:1422-1443`), and
`deny_unknown_fields` is used nowhere in `crates/codegg-config`. An existing
configuration file that still contains `chunk_timeout` therefore parses
successfully and the key is ignored. Because the key never had an effect, no
configuration that worked before stops working, and no configuration that
did not work starts working.

This must be **verified by loading such a file**, in the focused test, not
asserted from the serde attributes alone.

There is **no storage migration**. `chunk_timeout` is not a column in the
provider-connection store and appears in no migration; it exists only in the
configuration schema.

The `wire.rs` signature change is a source-level change to an internal,
pre-1.0 workspace crate with no out-of-repository consumer in this repository.
It is recorded as such, and §7.8 requires the implementer to confirm it at the
baseline.

## 9. Documentation and static guards

- `architecture/config.md` — drop the key; state that the agent layer owns
  stream liveness and that no per-provider chunk deadline exists or is
  configurable.
- `architecture/provider.md` — no change is required; it already records that
  `opencode_go` imposes no per-chunk deadline. The implementer must check
  that this remains accurate and must not weaken it.
- `scripts/check_config_merge_coverage.py` — extended per §4.3, with its
  self-test extended.
- `docs/configuration.md` — if it lists provider config keys, drop the key
  there too.
- `plans/closure/harness-lab-post-pilot-corrective/004-status.md` is updated
  with the outcome and the unblock audit.
- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md` §8 and
  `plans/registry.md` are updated in the same commit.

## 10. Handoff and closure evidence

The closure record `plans/closure/harness-lab-post-pilot-corrective/007-status.md`
must carry: the requirement-to-evidence matrix; the exact SHA; the tests run
with their results; proof that a config file containing the key still loads;
the extended guard's self-test output; the confirmed publication status and
consumer set of `codegg-providers`; the retained no-deadline test; and an
audit of the registry's blocked work for anything this unblocks.

**Stop conditions.** Stop and report rather than improvise if: removing the
key would require adding a replacement knob anywhere; the no-deadline
assertion cannot be preserved; `codegg-providers` turns out to be published or
to have an out-of-repository consumer of `codegg_providers::wire::*`, which
would make the signature change a semver-major event; the guard extension
cannot enumerate nested config structs from source without a hand-written list
that would go stale; or the change would touch the shared eggpool wire map.
