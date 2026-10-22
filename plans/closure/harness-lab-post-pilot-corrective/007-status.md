# Harness Lab Post-Pilot Corrective Milestone 007 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/harness-lab-post-pilot-corrective/007-retire-the-dead-chunk-timeout-config-key.md`
Source subsystem roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`
Repository baseline reviewed: `c237fd372030b5edd53e419f1d4014bd05bbb42e` (`plans/harness-lab-post-pilot-correctives`, merged with `origin/main`; the plan's own stated baseline `ff651895b` is an ancestor of it). A concurrent commit `0634dd229` ("fix(tests): drop the redundant wildcard in the TurnFailed projection arm") landed on the branch while this milestone was in flight; it touches `tests/session_family/projection_consumer.inc` only, has **no** overlap with any file this milestone changed, and every verification in §4 was re-run on top of it and stayed green.
Implementation commits: see §3 — **no commit was made by the implementer**; the change is left in the working tree for the owner's review and commit

## 1. Executive finding

**The key, its merge arm and the provider plumbing that could have carried it are
gone. No provider imposes a per-chunk deadline. A stale key in an existing config
file keeps parsing and changes nothing. The guard that let the key stay dead now
descends into the nested configuration structs.**

**The plan's premise about the provider call sites was false, and the closure
records that rather than reproducing it.** §2.1 of the plan states that "**Every**
call site passes `None`" and enumerates the eight provider call sites. At the
baseline, **two** of them passed a hard-coded 30 s deadline:

- `crates/codegg-providers/src/opencode_zen.rs:99` — `Some(Duration::from_secs(30))`
- `crates/codegg-providers/src/openai_compatible.rs:600` — `Some(Duration::from_secs(30))`

`openai_compatible` is the shared OpenAI-compatible transport, so the deadline
reached every provider that delegates its stream to it: `vertex`, `cloudflare`,
`gitlab`, `copilot`, and the `additional.rs` presets (`groq`, `together`,
`cerebras`, `sambanova`, `fireworks`, `deepseek`, `nvidia`, `sambanova`, …).
`opencode_go` and `openrouter` use `openai_compatible` for **discovery only**,
so they were not affected by that path.

The **key** was still dead exactly as the plan says: nothing ever fed the
configured value into the plumbing, and the value had no source. What was not
dead was the **plumbing** — it imposed a deadline from a hard-coded constant.
That is the same defect C004 had already diagnosed and removed from
`opencode_go` (`plans/closure/harness-lab-post-pilot-corrective/004-evidence.md:232-236`
names `opencode_go`, `openai_compatible` **and** `opencode_zen` as hard-coding a
30 s chunk timeout, and C004 fixed only the first).

Plan §4.2 requires the `tokio::time::timeout` arm to be removed, and plan §7.2
requires that "no provider imposes a per-chunk deadline". Both are satisfied by
removing the plumbing, so the deadline goes with it. This is a **behaviour
change on the affected surfaces** and it is recorded as such in §11.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| §4.1 `ProviderConfig` no longer declares `chunk_timeout` | met | `crates/codegg-config/src/schema.rs`: the field is gone from the struct |
| §4.1 `ProviderConfig::merge` no longer has an arm for it | met | the same file: the `if other.chunk_timeout.is_some()` arm is gone |
| §4.1 The config crate's focused tests pass | met | `cargo test -p codegg-config` — 115 passed, 0 failed |
| §4.1 A test proves a config file containing the key still parses and still produces the same effective configuration | met | `crates/codegg-config/src/paths.rs::tests::retired_chunk_timeout_key_is_inert_when_loaded` — §4.1 |
| §4.2 The `chunk_timeout` parameter is gone from `openai_responses_stream`, `openai_chat_stream` and `shared_stream` | met | `crates/codegg-providers/src/wire.rs` — the parameter and the `tokio::time::timeout` arm are gone |
| §4.2 The `tokio::time::timeout` arm it guarded is removed | met | the same file — `let next = stream.next().await;` |
| §4.2 Every call site in the crate is updated | met | all eight provider call sites plus six `wire.rs` test call sites — §3.2 |
| §4.2 `cargo test -p codegg-providers` passes | met | 276 passed, 0 failed |
| §4.2 The `opencode_go` no-deadline test still passes | met | `opencode_go::chunk_timeout_tests::the_provider_imposes_no_per_chunk_deadline` — §3.3 |
| §4.2 The provider wire boundary guards stay green | met | §4.3 |
| §4.3 The guard covers the nested configuration structs | met | `scripts/check_config_merge_coverage.py` — §3.4 |
| §4.3 The nested structs are enumerated from source, not from a hand-written list | met | the guard derives them by walking the field types reachable from `Config` — §3.4 |
| §4.3 The guard stays future-proof | met | a field added to both a nested struct and its merge passes without editing the guard — §4.2 |
| §4.3 The guard stays sensitive | met | a field added to a nested struct with no merge arm fails — §4.2 |
| §4.3 The existing top-level behaviour is unchanged | met | the original check and its three self-test cases are carried over unchanged — §4.2 |
| §4.3 `--self-test` injects an uncovered **nested** field and proves rejection | met | the `nested field with no merge arm fails` case — §4.2 |
| §4.4 `architecture/config.md` drops the field and says where the bound lives | met | §3.5 |
| §4.4 `codegg.example.jsonc` still validates | met | §4.3 |
| §4.4 No remaining mention of the key in documentation or example configs | met | §4.4 |
| §5 A config file containing the key parses successfully | met | §4.1 |
| §5 The watcher reloads a file containing the key without error | met | `crates/codegg-config/src/watcher.rs::tests::retired_chunk_timeout_key_is_inert_on_watcher_reload` — §4.1 |
| §5 The key's presence does not change the effective configuration on reload | met | the same test |
| §7.1 `ProviderConfig` no longer declares `chunk_timeout`, and its merge has no arm | met | §4.1 of this record's §2 matrix above |
| §7.2 `wire.rs` carries no `chunk_timeout` parameter, and no provider imposes a per-chunk deadline | met | §3.2; the deadline that `opencode_zen` and `openai_compatible` did impose is gone with the parameter |
| §7.3 The test that pins "no per-chunk deadline" still exists and asserts the same thing | met | §3.3 — the module name, the test name and the assertion are unchanged |
| §7.4 A config file containing the key parses and produces the same effective configuration, proven by execution | met | §4.1 |
| §7.5 The guard covers the nested structs, enumerates them from source, and its self-test proves both properties | met | §3.4 and §4.2 |
| §7.6 `architecture/config.md` no longer documents the key and says where the bound lives | met | §3.5 |
| §7.7 `codegg.example.jsonc` still validates | met | §4.3 |
| §7.8 The closure record names the confirmed publication status and the confirmed consumer set | met | §6 |
| §10 No stop condition triggered | held | §11 lists the contradictions found; none of the five §10 stop conditions triggered |

## 3. Production implementation evidence

### 3.1 The schema — the key and its merge arm

`crates/codegg-config/src/schema.rs`:

- `ProviderConfig` no longer declares `pub chunk_timeout: Option<u64>`.
- `ProviderConfig::merge` no longer has the `if other.chunk_timeout.is_some()`
  arm.

No replacement key was added anywhere — not in the config layer, not in the
provider layer, not as an environment variable. `ProviderConfig.timeout` (the
live, different key) is untouched.

### 3.2 The provider plumbing — the parameter and the deadline it carried

`crates/codegg-providers/src/wire.rs`:

- `openai_responses_stream(stream, chunk_timeout, policy)` →
  `openai_responses_stream(stream, policy)`
- `openai_chat_stream(stream, policy, chunk_timeout)` →
  `openai_chat_stream(stream, policy)`
- `shared_stream(stream, adapter, chunk_timeout, policy)` →
  `shared_stream(stream, adapter, policy)`
- the guarded arm is gone:

  ```rust
  let next = if let Some(timeout) = chunk_timeout {
      match tokio::time::timeout(timeout, stream.next()).await { … }
  } else {
      stream.next().await
  };
  ```

  becomes `let next = stream.next().await;`

Every call site in the crate was updated — **eight** provider call sites and
**six** `wire.rs` test call sites:

| File | Before | After |
|---|---|---|
| `openai.rs:181` | `openai_chat_stream(stream, policy, None)` | `openai_chat_stream(stream, policy)` |
| `azure.rs:88` | `openai_chat_stream(stream, policy, None)` | `openai_chat_stream(stream, policy)` |
| `openrouter.rs:97` | `openai_chat_stream(stream, policy, None)` | `openai_chat_stream(stream, policy)` |
| `opencode_zen.rs:96` | `openai_chat_stream(stream, policy, Some(30 s))` | `openai_chat_stream(stream, policy)` |
| `openai_compatible.rs:597` | `openai_chat_stream(stream, policy, Some(30 s))` | `openai_chat_stream(stream, policy)` |
| `google.rs:97` | `shared_stream(stream, adapter, None, policy)` | `shared_stream(stream, adapter, policy)` |
| `anthropic.rs:110` | `shared_stream(stream, adapter, None, policy)` | `shared_stream(stream, adapter, policy)` |
| `opencode_go.rs:392` | `shared_stream(bytes, adapter, CHUNK_TIMEOUT, policy)` | `shared_stream(bytes, adapter, policy)` |

The request path is unchanged for every provider: the encoding, the endpoint,
the headers and the credentials are untouched. The only difference is that a
slow-but-valid stream is no longer cut off at 30 s by the provider.

### 3.3 The retained no-deadline test

`crates/codegg-providers/src/opencode_go.rs` keeps the module
`chunk_timeout_tests`, the test `the_provider_imposes_no_per_chunk_deadline`
and **the assertion verbatim**:

```rust
assert!(
    CHUNK_TIMEOUT.is_none(),
    "a provider chunk deadline below the agent layer's idle timeout \
     preempts the agent's own liveness policy"
);
```

The constant `CHUNK_TIMEOUT` is now `#[cfg(test)]`, because the shared stream
bridge no longer carries a deadline parameter and the constant therefore has no
production reader. Its doc comment records why: the provider cannot impose a
per-chunk deadline whatever it passes. The assertion itself is unchanged, and
the module and test names are unchanged.

