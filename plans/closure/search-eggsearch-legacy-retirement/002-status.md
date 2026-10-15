# Search / Eggsearch Legacy Retirement Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/search-eggsearch-legacy-retirement/002-builtin-search-backend-retirement.md`

Source subsystem roadmap:

- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md#M002--builtin-external-search-backend-retirement`

Repository baseline reviewed: `113b1f8a3a13c1ab78690352767de940528a162c`

Implementation commits:

- `97693884748184d0decdbe20944b666545011511` — removed the in-tree external-search provider and builtin webfetch implementations; restricted dispatch/configuration to eggsearch or disabled.
- `64560ae97c31d3bbf9725c6a887641b819cf6121` — removed the final stale builtin-mode recovery guidance and clarified wrapper registration comments.

## 1. Executive finding

M002 is complete. Eggsearch is CodeGG's sole external web/repository/security/research search and fetch provider execution owner. CodeGG retains stable native wrappers, their schemas, output caps, structured results, and Eggsearch provenance. The old provider tree, executable builtin backend, and builtin webfetch HTTP path are deleted. Removed configuration receives actionable migration errors. The exact pinned Eggsearch v0.4.3 real-process pair passed again on the final implementation commit.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Runtime backend is `eggsearch | disabled` | `SearchBackendConfig`, dispatch matches, default config and disabled tests | pass | Default stays Eggsearch; disabled skips external execution. |
| Remove builtin and fallback execution | Source deletion census; `search_runtime_isolation`; paired suite | pass | Eggsearch errors do not invoke another provider. |
| Actionable migration for old settings | `SearchBackendConfig` deserializer; `parse_config`; config tests | pass | `backend = "builtin"` fails deserialization. `fallback_to_builtin` is detected from parsed config and fails file loading. |
| Delete duplicate provider and fetch implementations | Implementation commit inventory and production source census | pass | Entire `src/search/`, `src/search_backend/legacy.rs`, and legacy integration test removed. |
| Preserve wrappers, schemas, provenance, and raw-MCP policy | `search_backend_eggsearch`, `search_backend_arg_mapping`, `fake_eggsearch_mcp` | pass | 9, 18, and 28 tests respectively. |
| No semantic fallback when Eggsearch is unavailable | `search_runtime_isolation` | pass | 9 tests, including absent service and failure cases. |
| Preserve runtime-context isolation and preflight behavior | `search_runtime_isolation`; `preflight_integration` | pass | 9 and 72 tests. |
| Preserve qualified real-child behavior after retirement | `scripts/test-eggsearch-pair.sh` on final implementation SHA | pass | Exact v0.4.3 binary and digest recorded below; local repo search under Mode Off had zero remote attempts. |
| Documentation and supported installation surfaces agree | docs/architecture diff; installer and clean-host harnesses | pass | Installer: 165 checks. Clean-host: 26 checks. |
| Workspace quality gates | `scripts/verify.sh quick`; workspace Clippy | pass | Commands and exact outcomes below. |

## 3. Production implementation evidence

The retirement removed:

- Every module under `src/search/`: provider registry/routing/types and the DuckDuckGo, Mojeek, Wikipedia, arXiv, OpenAlex, PubMed, Hacker News, Google News, GitHub, Exa, Tavily, Brave, SerpAPI, and Kagi implementations.
- `src/search_backend/legacy.rs`, its `src/lib.rs` export, and `tests/search_backend_legacy.rs`.
- `SearchBackendConfig::Builtin`, `SearchConfig::fallback_to_builtin`, fallback dispatch/provenance, and builtin-specific bootstrap reporting.
- `WebFetchTool`'s builtin HTTP client, extraction/response parsing and truncation path, Cloudflare retry branch, and helpers/tests exclusive to that path.

`SearchRuntimeContext` now dispatches external search/fetch only through Eggsearch or returns the disabled/unavailable error. `WebFetchTool` is a thin Eggsearch wrapper. Native tool names, schemas, categories, Eggsearch fetch controls, and structured provenance remain. Managed-sidecar resolution, explicit command/MCP overrides, coverage diagnostics, output caps, and raw Eggsearch MCP exposure policy remain in the Eggsearch path.

Shared Eggfetch, html2text, and SSRF dependencies were retained because other production components consume them. Generic `ToolBackendKind::BuiltinLegacy` and `ToolProvenance::legacy` were retained for generic legacy-tool compatibility; search wrappers no longer select that backend. No separate provider Cargo feature remains.