### 3.4 The guard extension — `scripts/check_config_merge_coverage.py`

The guard already derived the `Config` field set from the struct and the merged
field set from `merge_configs`, and required them to be equal. It now also
descends into the nested configuration structs.

**The nested structs are enumerated from the source, not from a list.** The
guard scans every Rust source in `crates/codegg-config/src/`, derives every
`pub struct` and its fields, and then walks the **field types** reachable from
`Config` — `Option<HashMap<String, ProviderConfig>>` reaches `ProviderConfig`,
`Option<EggworkRemoteRetryPolicy>` reaches `EggworkRemoteRetryPolicy`, and so
on. A new nested configuration struct is therefore covered automatically,
without this file naming it.

**The reachability rule, and why it is what it is.**

- A struct that is **not reachable** from `Config` is not required to have
  merge arms. It is not part of the parsed configuration surface, so requiring
  arms of it would fail on unrelated code. (Seven structs in the crate are
  unreachable: `ConfigWatcher`, `DocumentParseError`, `FormatterRule`,
  `LspRestartPolicyConfig`, `ManagedMasterKey`, `ResolvedRemoteRetryPolicy`,
  `ScratchRetentionSettings`. None of them owns a `merge` method, so none of
  them is affected either way.)
- A struct that **is reachable but owns no `merge` method** is not required to
  have merge arms, and this is a deliberate decision. Such a struct is never
  merged field by field: its whole value is replaced by its parent's arm (for
  example `ProviderConfig.models` is replaced wholesale, so `ModelConfig`'s own
  fields are never merged). There are no arms to compare it against, and
  requiring some would be nonsense. Sixty of the seventy-seven reachable
  structs are in this category. The rule therefore requires coverage only of
  **reachable structs that own their own `merge` method** — which is exactly
  the set that can silently drop a field.
- The merged-parameter name is read from the **signature**, not assumed to be
  `other`, so renaming the merged-in value does not silently blind the guard.

**The measured set.** There are **seventeen** structs in the crate that own a
`merge` method, and **all seventeen are reachable from `Config`**:

`ApprovalReviewerConfig`, `CommandIntentConfig`, `DaemonConfig`,
`DiscoveryConfig`, `EggworkConfig`, `EggworkNodeProfile`, `HumanShellConfig`,
`PreflightConfig`, `ProviderConfig`, `RemoteResourceLimits`, `ResearchConfig`,
`SchedulerConfig`, `ScratchConfig`, `ServerConfig`, `ShellConfig`,
`ToolBackendConfigSchema`, `ToolDeferralConfig`.

(`LspRestartPolicyConfig` has a `merge_with_profile` method, not a `merge`
method, so it is not one of the seventeen.) The plan and the addendum both said
**sixteen**; the measured number is **seventeen**. All seventeen were fully
covered before this change, so the extended guard passes on the real tree.

**The guard reads like the file it replaces.** Same module docstring
discipline, same `_strip_comments` / `_braced_block` helpers, same
`unmerged_fields` top-level check carried over unchanged, same
`--verbose` output shape, same exit-code contract (0 if all checks pass, 1 if
any fail), and one new named check added to the existing `checks` list:

```
  [PASS] Config + paths modules exist
  [PASS] every Config field has a merge_configs arm
  [PASS] every reachable nested config struct field has a merge arm
```

### 3.5 Documentation

`architecture/config.md`:

- the `ProviderConfig` listing no longer contains the field;
- a new paragraph states that **there is no per-provider chunk deadline and
  none is configurable**, that per-chunk stream deadlines are owned by the
  agent layer (`STREAM_SETUP_TIMEOUT` of 120 s, `STREAM_IDLE_TIMEOUT` of 90 s),
  and that a provider-level deadline below the agent's own idle timeout fires
  first and reports a slow but valid stream as a provider error.

The paragraph deliberately **does not name the retired key**: §4.4's acceptance
evidence requires a grep of the documentation to find no remaining mention of
it, and the closure record is where the key is named.

`docs/configuration.md` lists `provider.<id>.timeout` but never listed the
retired key, so it is unchanged. `codegg.example.jsonc` never mentioned the key
and is unchanged.

## 4. Verification executed

| Command | Result |
|---|---|
| `cargo test -p codegg-config` | **115 passed, 0 failed** — local |
| `cargo test -p codegg-providers` | **276 passed, 0 failed** — local |
| `cargo test -p codegg-providers chunk_timeout` | **1 passed** — `opencode_go::chunk_timeout_tests::the_provider_imposes_no_per_chunk_deadline` |
| `cargo fmt --all -- --check` | **clean** — local |
| `cargo clippy -p codegg-config -p codegg-providers --all-targets --locked -- -D warnings` | **passed**, no warnings — local |
| `python3 scripts/check_config_merge_coverage.py --self-test` | **all cases passed** — §4.2 |
| `python3 scripts/check_config_merge_coverage.py` | **3/3 checks passed** — local |
| `python3 scripts/check_provider_wire_boundary.py` | **passed** (exit 0) |
| `python3 scripts/check_provider_multi_surface_dispatch.py` | **6/6 checks passed** |
| `python3 scripts/check_provider_wire_cutover.py` | **passed** (exit 0) |
| `cargo run --bin codegg -- validate --config codegg.example.jsonc` | **passed** — §4.3 |

`scripts/verify.sh quick` and the workspace-wide clippy were **not** run by the
implementer, at the owner's instruction (they are the owner's own runs and the
sweep would contend with them).

### 4.1 The failing-first evidence — the migration tests

The two new tests were written and run **before** the key was removed. Both
failed, and they failed for the right reason: the key was carried into the
schema, so the two configurations were not equal.