## 4. Verification executed

Rust verification and the clean-host run used macOS arm64 with Rust 1.89. The installer-only shell harness ran on the host environment reported by `uname` as x86_64 Darwin. `CARGO_TARGET_DIR` was `/tmp/codegg-c002-arm64-target` for Rust commands. No live web provider was called.

### Commands run

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets --locked
cargo test -p codegg-config --locked
scripts/test-eggsearch-pair.sh
cargo test --test search_runtime_isolation -- --test-threads=1
cargo test --test preflight_integration -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
bash scripts/release/test-installer.sh
bash scripts/release/test-clean-host.sh
```

The paired command used the pinned Eggsearch `v0.4.3` release binary built from immutable source commit `5e7b853e7eebf29c58f84959805c8774279d7ea4`:

- Final CodeGG candidate: `64560ae97c31d3bbf9725c6a887641b819cf6121`
- Eggsearch tag/version: `v0.4.3` / `eggsearch 0.4.3`
- Binary SHA-256: `bfa7d59ee747b4838522ca930f59d3ff841f3923f760a3b0bcbd0eba6bb1dad4`

### Results

- `cargo fmt --all -- --check`: passed.
- `cargo check -p codegg --all-targets --locked`: passed on implementation commit `9769388`; final commit's source-only guidance change was compiled by the final pair suite, tests, and Clippy.
- `cargo test -p codegg-config --locked`: 94 passed, 0 failed on implementation commit `9769388`; config code was unchanged by the final guidance commit.
- Final `scripts/test-eggsearch-pair.sh`: passed. Groups passed: Eggsearch adapter 29, bootstrap 21, research adapter 18, wrapper registration/raw-MCP 9, provider mapping 18, fake MCP 28, MCP child lifecycle 5, and real paired child 1. The real run logged CodeGG `64560ae…`, Eggsearch `0.4.3`, `repo_search` local workspace provenance with `remote_attempts=0`, denied web/security requests, cleared inherited credentials/proxy, private child config/home, and child shutdown with `alive=false`. Pair `network_containment=not_claimed`; this is not a packet-level network isolation claim.
- The same pair script ran `scripts/verify.sh quick`: passed all formatting, static guards, and locked workspace all-target check.
- `cargo test --test search_runtime_isolation -- --test-threads=1`: 9 passed, 0 failed.
- `cargo test --test preflight_integration -- --test-threads=1`: 72 passed, 0 failed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed. The plan's suggested `--all-features` variant was intentionally not used because `AGENTS.md` and the testing guide prohibit all-features workspace sweeps; the repository's canonical broad Clippy command is the one executed here.
- `bash scripts/release/test-installer.sh`: 165 passed, 0 failed.
- `bash scripts/release/test-clean-host.sh`: 26 passed, 0 failed using a generated aarch64 macOS fixture bundle. The installed app, search doctor, managed Eggsearch 0.4.3, Eggsearch wrapper, and deterministic-tools doctor passed with a sanitized PATH and isolated HOME.

The first Clippy run and first installer-harness run both stopped when the
shared host volume ran out of space. I removed only this task's disposable
`/tmp/codegg-c002-arm64-target/debug/incremental` cache, confirmed the volume
had recovered, and reran both commands successfully as reported above. No
source or test failure remained.

The interactive TUI was not launched because this clean-host harness is headless; its `--help` surface passed. No public release, live-provider call, hosted CI run, or Linux sandbox qualification is claimed.

## 5. Invariant review

- Default backend remains Eggsearch and the supported managed sidecar resolves to the pinned v0.4.3 binary.
- Disabled mode skips Eggsearch bootstrap and exposes no external execution.
- Eggsearch absence/failure returns an actionable error. It never routes into a CodeGG provider stack, retries a second provider, or fabricates legacy search provenance.
- Native wrappers and Eggsearch schemas/provenance remain intact; raw MCP tools stay hidden by default.
- Two runtime contexts with different configs remain isolated; tests pass.
- `codesearch`, local filesystem/Git/LSP tools, research coordination and non-search HTTP consumers remain separate.
- No legacy provider routing or provider credential owner remains under CodeGG production source.

## 6. Failure and recovery review

An unavailable configured sidecar reports that the managed Eggsearch sidecar should be configured or `[search].backend = "disabled"` selected. Disabled mode returns its existing bounded disabled error without invoking MCP. The real-child test covers request completion and child teardown. Existing process timeout/cancellation ownership was not modified. No fallback loop or sidecar restart loop was added. As this change removes in-memory execution branches and introduces no persistence, duplicate-delivery and partial-persistence scenarios do not apply.

## 7. Migration and compatibility review

The accepted backend values are `eggsearch` and `disabled`. For example, change:

```json5
search: { backend: "builtin" }
```

to either:

```json5
search: { backend: "eggsearch" } // configure/use the managed or explicit Eggsearch sidecar
```

or:

```json5
search: { backend: "disabled" }
```

Delete the obsolete `fallback_to_builtin` property, for example:

```json5
search: { backend: "eggsearch" }
```

`load_config` routes file content through `parse_config`, which identifies the removed fallback property before normal deserialization and reports its removal. Direct serde parsing of `SearchConfig` is not the file-loading migration boundary. The `builtin` enum value itself has an explicit deserialization error and is never silently reinterpreted as Eggsearch. No storage/schema or protocol migration was needed. Generic legacy tool classification remains because it serves non-search tools.

## 8. Security review

CodeGG no longer owns the duplicate external provider clients, endpoint routing, or associated API-key reads. Production source census found no `SearchBackendConfig::Builtin`, `search_backend::legacy`, `crate::search::`, `SearchProviderRegistry`, Kagi/SerpAPI search credential owners, or Exa/Tavily provider endpoints. The only production `fallback_to_builtin` match is the config migration rejection in `crates/codegg-config/src/paths.rs`; its other source match is a regression test.

The pair run verified child-specific config/home/XDG paths and removal of inherited credentials/proxy variables, plus local-only repo search under Mode Off. It explicitly does not claim host-level network containment. The change introduces no new authorization, secret storage, filesystem, or audit boundary.

## 9. Documentation and operations

Updated `README.md`, `AGENTS.md`, `docs/tools.md`, `architecture/config.md`, `architecture/native_crates.md`, `architecture/overview.md`, `architecture/search_backend.md`, `architecture/tool.md`, and `.opencode/skills/mcp-plugin/SKILL.md`. Updated the implementation plan, owning subsystem roadmap, pair-corrective current prose, and `plans/registry.md`. Historical closure records and closed registry-history entries were left unchanged.

The source census command was:

```bash
rg -n 'SearchBackendConfig::Builtin|fallback_to_builtin|search_backend::legacy|crate::search::|SearchProviderRegistry|KAGI_API_KEY|SERPAPI_API_KEY|api\.exa\.ai|api\.tavily\.com' src crates/codegg-config/src
```

It returned only the deliberate fallback-setting parser/error and its test. Searches for `src/search` and `src/search_backend/legacy.rs` confirmed both paths are absent. Installer and clean-host operator diagnostics report the managed Eggsearch sidecar and source of resolution.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Pair and clean-host runtime evidence is macOS arm64 only; no Linux sandbox or interactive-TUI run was performed. | Other platform execution and interactive TUI surfaces are outside this source-retirement evidence. | Use the normal platform-specific release/CI qualification when such release/platform claims are needed. |
| low | No public network/provider calls or packet-level containment test were run. | Provider health and host-level network isolation are not claimed. | None for M002; provider operation belongs to Eggsearch's own qualification. |

No known M002 correctness, migration, or security finding remains open.

## 11. Roadmap disposition

M002 is closed and the legacy-retirement subsystem roadmap is closed. There is no later milestone in this subsystem to unblock. Dependency review found Evidence-workflow M005 remains blocked on its own Evidence-workflow M002 and M004 closures. Legacy-retirement M002 is not one of those dependencies, so no Evidence-workflow status changed. The source roadmap and registry now record the closure; the historical paired-compatibility C002 registry disposition remains an immutable record of the state at that closure.

## 12. Registry updates

- Marked the legacy-retirement subsystem and M002 closed; removed M002 from the ready queue and added this closure to recently closed entries.
- Clarified that Evidence-workflow M005 depends on Evidence-workflow M002 and M004, not legacy-retirement M002. It remains blocked.
- No future plan was unblocked by this closure, so no future implementation-plan status was changed.