```
running 2 tests
test paths::tests::retired_chunk_timeout_key_is_inert_when_loaded ... FAILED
test watcher::tests::retired_chunk_timeout_key_is_inert_on_watcher_reload ... FAILED

---- paths::tests::retired_chunk_timeout_key_is_inert_when_loaded stdout ----

thread 'paths::tests::retired_chunk_timeout_key_is_inert_when_loaded' panicked at crates/codegg-config/src/paths.rs:1307:9:
assertion `left == right` failed: the retired `chunk_timeout` key must be ignored, not carried into the schema
  left: Config { … provider: Some({"retirement_demo": ProviderConfig { …, timeout: None, chunk_timeout: Some(30), whitelist: None, … } }), … }
 right: Config { … provider: Some({"retirement_demo": ProviderConfig { …, timeout: None, chunk_timeout: None, whitelist: None, … } }), … }

---- watcher::tests::retired_chunk_timeout_key_is_inert_on_watcher_reload stdout ----

thread 'watcher::tests::retired_chunk_timeout_key_is_inert_on_watcher_reload' panicked at crates/codegg-config/src/watcher.rs:276:9:
the retired `chunk_timeout` key must not change the effective configuration on reload

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 113 filtered out; finished in 0.01s
```

(The two `Config` dumps are abbreviated above; the run prints them in full. The
paths test dumps the whole configuration because it compares a single, known
layer, so the dump carries nothing but the test's own temporary provider
entry. The watcher test asserts with `assert!` and a message rather than with
`assert_eq!`, because it compares the configuration the **host** resolves —
which carries the host's own global layer — and a failed comparison must not
print it. No credential appears in either output.)

With the key removed, both pass:

```
running 2 tests
test paths::tests::retired_chunk_timeout_key_is_inert_when_loaded ... ok
test watcher::tests::retired_chunk_timeout_key_is_inert_on_watcher_reload ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 113 filtered out; finished in 0.02s
```

### 4.2 The failing-first evidence — the guard extension

The extension was proven against the guard it replaces. A copy of the real
config crate was made with **one** uncovered nested field injected into
`ProviderConfig` (`injected_dead_key: Option<u64>`, with no merge arm), and
both guards were run against it.

**Before** — the guard at `HEAD` passes, which is the hole:

```
===== BEFORE (old guard, HEAD) =====

  [PASS] Config + paths modules exist
  [PASS] every Config field has a merge_configs arm

2/2 checks passed.
All config merge coverage invariants verified.
exit=0
```

**After** — the extended guard rejects it, and names the field:

```
===== AFTER (extended guard) =====
  FAIL: 1 nested config field(s) have no merge arm:
        - ProviderConfig.injected_dead_key
  A nested struct that owns its `merge` method drops an unarmed field for EVERY layer, so the setting parses and then silently does nothing.

  [PASS] Config + paths modules exist
  [PASS] every Config field has a merge_configs arm
  [FAIL] every reachable nested config struct field has a merge arm

2/3 checks passed.
1 check(s) FAILED.
exit=1
```

The extended guard's own `--self-test` (which injects the same shape into a
synthetic tree) is:

```
  [PASS] self-test: field covered by merge_option! passes
  [PASS] self-test: field on Config with no merge arm fails
  [PASS] self-test: field covered by a later field passes
  [PASS] self-test: Config with no extra field passes
  [PASS] self-test: nested field covered by its merge arm passes
  [PASS] self-test: nested field with no merge arm fails
  [PASS] self-test: nested struct with no extra field passes
  [PASS] self-test: real tree is fully covered
  [PASS] self-test: real tree nested structs are fully covered
All guard self-test cases passed.
exit=0
```

The first four cases are the original guard's cases, carried over unchanged.
The next three are the nested cases: a nested field with an arm passes
(**future-proof**), the same field without an arm fails (**sensitive**), and a
nested struct with no extra field passes.

### 4.3 The example configuration

```
$ cargo run --bin codegg -- validate --config codegg.example.jsonc
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12m 53s
     Running `target/debug/codegg validate --config codegg.example.jsonc`
Configuration is valid: codegg.example.jsonc
exit=0
```

### 4.4 The grep — no remaining mention of the key in documentation or example configs

```
$ grep -rn -i "chunk_timeout" architecture/ docs/ .opencode/ README.md codegg.example.jsonc
(no matches)
```

The key survives in exactly two kinds of place, both deliberate:

- the **test sources** that prove the migration (`crates/codegg-config/src/paths.rs`,
  `crates/codegg-config/src/watcher.rs`) and the retained no-deadline test
  module (`crates/codegg-providers/src/opencode_go.rs`);
- the **records** that document the removal: this closure record, the plan, the
  subsystem addendum and the registry.

## 5. Invariant review

| Invariant | Status |
|---|---|
| No provider imposes a per-chunk deadline, before or after this change | **held after** — the deadline is gone with the plumbing. It was **not** held before: `opencode_zen` and `openai_compatible` imposed a 30 s one. §1 and §11 |
| The test that pins "no per-chunk deadline" survives with its assertion intact | held — the module name, the test name and the assertion text are unchanged — §3.3 |
| No replacement knob is added — not in the provider layer, not in the config layer, not as an environment variable | held — the key and the parameter are both removed and nothing replaces them |
| `ProviderConfig.timeout` (the live, different key) is untouched | held |
| No provider's request path changed | held — encoding, endpoint, headers and credentials are untouched; the only change is the removal of the stream-read deadline on two surfaces — §11 |
| No change to the shared eggpool wire map | held — the shared kernel's contract lives in the sibling `gorouter` repository and was not touched; `eggpool-wire` stays pinned at `9ac0a1318e8db3c034b5ab54987317752d5ffea6` |
| No change to the provider wire boundary's ownership | held — `eggpool-wire` remains the codec authority; a CodeGG-local parameter was removed and nothing was re-implemented or relocated |
| An existing config file that still contains the key keeps working | held — proven by execution — §4.1 |
| The guard extension is future-proof and sensitive, and proves both by self-test | held — §4.2 |
| No candidate code is merged or pushed | held — the change is left in the working tree; no commit, no push, no branch was created |

## 6. Publication status and consumer set of `codegg-providers` (plan §7.8)

**Confirmed at the baseline `c237fd372`:**

1. **The crate is unpublished.**
   - `https://crates.io/api/v1/crates/codegg-providers` →
     `{"errors":[{"detail":"crate `codegg-providers` does not exist"}]}`.
     The root package `codegg` is likewise absent from crates.io, so nothing in
     this repository is published to a registry.
   - There is **no publish automation** anywhere in the repository: no
     `cargo publish`, `cargo-release`, `release-plz` or crates.io reference in
     any workflow, script or manifest. The only `publish = false` entries are
     in `research/` and in `crates/codegg-core/fuzz/`, neither of which is a
     workspace member.
   - `crates/codegg-providers/Cargo.toml` declares no `publish` key and the
     workspace pins it at `version = "=0.1.0"` — **pre-1.0**, and a workspace
     path dependency (`Cargo.toml:30`).
2. **There is no out-of-repository consumer of `codegg_providers::wire::*`.**
   - `grep -rn "codegg_providers::wire" --include="*.rs" .` → **no matches
     outside the crate itself**. The crate's own `wire` module is used through
     `crate::wire::…` internally.
   - The crate's only dependents are **inside this repository**: the root
     `codegg` package and `crates/codegg-core`, neither of which references
     `codegg_providers::wire::*`.
   - Because the crate is absent from crates.io, there is no registry consumer
     and no reverse dependency.
3. **Consequence.** The `wire.rs` signature change is a source-level change to
   an internal, pre-1.0 workspace crate with no out-of-repository consumer. It
   is **not** a semver-major event. No stop condition under §10 was triggered
   by this fact.

## 7. Failure and recovery review

- The retired key is **inert, not an error**: `ProviderConfig` derives
  `Deserialize` with `#[serde(default)]` and there is **no**
  `deny_unknown_fields` anywhere in `crates/codegg-config` (verified by grep),
  so an unknown key is ignored. This is proven by execution, not asserted from
  the attributes — §4.1.
- The key is inert on **every** load, not only the first: the configuration
  watcher's reload path is proven separately — §4.1.
- A configuration file that contains the key produces **the same effective
  configuration** as one that does not, so no configuration that worked before
  stops working, and none that did not work starts working.
- The guard fails **closed**: an uncovered nested field is a non-zero exit, so
  the hole cannot reopen silently.

## 8. Migration and compatibility review

- **No storage migration.** `chunk_timeout` is not a column in the
  provider-connection store and appears in no migration; it existed only in the
  configuration schema.
- An existing configuration file that still contains the key parses
  successfully and produces the same effective configuration — proven by
  execution.
- The `wire.rs` signature change affects only the crate's internal callers,
  all of which are updated in the same change.

## 9. Security review

- The change **removes** a public configuration surface; it adds none.
- No credential, token, prompt or raw provider body appears in any file this
  milestone wrote. The failing-first dump of §4.1 contained the host's own
  global configuration layer — including a real API key — and is reproduced
  here only in the abbreviated form that omits it. The watcher test now uses
  `assert!` with a message rather than `assert_eq!` precisely so that a future
  failure cannot print the host's configuration.
- No new environment variable was read or written by production code. The test
  helper sets `CODEGG_TUI_CONFIG` only inside the test process and restores
  the previous value on drop.
- The crate already had a process-wide environment lock, private to
  `encryption.rs`'s test module. The new watcher test needs the same
  protection — the environment is process-global, so two tests that mutate it
  must not overlap even when they set different variables — so the lock was
  hoisted to `crates/codegg-config/src/lib.rs` as a `#[cfg(test)]`
  `pub(crate) static ENV_LOCK` and both test modules now take it. The change
  is test-only: no production code path is touched, and the behaviour of every
  existing test is unchanged.

## 10. Documentation and operations

- `architecture/config.md` — the field is dropped from the `ProviderConfig`
  listing, and the module now states that there is no per-provider chunk
  deadline, that none is configurable, and where the bound actually lives.
- `architecture/provider.md` — **edited**, because the plan's §9 statement that
  it "requires no change" was itself wrong against the reconciled base. See
  §11.2: the file claimed that provider modules own "chunk-idle deadlines"
  after this change removed the last one, and it carried a "30-second chunk
  timeout" sentence for the OpenAI-compatible transport that no longer
  described the code. Its record that `opencode_go` imposes no per-chunk
  deadline remains accurate and was not weakened.
- `docs/configuration.md` — **unchanged**: it never listed the retired key.
- `codegg.example.jsonc` — **unchanged**: it never mentioned the retired key,
  and it still validates.
- `scripts/check_config_merge_coverage.py` — extended per §4.3, with its
  self-test extended, and its module docstring extended to describe the nested
  coverage.

## 11. Contradictions with the plan, and unresolved findings

### 11.1 Contradiction — the plan's call-site premise was false

Plan §2.1 says "**Every** call site passes `None`" and lists the eight provider
call sites, including `opencode_zen.rs:96` and `openai_compatible.rs:597`. At
the baseline both passed `Some(Duration::from_secs(30))`. This was true at the
plan's own stated baseline `ff651895b`, so it is a wrong premise rather than
drift:

```
$ git show ff651895b:crates/codegg-providers/src/opencode_zen.rs | grep -A 8 "openai_chat_stream"
        Ok(crate::wire::openai_chat_stream(
            stream,
            req.context.wire_policy.clone(),
            Some(std::time::Duration::from_secs(30)),
        ))
```

Plan §3's invariant "**No provider imposes a per-chunk deadline, before or
after this change**" was therefore false before the change, and plan §3's
invariant "**No provider behaviour change**" cannot hold for the two surfaces
that did impose one. The plan's §7.2 acceptance criterion ("no provider
imposes a per-chunk deadline") and §4.2 ("remove the `tokio::time::timeout`
arm") are only satisfiable by removing those deadlines, which is what was done.

**Resolution.** The removal was carried out as §7.2 requires. The affected
surfaces are named here rather than left implicit, because the change is
observable: a stream that was previously cut off with `provider error: stream
error: stream chunk timeout` after 30 s of silence now continues until the
agent layer's own `STREAM_IDLE_TIMEOUT` (90 s) applies. This is the same
correction C004 already applied to `opencode_go`, and the same reasoning
applies — a provider deadline below the agent's idle timeout preempts the
agent's bound and reports a slow but valid stream as a provider error.

**Severity: medium.** The behaviour change is deliberate, mandated by §7.2, and
aligned with the agent layer's ownership of stream liveness; it was not
verified by a live provider run, because no live provider run is in scope for
this milestone and none was performed.

### 11.2 The plan said `architecture/provider.md` needed no change; it did

Plan §9 says "`architecture/provider.md` — **no change is required**". Against
the reconciled base that is false, for two separate reasons, and the file was
therefore edited in this milestone's commit:

1. **A sentence the merge had already invalidated.** `main`'s wire cutover
   (`dcea3ea72`) documented a "30-second chunk timeout" for the
   OpenAI-compatible transport. The branch's own base predated that cutover,
   so the branch's `architecture/provider.md` never carried the sentence, and
   the merge left the branch's text in place while keeping `main`'s code — a
   documentation/code disagreement that existed **before** this milestone.
   Removing the deadline is what makes the branch's text true again.
2. **A clause that contradicts the change.** `architecture/provider.md:1055`
   said that provider modules own "discovery, HTTP status handling, **chunk-idle
   deadlines**, and cancellation". After this change no provider module owns a
   chunk-idle deadline: the agent layer does, and the file's own §"Stream
   liveness" paragraph (added by C004) says so. The clause is now: provider
   modules own endpoint construction, credentials, headers, discovery, HTTP
   status handling and cancellation, and they **do not** own stream liveness.

**Severity: low**, and now closed rather than recorded as residual. The
authoritative statement of the ownership boundary — `architecture/provider.md`
§"Stream liveness" and the new `architecture/config.md` paragraph — was already
correct; this removes the one clause that contradicted it.

### 11.3 The plan's nested-struct count was sixteen; the measured count is seventeen

Plan §2.3 and the addendum's F009 both say **sixteen** nested configuration
structs own their own `merge`. The measured number is **seventeen**, all
reachable from `Config`. The discrepancy is one struct (`RemoteResourceLimits`
or `EggworkNodeProfile`, both of which are reachable only through a nested
`HashMap` value type). The guard does not depend on the count, because it
enumerates from source; the count in the addendum has been corrected.

**Severity: low.** A count, not a behaviour.

### 11.4 No §10 stop condition triggered

- *Removing the key would require adding a replacement knob* — **not
  triggered**: nothing was added.
- *The no-deadline assertion cannot be preserved* — **not triggered**: it is
  preserved verbatim.
- *`codegg-providers` is published or has an out-of-repository consumer of
  `codegg_providers::wire::*`* — **not triggered**: §6 confirms both are
  false.
- *The guard extension cannot enumerate nested config structs from source
  without a hand-written list that would go stale* — **not triggered**: the
  enumeration is a walk of the field types reachable from `Config` — §3.4.
- *The change would touch the shared eggpool wire map* — **not triggered**: the
  shared contract lives in the sibling `gorouter` repository and was not
  touched.

## 12. Unblock audit

- **C004's third named successor condition is discharged.** The registry's C004
  row names three remaining conditions — C005, C006 and C007. With C007
  closed, C004's remaining conditions are **C005 and C006**.
- The addendum's finding **F009** is closed and now records what the
  implementation found — §1 and §11.1.
- **Nothing else is unblocked.** `C007` appears in exactly three places in
  `plans/registry.md` (the subsystem row, the C007 milestone row and the C004
  row); no other registered plan lists C007 as a dependency, and every other
  `C007` in `plans/` belongs to a different subsystem's own milestone
  numbering (`linux-execution-isolation`, `tui-subagent-and-job-visibility`,
  `async-background-jobs`, `agent-tool-use-operability`, …).
- `plans/registry.md` was **not** edited by the implementer. The owner's
  exactly-one status change is the C007 milestone row (`ready` → `closed`, with
  the closure record path); the subsystem row and the C004 row name C007 as a
  remaining condition and will need the corresponding wording update.

## 13. Registry updates

- `plans/registry.md`: **not edited** — owned by the operator. The C007
  milestone row moves from `ready` to `closed` with the closure record path
  `plans/closure/harness-lab-post-pilot-corrective/007-status.md`; the
  subsystem row's "three registered successors" becomes "two"; and the C004
  row's third remaining condition (the dead `ProviderConfig.chunk_timeout`
  key) is discharged.
- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`: **edited**
  — the `Status:` line, the F009 finding and the C007 milestone section, and
  the C007 row of the §8 milestone status table, following the pattern C005 and
  C006 established.
