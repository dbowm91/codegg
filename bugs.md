# CodeGG Bug Report

Audit date: 2026-10-07 · commit `d85ed67b` (branch `main`, clean tree) · Rust 1.89.0 · macOS x64

## How this was produced

Six parallel deep-review passes over disjoint subsystems (config/providers,
core-storage-jobs-scheduler, server/projection/client, TUI-agent-tools-context,
the leaf crates `egglsp`/`egggit`/`codegg-git`/`codegg-document`/`eggsentry`/`eggcontext`,
and the workspace-excluded desktop crate + release scripts), plus a mechanical
dead-configuration sweep and the full `verify.sh` pipeline.

Every finding marked **[reproduced]** was independently confirmed by me against
real tool output or by executing the exact logic — not merely read. Findings
marked **[code-read]** were traced through source and callers but not executed.

## Automated verification status

| Stage | Result |
|---|---|
| `cargo fmt --check --all` | PASS |
| `generate_builtin_agents.py --check` | PASS (generated agents in sync with `assets/`) |
| 3 boundary guards (`core`/`client`/`desktop`) | PASS |
| 15 architecture guards (`sandbox_contract`, `execution_ownership`, `tui_*_authority`, `http_route_disposition`, `audit_coverage`, `scheduler_bypass`, `config_merge_coverage`, `provider_*`, `openai_endpoint_composition`, `eggwork_target_routing`) | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo nextest run --workspace --profile ci` | 5605 passed, **1 flaky TIMEOUT**, 6693 not run (fail-fast aborted) |
| `cargo nextest run -p codegg --features server,plugins,lsp-test-support` | 6465 passed, **1 flaky TIMEOUT**, 9439−6466 not run (fail-fast aborted) |
| `cargo nextest run --workspace --profile ci --no-fail-fast` (complete sweep) | **12299/12299 passed**, 5 skipped, 1 LEAK, exit 0 |

**The suite is functionally green.** See "Test failures" for the two real test-layer
defects it surfaced (a load-dependent hang and a process leak).

**The green pipeline is the central finding of this report**: every one of the 39
product defects below passes `cargo fmt`, `clippy -D warnings`, and all 18 guards. The
bug classes below are exactly the ones the guard suite does not cover.

## Summary

| # | Severity | Component | Defect |
|---|---|---|---|
| 1 | HIGH | `egggit` | Untrusted commit body spoofs every parsed `CommitInfo` field **[reproduced]** |
| 2 | HIGH | `egggit` | Blame author/timestamp wiped for all but the first line of each commit **[reproduced]** |
| 3 | HIGH | `egggit` | Blame misparses long source lines as headers, silently dropping them **[reproduced]** |
| 4 | HIGH | `codegg-providers` | Anthropic endpoint double-joins `/v1` → every request 404s **[reproduced]** |
| 5 | HIGH | `codegg-core` | Projection redactor leaks secret-keyed values (short, or any out-of-class char) **[reproduced]** |
| 6 | HIGH | `src/agent` | `QuestionRegistry::unregister` called with a session id — cleanup never happens **[reproduced]** |
| 7 | HIGH | TUI | `/search` panics on Unicode whose lowercase changes UTF-8 length **[reproduced]** |
| 8 | HIGH | desktop | Session switch leaks the previous session's projection to the renderer |
| 9 | HIGH | `scripts/` | `check_daemon_cwd_usage.py` silently passes on 3 of 5 violation spellings **[reproduced]** |
| 10 | HIGH | config | `formatter` documented + shipped in example config, but the feature is unreachable |
| 11 | HIGH | config | `log_level` validated + shipped in example config, never read by production code |
| 12 | MEDIUM | TUI | Workspace dashboard shares one request counter → permanent "Loading…" hang **[reproduced]** |
| 13 | MEDIUM | TUI | Search highlight anchors on the first textual occurrence, not the current line |
| 14 | MEDIUM | scheduler | Three fallible paths strand a durable job in `Running` forever **[code-read]** |
| 15 | MEDIUM | `codegg-core` | SQLite `finish_attempt` never validates the job-state transition **[code-read]** |
| 16 | MEDIUM | `codegg-core` | In-memory `finish_attempt` mutates the attempt before validating — no rollback **[code-read]** |
| 17 | MEDIUM | `codegg-core` | `claim_due` materializes the job before the dedup guard and swallows errors **[code-read, latent]** |
| 18 | MEDIUM | `eggsentry` | Multi-line secret scanning never runs in production **[code-read]** |
| 19 | MEDIUM | `egggit` | `parse_numstat_line` can never yield `Added`/`Renamed` **[reproduced]** |
| 20 | MEDIUM | `codegg-document` | `len_lines()` over-reports by one for any file without a trailing newline **[reproduced]** |
| 21 | MEDIUM | `codegg-providers` | `responses_api` SSE decodes UTF-8 per chunk, corrupting split characters **[code-read]** |
| 22 | MEDIUM | `codegg-providers` | SSE `data:` parsed with a mandatory space — spec-legal events silently dropped **[code-read]** |
| 23 | MEDIUM | `codegg-providers` | Negative `i64` → `usize` cast inflates context window to ~1.8e19 **[code-read]** |
| 24 | MEDIUM | `scripts/` | `check_sandbox_policy_wiring.py` is permanently red on a correct tree **[reproduced]** |
| 25 | MEDIUM | `scripts/` | 23 guards are wired into neither `verify.sh` nor CI **[reproduced]** |
| 26 | MEDIUM | config | `enabled_providers` documented as supported, never read **[reproduced]** |
| 27 | MEDIUM | config | `plugin` documented + shipped in example config, never read **[reproduced]** |
| 28 | LOW | config | `share` shipped in example config, never read **[reproduced]** |
| 29 | LOW | config | `provider_connections.max_concurrent_refreshes` / `.health_stale_after_ms` never read **[reproduced]** |
| 30 | LOW | config | `enterprise` section entirely unreferenced **[reproduced]** |
| 31 | LOW | `codegg-core` | Post-connect `PRAGMA busy_timeout=5000` contradicts its own 30 s connect-option fix **[code-read]** |
| 32 | LOW | `egggit` | Commit body truncated to its first line **[reproduced]** |
| 33 | LOW | `eggcontext` | `for_model` omits the `o1` family its own comment claims to cover **[code-read]** |
| 34 | LOW | `eggcontext` | Tokenizer load failure reported as an exact count of 0 **[code-read]** |
| 35 | LOW | `codegg-client` | Unbounded `read_line` from the daemon socket **[code-read]** |
| 36 | LOW | server | `artifact_list` check-then-claim is non-atomic, unlike `artifact_read` **[code-read]** |
| 37 | LOW | `scripts/` | `check_tool_broker_boundary.py` flags trait self-dispatch; permanently red **[reproduced]** |
| 38 | LOW | server | `deliver_to_stream` result discarded after durable commit **[code-read]** |
| 39 | MEDIUM | `src/agent` | Blocking `SnapshotBuilder::build` runs on a tokio worker thread, not `spawn_blocking` **[code-read]** |
| 40 | FLAKY | `src/agent` | Test hangs 120 s under CPU load; passes in 0.00 s in isolation **[reproduced]** |
| 41 | LOW | `src/tool` | `ToolRegistry` has no `Drop`/`shutdown`; nextest flags a leaking test **[reproduced]** |

---

# HIGH

## 1. Untrusted commit body can spoof every field of `CommitInfo`

`crates/egggit/src/log.rs:66-99` (dispatch loop at `:66`)

```rust
for line in &lines {
    if line.starts_with("commit:") { oid = parse_value(line, "commit:"); }
    else if line.starts_with("author-name:") { author_name = parse_value(line, "author-name:"); }
    ...
    } else if line.starts_with("body:") { body_lines.push(parse_value(line, "body:")); }
}
if oid.is_empty() { continue; }   // guard runs AFTER the loop
```

The parser is line-oriented, but `git log --format=…%b…` emits `body:` followed by
**every subsequent body line raw**. Because continuation lines are tested against
the same field prefixes as real headers, and the `oid.is_empty()` guard runs only
after the loop, a crafted commit message rewrites `oid`, `short_oid`, `parents`,
`author_*`, `committer_*`, `subject` and `decorations`.

**[reproduced]** Real `git log` output for a commit whose body contains two
forged header lines:

```
commit:d96a55c611a10c9b447b4c8c850af16d2875d954
short:d96a55c
author-name:Real Author
author-time:1791347499
subject:honest subject line
body:this is the body first line
author-name: Attacker      <-- untrusted body line, matched by the parser
author-time: 1
```

`author_name` ends as `"Attacker"` and `author_time` as `1`.

**Failure scenario:** an agent runs the read-only `git log` action in a cloned or
otherwise untrusted repository. The structured JSON handed to the model shows
forged authorship and OIDs. A `---END---`-style body line additionally
desynchronizes every subsequent commit record.

Blast radius is the public `eggit::log_commits` crate API; `src/git_service.rs:940`
uses a separate parser, so the live tool path is currently unaffected.

## 2. Blame author/timestamp wiped for every line after the first in a commit group

`crates/egggit/src/blame.rs:58-59`

```rust
// Reset per-line metadata for new hunk
current_author.clear();
current_author_time = 0;
```

`git blame --porcelain` emits `author` / `author-time` **only on the first line of
a run**. Continuation lines carry only `<sha> <orig> <final>`.

**[reproduced]** Real output from a 4-line file in a 2-commit repo, run through a
faithful port of `parse_porcelain`:

```
 ln author     time         content
  1 'Tester'   1791347361   line one
  2 ''         0            line two
  3 ''         0            line three
```

**Failure scenario:** any file where ≥2 consecutive lines share a commit — the
common case — yields `author == ""` and `author_time == 0` for all but the first
line of each group. Attribution is silently wrong. The two `clear()`/`= 0` lines
are the defect; the reset is only valid for a new *author*, not a new line.

Tests miss it: `blame_entries_have_commit_info` uses a 1-line file (single group),
`blame_multiline_file` asserts only `lineno`/`content`, and
`blame_after_modification` uses one commit per line.

## 3. Long source lines misparsed as blame headers: line dropped, later entries corrupted

`crates/egggit/src/blame.rs:51`

```rust
if line.len() >= 41 && line.as_bytes()[40] == b' ' {
```

The check never verifies a leading tab or a 40-hex-char first field. Git prefixes
content with `\t`, so any file line whose **byte offset 39 is a space** (and is
≥40 bytes long) is misclassified as a header. Line 61 then `continue`s, so the
line is **silently dropped**, while `current_commit` is overwritten with the
line's own text and `current_orig_lineno` with `parts[1].parse()`.

**[reproduced]** A tab-indented line with 38 `y`s followed by a space is
misclassified. Running the real parser logic over the real output:

```
entries returned: 3 | actual file lines: 4
```

**Failure scenario:** `blame_file` returns fewer entries than the file has lines.
Because `lineno = entries.len() + 1`, **every subsequent line number silently
shifts**, and surviving entries carry garbage `commit`/`author`/`orig_lineno`.
Triggers include aligned tables, long indented string literals, and wide comments.

## 4. Anthropic endpoint double-joins `/v1`, producing an unreachable URL

`crates/codegg-providers/src/anthropic.rs:101`

```rust
let url = format!("{}/v1/messages", self.base_url);
```

`with_base_url` (`:27-28`) stores the operator string **verbatim** — no trailing-slash
strip, no `/v1`-prefix detection. It is fed directly from config at
`provider_core.rs:869` and `setup_catalog.rs:512`.

**Failure scenario:** `"base_url": "https://proxy.corp/v1"` (the exact form the docs
use for OpenAI) → requests go to `https://proxy.corp/v1/v1/messages` → 404 on every
turn. A trailing slash yields `…com//v1/messages`.

This is precisely the class the sibling provider already fixed: `openai.rs`
normalizes through `chat_completions_url` (`:70`) and carries a regression test
`native_source_does_not_reintroduce_duplicated_version_prefix` whose comment reads
*"Old defect composed `{base_url}/v1/chat/completions` on top of an already-versioned
prefix."* Anthropic has no normalizer and no test; `AzureProvider` does strip
(`azure.rs:16`). The inconsistency is provable, not stylistic.

## 5. Projection redactor leaks secret-keyed values shorter than 16 characters

`crates/codegg-core/src/projection_replay/redactor.rs:537` and `:278-296`

```rust
// classify_object_key
if is_secret_key(&lower) || lower.contains("authorization") || lower == "auth" {
    FieldName::Authorization
```
```rust
// RULES_AUTHORIZATION — the only mask that catches a bare secret
RedactionRule { name: "auth-blob", pattern: r"^[A-Za-z0-9+/=._\-]{16,}$", ... }
```

`is_secret_key` (`:558+`) matches `api_key`, `apikey`, `api-key`, `access_token`,
`secret_key`, `secret`, `auth_token`, `password`, `passwd`, `pwd`, `client_secret`.
All of them map to `FieldName::Authorization`, whose ruleset only masks values that
carry a `Bearer`/`Basic` prefix **or** are ≥16 chars of `[A-Za-z0-9+/=._-]`.

**Failure scenario:** a payload field `{"api_key": "sk-abc123"}` (10 chars) →
`redact_text` returns `Unchanged` → `summary.is_clean()` is true →
`redact_envelope` (`service.rs:661-665`) returns the **original unredacted
envelope**. The credential is persisted to the replay store and delivered to
subscribers.

**[reproduced]** Running the guard's own three patterns against secret-keyed values:

| Key | Value | `auth-blob` | `auth-bearer` | `auth-basic` | Result |
|---|---|---|---|---|---|
| `api_key` | `sk-abc123` | ✗ | ✗ | ✗ | **leaks** |
| `password` | `hunter2!` | ✗ | ✗ | ✗ | **leaks** |
| `client_secret` | `a/b+c=d_e-fghij` (15 chars) | ✗ | ✗ | ✗ | **leaks** |
| `password` | `correcthorsebattery` | ✓ | ✗ | ✗ | masked |
| `api_key` | `AKIAIOSFODNN7EXAMPLE` | ✓ | ✗ | ✗ | masked |

Note the second row: the leak is **not only about length**. Any secret containing a
character outside `[A-Za-z0-9+/=._-]` escapes `auth-blob` **regardless of length** —
so a 40-character password containing `!`, `~`, a space, or any non-ASCII character is
never masked.

This contradicts the module doc (`redactor.rs:5-11`) which claims the pipeline
"fails closed". `service.rs:661` and `:766` are the only `redact_json` callers, and
no other pass masks secret keys, so there is no backstop.

## 6. `QuestionRegistry::unregister` called with a session id — cleanup is a guaranteed no-op

`src/agent/tool_batch.rs:1884`

```rust
QuestionRegistry::unregister(&self.session_id);
```

`unregister(question_id: &str)` (`crates/codegg-core/src/bus/mod.rs:323`) forwards
to `unregister_scoped(DEFAULT_SESSION_ID, question_id)` →
`senders.remove_if(question_id, |_, v| v.session_id == "default")`. The map is keyed
by question id (`format!("q-{uuid}")`, `tool_batch.rs:248/290`). `self.session_id` is
a session identifier and is never a key. There are **two independent mismatches**:
the wrong key, and a scope predicate demanding `"default"` when registration used the
real session id. The `remove_if` can never match.

**Failure scenario:** a question that times out or is cancelled (the `Ok(Err(_))`
and `Err(_)` arms at `:1854`/`:1868`) leaves its entry in the global registry.
`get_pending_for_session` (`src/server/routes/question.rs:38,76,117`) and
`pending_question_ids` (`src/server/ws.rs:2573`) then report a **phantom pending
question** until the 310 s TTL sweep; answering it hits `tx.send()` on a dropped
receiver and returns "No pending question found".

The correct sibling implementation is `src/permission/approval.rs:301`, which calls
`unregister_scoped(&request.session_id, perm_id)`. This is the exact
sync-responder-cleanup contract from `AGENTS.md`; the permission path honours it and
the question path does not.

## 7. `/search` panics on Unicode whose lowercase form changes UTF-8 length

`src/tui/components/messages.rs:1441-1455`

```rust
let lower_content = part_content.to_lowercase();
while let Some(pos) = lower_content[start..].find(case_insensitive_query) {
    let abs_start = start + pos;
    let abs_end = abs_start + query.len();
    let line_in_msg = part_content[..abs_start].matches('\n').count();
```

`str::to_lowercase()` applies Unicode SpecialCasing, which **changes UTF-8 byte
length**. Offsets found in `lower_content` are then used to slice the *original*
`part_content`.

**[reproduced]** Two distinct panic modes:

- *Not a char boundary* — `"ẞhello"` is 8 bytes; lowercased `"ßhello"` is 7 bytes.
  `find("hello")` returns `2`, but byte index 2 in the original lands **inside** the
  3-byte `ẞ` (`E1 BA 9E`) → `byte index 2 is not a char boundary`.
- *Out of bounds* — `"İ"×6 + "hello"` lowercases 1 byte per `İ` **longer**, so
  `abs_start = 18 > part_content.len() = 17` → range-out-of-bounds panic.

**Failure scenario:** an assistant or tool message contains `ẞ` or `İ` and the user
runs `/search hello`. Content is untrusted model/tool output; the entry point is
`src/tui/app/mod.rs:5036`. The corrupted `start`/`end` are also stored in
`SearchMatch` and re-sliced in the render path. No test covers `search()` with
non-ASCII input.

## 8. Desktop session switch leaks the previous session's projection to the renderer

`apps/desktop/src-tauri/src/route.rs:347`

```rust
route.session_id = Some(session.id.clone());
```

`route_session_open` changes `route.session_id` but, unlike `route_project_detail`
(`route.rs:218`) and `route_workspace_select` (`route.rs:251`), neither bumps
`route_generation` nor calls `stop_projection_owner()`.

`projection_current` (`projection.rs:219-222`) fences **only** on
`(connection_generation, route_generation)` — it never compares `owner.session_id`
to `route.session_id`, unlike `route_artifact_read` which does exactly that at
`artifact.rs:65`.

**Failure scenario:** open session A → `desktop_projection_start` attaches owner A →
open session B in the same workspace (same route generation) → `session_id` becomes
B while owner A stays attached and the generation is unchanged →
`desktop_projection_current()` passes its fence and returns **session A's**
transcript and tool output to a renderer now labelled session B.
`projection_watcher_current` (`projection.rs:315-319`) has the same generation-only
fence, so the stale watcher keeps streaming A's views, and a live daemon
subscription for A leaks.

The renderer's `view.sessionId !== expectedSession` filter (`App.tsx:144`) mitigates
the push path only after `projectionStart` resolves; it does not protect
`projectionCurrent`, which `App.tsx` calls at lines 283, 314, 342.

## 9. `check_daemon_cwd_usage.py` silently passes on three of five violation spellings

`scripts/check_daemon_cwd_usage.py:74` and `:108-111`

```python
ENV_CWD_RE = re.compile(r"std::env::(current_dir|set_current_dir)\s*\(")
# comment two lines above claims: "The leading `std::env::` is optional
#                                  because `use std::env;` may be in scope."
...
re.compile(r"std::env::current_dir\(\)\s*\."),   # line 110 — matches ANY method chain
```

**[reproduced]** Applying the guard's own regex + allowlist:

| Injected line | Matched | Suppressed | Verdict |
|---|---|---|---|
| `let root = std::env::current_dir();` | yes | no | detected |
| `let root = std::env::current_dir().unwrap();` | yes | **yes** | **silent pass** |
| `let root = std::env::current_dir().map(\|p\| p.join("x"));` | yes | **yes** | **silent pass** |
| `let root = std::env::current_dir().expect("cwd");` | yes | **yes** | **silent pass** |
| `env::current_dir();` | **no** | no | **not detected** |

So the guard only fires on the exact bare qualified spelling. Unqualified
`env::current_dir()` is real in this codebase (`src/main.rs:1549,1590,1652,1694,2394,2560,3248,3496`,
and `src/main.rs` does `use std::env;`). `AGENTS.md` documents this guard as the
remedy for "no `std::env::current_dir()` in workspace-bound daemon code"; the
allowlist entry at line 110 defeats it for every method-chained call. Compounding
this, the guard is wired into neither `verify.sh` nor CI (finding 25).

## 10. `formatter` is documented and shipped in the example config, but the feature is unreachable

`docs/configuration.md:77,89,186`, `codegg.example.jsonc:244`, `crates/codegg-config/src/schema.rs:259`

`Config::formatter` is declared, merged (`paths.rs:190`), and documented with
detailed shape guidance — including a warning that `{"rules": {...}}` is wrong. The
annotated example ships a working-looking rustfmt/prettier configuration.

But the entire implementation, `src/tool/formatter.rs`, is **unreachable
production code**:

- `pub mod formatter;` is declared (`src/tool/mod.rs:28`) so it compiles,
- `Formatter::new` is called only from lines 107–163 — all inside the
  `#[cfg(test)]` module that starts at line 101,
- `format_file` is called only from tests at `:147,164`,
- no `Formatter` value is constructed anywhere else in `src/`, `crates/`, or `apps/`,
- no `format` tool is registered in the tool registry.

**Failure scenario:** a user configures `"formatter": {"rs": {"command": ["rustfmt", "$FILE"]}}`
following the shipped example and `docs/configuration.md`. Nothing is ever formatted;
the setting is silently inert. `docs/tools.md` documents no `format` tool at all,
which contradicts the config docs pointing `formatter` at it.

## 11. `log_level` is validated and shipped in the example config, but never read

`crates/codegg-config/src/schema.rs:222,2625-2629`, `codegg.example.jsonc:14`, `src/main.rs:1123-1163`

The field is declared, merged (`paths.rs:172`), and **validated**:

```rust
if let Some(ref level) = self.log_level { /* must be one of debug, info, warn, error, trace */ }
```

`codegg.example.jsonc:14` ships `"log_level": "info"`.

`src/main.rs` derives the tracing filter **solely** from the CLI verbosity flag and
never consults the loaded config:

```rust
let log_level = verbosity_log_level(cli.verbose);      // :1123
… EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level))
```

**Failure scenario:** a user sets `"log_level": "debug"` in config expecting debug
diagnostics. An invalid value is *rejected at load time* (so the key is clearly live)
but a valid value has **zero effect**. The only working control is `-v`/`-vv` or
`RUST_LOG`. Note config load happens *after* CLI bootstrap, so wiring this also needs
reordering — the same ordering hazard the previous commit fixed for `--cwd`.

---

# MEDIUM

## 12. Workspace dashboard shares one request counter → permanent "Loading…" hang

`src/tui/app/state/workspace_dashboard.rs:112`, `src/tui/commands/workspace_dashboard.rs:181,381,310`

`WorkspaceDashboardState` holds a single `pub request: AsyncUiRequestState` (`:112`)
that **two independent operations** share: page refresh (`commands/workspace_dashboard.rs:181`)
and inline expand (`:381`).

**[reproduced]** The reachable sequence, confirmed line by line:

1. `begin_refresh` (`:244-250`) sets `loading = true` but does **not** clear `rows`.
2. `selected()` (`:221-226`) reads `self.rows`, so it still returns a row — and
   `toggle_dashboard_expand` (`:342`) gates **only** on `.selected()`, not on `loading`.
3. Expand calls `request.begin()`, making the refresh's id stale.
4. The refresh completion hits `:310` `if !dashboard.request.finish(request_id) { return; }`.
5. `apply_loaded` — the **only** place that clears `loading` (`:273`) — never runs.

**Failure scenario:** press `Ctrl+R`, then `Space` to expand a project before the
daemon replies. The view renders `"Loading workspace…"` **indefinitely** with stale
rows until a manual `Ctrl+R`. The reverse order leaves `expanded_loading = true`, so
the row shows `"loading tasks…"` forever.

This is exactly the stale-completion bug class `AGENTS.md` documents for
`spawn_tui_task`. Existing tests (`stale_generation_and_epoch_completions_drop`,
`revocation_clears_row_and_detail`) exercise generation fencing but never interleave
these two operations on one counter.

## 13. Search highlight anchors on the first textual occurrence, not the current line

`src/tui/components/messages.rs:1806`

```rust
let line_start = content.find(text_line).unwrap_or(0);
let line_end = line_start + text_line.len();
```

`content` is the whole message part while `text_line` iterates `content.lines()`, so
`find` locates the **first** occurrence of that line's text anywhere in the part.

**Failure scenario:** a message repeating a line (routine in terminal and build
output) highlights the wrong line when the user presses `n`. When the line text is
absent, `unwrap_or(0)` silently anchors highlighting to offset 0.

Note the same function also slices `&text_line[..rel_start]` with a **byte** offset
derived from the original string, so a multi-byte character on the highlighted line
panics here too — the sibling path at `:2355-2370` correctly clamps through `chars`,
confirming this site is the outlier.

## 14. Scheduler strands a durable job in `Running` on three fallible paths

`src/scheduler/scheduler.rs:1057-1059` (also `:1067-1069`, `:1079`)

```rust
self.store.set_attempt_source_subject_started(&attempt.attempt_id, &start_provenance).await?;
```

`begin_attempt` (`:1010`) has already durably moved the job to `Running`. These three
`?` operators return **before** the executor task is spawned (`:1153`), before the
running-map insert (`:1085`), and before the counter increments (`:1109/1111`).

On error the job stays `Running` with a `Created` attempt and nothing running.
`reconcile` only re-queues `states: vec![JobState::Queued]` (`:622-623`), and there is
no reaper (`grep -E 'reap|orphan_job' src/scheduler/` → none). The job is stuck until
daemon restart, where `recover_generation` finally interrupts it. Contrast the
deliberate `mark_unschedulable` cleanup on the sibling error paths (`:988-1005`).

## 15. SQLite `finish_attempt` never validates the job-state transition

`crates/codegg-core/src/jobs/store.rs:1985-2085`

Only `validate_attempt_transition` is called (`:2005`); the
`UPDATE job SET state = ? … WHERE id = ?` at `:2050-2051` unconditionally overwrites
the job. The in-memory twin validates at `:746`.

This diverges from the documented invariant *"Stale workers may not overwrite a
terminal state"* (`architecture/jobs.md:353`) and from the conformance claim at
`store.rs:5`. `tests/durable_jobs_phase4.rs:927` exercises only the happy path.

Reported as a latent enforcement hole: I could not construct a reachable production
state with the job terminal while the attempt is `Created`/`Admitted`/`Running`
(`request_cancel` only records `cancel_requested_at`, and `recover_generation`
interrupts the attempt before terminalizing the job), so the attempt-level check
masks it today.

## 16. In-memory `finish_attempt` mutates the attempt before validating — no rollback

`crates/codegg-core/src/jobs/store.rs:733-735` (validation at `:746`)

```rust
guard.attempts.insert(completion.attempt_id.clone(), new_attempt);   // :735
…
validate_state_transition(job.state, new_job_state).map_err(...)?;    // :746
```

On `InvalidTransition` the `?` returns with the attempt already mutated terminal and
the job unchanged — violating the atomic contract *"Atomically persist attempt + job
completion"* (`architecture/jobs.md:239`). `validate_state_transition(X, X)` is
invalid for `Scheduled`/`Queued`/`Blocked` per the table at `:86-100`. No current
caller triggers it (`persist_completion` always maps to a terminal state).

## 17. `claim_due` materializes the job before the dedup guard and swallows errors

`crates/codegg-core/src/jobs/schedule_store.rs:572-603`

`materialize(...).await.ok()` (`:573-577`) creates a durable job **before**
`INSERT OR IGNORE INTO schedule_occurrence` (`:581`). The documented contract
(`architecture/jobs.md:263-275`) requires the PK insert first with duplicates failing
as `DuplicateOccurrence` — a variant this code can never emit. A duplicate claim
therefore creates a second job and silently drops the row: the occurrence keeps the
old `job_id` while `claim_due` returns the new one — exactly the double-fire the PK
exists to prevent.

Additionally a `MaterializerError` yields `job_id = None` with status `Queued`,
records the occurrence as queued, advances `next_run_at`, and returns a synthetic
`skipped-<ms>` `JobId` (`:601`) — **the run is silently lost**. Same shape in the
in-memory impl (`:288-313`).

Severity is capped: `claim_due` has **no production callers** (verified repo-wide
outside tests), so this is latent.

## 18. The multi-line secret pass is dead code in production scanning

`crates/eggsentry/src/scanner.rs:422` vs `:319-326`

```rust
// inspect_text — the only caller of inspect_multiline
let mut findings = inspect_lines(...);
findings.extend(inspect_multiline(...));

// inspect_file — what all three production runners use
Ok(inspect_lines(Some(path), &text))
```

All three production profile runners call `inspect_file` — `profile.rs:81` (ambient),
`:158` (pre-commit), `:171` (security review). `inspect_text`, the only path that
reaches `inspect_multiline` (`:334`), is referenced solely from `#[cfg(test)]`.

**Failure scenario:** a PEM body or base64 token split across line breaks in a real
file on disk produces **zero findings** from ambient scanning, pre-commit, and
security review, while the same bytes handed to `inspect_text` in tests would be
flagged. `inspect_multiline` has no test at all. This is the highest-value scanner
finding to fix — it is a pass that documents itself as catching "secrets wrapped
across line breaks (PEM bodies, chunked base64, split tokens)" and never runs.

## 19. `parse_numstat_line` can never yield `Added`/`Renamed` and returns a non-path for renames

`crates/egggit/src/diff.rs:63-69`

```rust
let kind = if added.starts_with('-') && removed.starts_with('-') { Deleted }
           else if added == "0" && removed == "0" { Other }
           else { Modified };
```

**[reproduced]** Real `git diff --numstat` output:

```
0	0	brand_new_empty.rs
0	0	src/{old.rs => new.rs}
```

Both classify as `Other`, and the rename path is the brace-compressed display form
`src/{old.rs => new.rs}` — not a real path. `ChangeKind::Added` and
`ChangeKind::Renamed` are **unreachable** from this parser.

**Failure scenario:** `changed_files()` (consumed at `src/git_service.rs:526` →
`ChangedFilePayload`) reports renames as `"other"` with an unusable path and
added-empty files as `"other"` instead of `"added"`; any downstream path join on
`ChangedFile.path` fails for renames.

## 20. `len_lines()` over-reports by one for any document without a trailing newline

`crates/codegg-document/src/buffer.rs:39-41`

```rust
pub fn len_lines(&self) -> usize { self.text.line_len().saturating_add(1) }
```

crop 0.4.3: `line_len() = line_breaks + 1 - has_trailing_newline - is_empty`. The
trailing `+1` is correct only when the text ends in `\n`:

- `"abc\n"` → `1+1-1 = 1` → `len_lines() = 2` ✅ (lines `"abc"`, `""`)
- `"abc"` → `0+1-0 = 1` → `len_lines() = 2` ❌ (there is only **1** line)

The `+1` exists because `byte_of_line()` still accepts an offset past a
non-terminated final line, so `line_range(1)` returns `3..3` and
`position_to_byte(line=1, col=0)` returns `Ok(3)` — a phantom line one byte past EOF.

**Failure scenario:** most files lack a trailing LF → the editor status/gutter shows
one extra line (`src/tui/editor.rs:182,715`), cursor/selection bounds permit line
index `len_lines()-1` (`src/tui/editor.rs:204,223,286,327,523,722`), and
`crates/codegg-client/src/document.rs:290` reports an inflated `line_count`. The
single assertion (`tests/transactions.rs:67`) uses `"é\r\nx\n"` — the one case where
the formula is correct.

## 21. `responses_api` SSE decoder corrupts multi-byte UTF-8 split across chunks

`crates/codegg-providers/src/responses_api.rs:1471`

```rust
let chunk_str = String::from_utf8_lossy(&chunk).to_string();
buffer.push_str(&chunk_str);
```

Decoding is applied **per network chunk**, not to the accumulated buffer.
`bytes_stream()` splits at arbitrary byte offsets, so a multi-byte character
straddling a chunk edge is decoded twice as U+FFFD.

**Failure scenario:** a CJK/emoji token arriving as `[0xE4][0xB8]` + `[0xAD]` becomes
`""` instead of `"中"`. The replacement characters land inside a `data:` JSON payload;
`serde_json` may still parse it, silently persisting corrupted model output into the
session transcript.

Every other provider routes through `wire::shared_stream` →
`SharedStreamDecoder::push()` (`wire.rs:304`), which is incremental and
boundary-correct. Reachability caveat: `ResponsesTransport::create_response_stream`
has no production caller (only `tests/hosted_tool_program_{adapter,contention}.rs`),
so this is a live public API defect of a published crate rather than a live turn-path bug.

## 22. SSE `data:` parsed with a mandatory space — spec-legal events silently dropped

`crates/codegg-providers/src/responses_api.rs:1403` (and `bedrock.rs:396`)

```rust
if let Some(d) = line.strip_prefix("data: ") {
```

The SSE spec strips **one optional** leading space after the colon, so `data:{...}`
is legal and is emitted by real servers and gateways. Non-matching lines leave `data`
empty → `serde_json::from_str::<ResponsesStreamEvent>("")` fails → `continue` at
`:1454` **silently discarding the event**.

**Failure scenario:** a proxy emitting `data:{"type":"response.output_text.delta",…}`
drops every delta — no `TextDelta` and no `Finish` — and the turn ends as an empty
*successful* response. This is the "failure silently becomes empty success" class.

`wire.rs:634` correctly uses `strip_prefix("data:")`, so the inconsistency is provable.
Same reachability caveat as finding 21.

## 23. Negative `i64` → `usize` cast inflates the context window to ~1.8e19

`crates/codegg-providers/src/discovery.rs:105`

```rust
context_window: r.3.unwrap_or(128_000) as usize,
max_output_tokens: r.4.map(|v| v as usize),
```

`r.3`/`r.4` are `Option<i64>` from the `cached_models` table, whose columns are
declared unconstrained `INTEGER` (`codegg-core/src/session/schema.rs:664-672`). The
write path uses unchecked `as i64` (`discovery.rs:210-211`).

**Failure scenario:** a negative value — from a corrupted or older row, a manual DB
edit, or another writer — becomes `18446744073709551615` on read, and that value is
served to the agent as the model's context window. The cast is provably unsound;
whether a negative row can occur is the unproven part. (`catalog.rs:60-64` and
`opencode_zen.rs:190-194` read `as_u64()` and are safe — only `discovery.rs` reads `i64`.)

## 24. `check_sandbox_policy_wiring.py` is permanently red on a correct tree

`scripts/check_sandbox_policy_wiring.py:64`

```python
(r"sandbox_profile,?\s*\n\s*\}\);", "factory passes sandbox_profile to ToolRegistryOptions")
```

The regex requires `sandbox_profile` to be the **last** field before `});`, but
`src/tool/factory.rs:180` has it followed by `memory_store`, `project_identity`, and
`extension_catalog` (lines 180-182).

**[reproduced]** Running the guard today:

```
$ python3 scripts/check_sandbox_policy_wiring.py
rc=1
sandbox-policy-wiring guard failed:
  src/tool/factory.rs: missing factory passes sandbox_profile to ToolRegistryOptions
```

`factory.rs:180` **does** correctly thread `sandbox_profile`. Verified the guard regex
cannot match: `re.search(pat, factory.rs, DOTALL)` → no match. Because the guard is
permanently failing it provides zero signal and will be ignored — and it is unwired
from `verify.sh`, so nobody sees it. `plans/closure/execution-reliability-approval-autonomy/005-status.md:48`
claims this guard **passes**, contradicting reality.

## 25. 23 guard scripts are wired into neither `verify.sh` nor CI

Enumerated by diffing every `scripts/check_*.py` against `scripts/verify.sh` and
`.github/workflows/ci.yml`. Includes `check_daemon_cwd_usage.py` (finding 9),
`check_sandbox_policy_wiring.py` (finding 24), `check_websocket_bounds.py`,
`check_identity_path_usage.py`, `check_projection_transport_isolation.py`,
`check_approval_router.py`, `check_master_key_resolver.py`, `check_tool_broker_boundary.py`,
`check_git_forbidden_patterns.py` and 14 more.

`AGENTS.md` documents these as "change-triggered", but **nothing fails when a
change-triggered guard is skipped**. Findings 9, 24 and 37 all live in this unwired set,
which is precisely why none of them blocks a commit. Of the 23, only 8 currently pass.

## 26. `enabled_providers` is documented as supported but never read

`docs/configuration.md:179`, `docs/…` provider group table, `architecture/config.md:187,389`

Declared (`schema.rs:243`), merged (`paths.rs:182`), and documented alongside
`disabled_providers` as the provider allow/deny list — but `disabled_providers` has a
reader (`crates/codegg-providers/src/provider_core.rs`) and `enabled_providers` has
**zero field accesses anywhere** in `src/`, `crates/`, `apps/`, `tests/`, or `examples/`.

**Failure scenario:** a user sets `enabled_providers` to restrict which providers load.
The setting is silently ignored and all configured providers remain active — the
opposite of the requested policy, with no diagnostic.

## 27. `plugin` is documented and shipped in the example config but never read

`codegg.example.jsonc:277`, `docs/configuration.md:79,187`, `schema.rs:265`

```jsonc
// WASM plugin manifests, as paths. Each entry is a string (or a
// [path, options] pair), NOT an object.
"plugin": [ "./plugins/custom.wasm" ],
```

`PluginSpec` has **zero references outside `codegg-config`** and `config.plugin` has
zero field accesses. Plugin loading is directory/manifest-scan based
(`docs/PLUGINS.md`), so this key is inert.

**Failure scenario:** a user adds a WASM plugin path to config per the shipped example;
it never loads, and no diagnostic explains why.

## 28-30. Further unread configuration

| Key | Location | Evidence |
|---|---|---|
| `share` | `codegg.example.jsonc:42`, validated `schema.rs:2635-2640` | Zero field accesses. Example ships `"share": "disabled"`. |
| `provider_connections.max_concurrent_refreshes`, `.health_stale_after_ms` | `schema.rs:387,392` (decl + `Default`) | Zero reads. `architecture/config.md:276-280` documents both as daemon-owned refresh policy (`max_concurrent_refreshes=1`, `health_stale_after_ms=300000`) — the only mentions outside the declaration are the `Default` impl and the doc itself. The other six fields in the section are read in `src/core/eggpool.rs`. |
| `enterprise` | `schema.rs:266` | `EnterpriseConfig` never referenced outside `codegg-config`. Undocumented, so lower harm. |

`autoupdate` is also unread but is **explicitly documented as inert** in its own doc
comment and in `docs/configuration.md:73` — correct behaviour, not a defect.

---

# LOW

## 31. Post-connect `PRAGMA busy_timeout` contradicts the documented fix

`crates/codegg-core/src/storage/mod.rs:270` sets `PRAGMA busy_timeout=5000;` in the
pool-level batch, versus `busy_timeout(Duration::from_secs(30))` in the connect options
at `:254`. The comment at `:244-250` states explicitly that per-connection pragmas
**must** ride the connect options because a pool-level pragma "touches exactly one
pooled connection, leaving every other connection at busy_timeout=0". The batch then
re-applies `busy_timeout` (plus `synchronous`, `foreign_keys`) to that single
connection, downgrading it from 30 s to 5 s.

## 32. Multi-line commit bodies truncated to their first line

`crates/egggit/src/log.rs:92-96` — only lines starting with `body:` are pushed into
`body_lines`, but git emits subsequent body lines unprefixed. Every multi-line body
loses everything after line one. Same root cause as finding 1.

## 33. `TokenizerType::for_model` omits the `o1` family its own comment claims to cover

`crates/eggcontext/src/lib.rs:72-75` — the comment reads *"o3-mini, `o1`, gpt-4.1, and
explicit o200k hints all use the newer o200k_base vocabulary"* but no `contains("o1")`
test exists, so `o1`, `o1-mini`, `o1-preview` fall through to `Cl100kBase`.
Compaction budgets for those models are computed against the wrong vocabulary, which
systematically undercounts and risks a context-window overrun.

## 34. Tokenizer load failure reported as an exact count of zero

`crates/eggcontext/src/lib.rs:177-191` and `:230-234` — `tiktoken::get_encoding(…)`
failures become `.unwrap_or(0)` while `TokenEstimate.approximate` stays `false`, i.e. a
failure is masked as an exact empty-success count, so every downstream budget check
treats the text as free.

## 35. Client reads unbounded lines from the daemon socket

`crates/codegg-client/src/local.rs:302` — `reader.read_line(&mut line).await` with no
`.take()` bound and no `MAX_FRAME` cap anywhere in the client or daemon frame reader.
A daemon that sends bytes without a newline makes the client buffer until OOM. Capped
at LOW because the socket is a user-scoped `flock` singleton, so the attacker must
already be the same user.

## 36. `artifact_list` check-then-claim is non-atomic

`src/server/ws.rs:3776` and `:3789` — `owns_project` and `try_begin_artifact_read()` are
two separate `projection.lock().await` acquisitions, while the sibling
`handle_projection_artifact_read` (`:3861-3866`) does both under one acquisition with
the comment *"so the ownership check and artifact-read claim are evaluated
atomically"*. Bounded in practice: the daemon independently re-authorizes
(`crates/codegg-core/src/authorization/policy.rs:690-694`) and
`tests/presence_m003_observation.rs:676-692` confirms an outsider is denied. A
defense-in-depth inconsistency, not an exploitable disclosure.

## 37. `check_tool_broker_boundary.py` flags trait self-dispatch and is permanently red

`scripts/check_tool_broker_boundary.py:32` — `re.compile(r"\.execute_structured\(")`
matches any occurrence, including a `Tool` trait implementation calling itself.
**[reproduced]** Current failures:

```
src/tool/extension.rs:43,120
src/tool/git_read.rs:144
src/tool/verify.rs:227  Ok(self.execute_structured(input, None).await?.output)
src/tool/verify.rs:244
```

These are trait implementations, not broker bypasses. The guard is red on correct code,
so a developer cannot distinguish new violations from permanently-accepted ones.

## 38. `deliver_to_stream` result discarded after a durable commit

`crates/codegg-core/src/projection_replay/service.rs:363` —
`let _ = self.subscriptions.deliver_to_stream(sid, envelope.clone());` after `tx.commit()`
(`:351`). Currently unreachable because the function only returns `Ok(delivered)` and
encodes per-subscription failure internally (Full → `ResyncRequired`, Closed → no-op),
so this is dead error handling that becomes a real silent drop the moment an error path
is added.

## 39. MEDIUM — Blocking `SnapshotBuilder::build` runs on a tokio worker thread

*(Numbered out of sequence because it was found after the first pass; severity is
MEDIUM. Inserted here to preserve the cross-reference numbering of findings 1–38.)*

`src/agent/asset_refresh.rs:232`

```rust
// The builder is intentionally outside the publication lock. It
// may perform bounded filesystem discovery and can fail without
// disturbing the previous immutable publication.
match self.builder.build(&context) {
```

`SnapshotBuilder::build` is a **synchronous** trait method
(`fn build(&self, …) -> Result<…>`, no `async`), and it is invoked directly from
inside the `async fn refresh_with_cancellation`. There is **no `spawn_blocking`
anywhere in the file** (`grep -n "spawn_blocking" src/agent/asset_refresh.rs` → no
matches).

**Failure scenario:** a project asset/skill/agent discovery walk — directory
traversal plus file reads across several roots — occupies a tokio worker thread for
its full duration instead of being offloaded. On a network filesystem, a cloud mount,
or a large tree this can be hundreds of milliseconds, delaying every other task
scheduled on that worker (TUI event handling, server WebSocket delivery, agent
turn progress). The risk scales with the number of workers, which is why a test using
`flavor = "multi_thread", worker_threads = 4` is the one that hangs — see the
"Test failures" section.

The coalescing logic itself is sound: `refresh_with_cancellation` uses
`refresh_lock.try_lock()` (`:180`) and falls back to `lock().await`, and `status()`
also uses a non-blocking `try_lock` for `in_flight` (`:326`). I verified there is **no
production deadlock** here. The fix is `tokio::task::spawn_blocking` around the
`build` call.

---

# Optimizations

Performance and efficiency observations, none of which are correctness bugs.

1. **`/search` rescans and re-lowercases the entire transcript on every submit.**
   `messages.rs:1441` allocates a full `to_lowercase()` copy of *every* message part,
   then `find`s in each. This is O(total transcript bytes) in allocation plus scan on
   the UI thread. An incremental index (or lowercasing once per part on append) would
   make repeated searches O(matches). Relevant because transcripts grow unbounded and
   searching is user-facing.

2. **Per-chunk double allocation in the `responses_api` decoder.**
   `responses_api.rs:1471` does `String::from_utf8_lossy(&chunk).to_string()`, which
   allocates a `Cow`, then clones it into a `String`, then `push_str`s it — three
   operations per network chunk. The shared `wire::SharedStreamDecoder` (fixing the
   correctness bug in finding 21 too) also removes the allocation.

3. **`nextest` ci profile caps at 4 test threads on a 14-core machine.**
   `.config/nextest.toml` pins `[profile.ci] test-threads = 4`, chosen for hosted-CI
   parity (4 vCPU runners) after 8 slots caused `scheduler_cancellation` flakes. That
   reasoning is correct for CI but leaves `verify.sh full` using ~29% of local
   parallelism. A separate local profile (or a CPU-count-derived value with the CI job
   overriding it) would cut wall-clock substantially without touching CI.

4. **`verify.sh full` recompiles the workspace three times.**
   `cargo check --all-targets` → `cargo clippy --all-targets` → two `nextest` runs each
   build a distinct profile/feature set. My cold run spent ~7 minutes in `cargo check`
   alone. Hoisting the check stage (or running clippy once with the feature set the
   tests need) would remove a redundant full build.

5. **The whole transcript is lowercased on search even when the query is ASCII.**
   `search()` calls `to_lowercase()` unconditionally; an ASCII-only fast path
   (`str::is_ascii()` → use `to_ascii_lowercase()`, which is allocation-light and
   byte-length-preserving) would both fix finding 7 for the common case and cut the
   allocation cost.

---

# Systemic observations

The most useful signal in this audit is not any individual bug but the **shape** of
the survivors. All 38 defects pass `cargo fmt`, `clippy -D warnings`, and all 18
guards. They cluster into four unguarded classes:

1. **Dead configuration** (findings 10, 11, 26, 27, 28-30). `scripts/check_config_merge_coverage.py`
   proves every field is *merged* into `Config` — it cannot prove any field is *read*.
   Commit `d85ed67b` fixed exactly this class for `skills` (`config.skills` was parsed
   and merged but read by no production path), and eight more instances of the same
   class remain. A read-coverage guard — asserting each declared key has at least one
   production consumer outside the schema/merge layer — would have caught the original
   bug and every instance still standing.

2. **Silent guard failure** (findings 9, 24, 25, 37). Two guards are permanently red on
   correct code; one cannot detect three of the five spellings of the violation it
   exists to prevent; and 23 guards are wired into neither `verify.sh` nor CI, so
   "skipped" and "passed" are indistinguishable. Mutation-testing each guard against an
   injected violation is the only check that distinguishes these states.

3. **Untrusted-input parsers** (findings 1, 2, 3, 7, 19, 21, 22, 23, 31, 33, 34). The
   `egggit` parsers and the SSE decoders treat line-oriented or chunk-oriented input as
   if it were record-oriented. Both `egggit` blame bugs and the log-body spoofing were
   reproduced here in minutes against real `git` output — the test suite passed
   throughout because every existing test uses single-line bodies, one-commit-per-line
   blame, and ASCII search terms.

4. **Cleanup ordering on error paths** (findings 6, 14, 16, 17). The correct
   implementations exist nearby as references — `src/permission/approval.rs:301` for the
   scoped unregister, `src/scheduler/scheduler.rs:988-1005` for the `mark_unschedulable`
   cleanup — which makes these divergences cheap to fix by copying the sibling.

# Areas examined and found clean

Reported so the absence of findings is informative rather than an omission.

- **Generated agents** — `generate_builtin_agents.py --check` passes; `generated.rs`
  matches `assets/agents/*.toml` and `assets/prompts/`.
- **Config merge coverage** — all 12 `merge()` impls verified to cover 100% of their
  struct fields, plus the hand-inlined `search` (14/14), `watcher` (2/2), `provider`,
  `agent`, `mcp`, `commands`, `mode`, `model_profile` arms. The prior "dropped section"
  bug class is genuinely closed.
- **17 built-in provider registration** — `register_builtin_with_config` registers all
  17, matching `builtin_registration_order()`. Defining one provider in config does not
  disable env-var auto-registration for the others; the `registry.list().is_empty()`
  fallback at `provider_core.rs:1088` is correctly a redundant net.
- **Storage layout invariant** — `STORAGE_LAYOUT_VERSION = 68` (`storage/mod.rs:41`)
  equals the highest wired migration; the chain is contiguous 1..=68 with every dispatch
  arm mapped. `migrate_and_record` is properly transactional.
- **Server auth** — `validate_token` uses `subtle::ConstantTimeEq`; all three WS upgrades
  route through `validate_ws_auth`; fail-closed (503 with no credential, 401 otherwise).
  `CODEGG_SERVER_AUTH_DISABLED` requires exact `1`/`true`.
- **Server streaming** — every channel is bounded (no `mpsc::unbounded_channel` anywhere
  in the workspace); `limit_ws` sets 4 MiB message and frame caps on all three upgrades;
  `critical_send` surfaces `QueueFull`/`WriterClosed`; `select!` blocks are `biased` with
  cancellation first.
- **Path traversal** — `sanitize_path_from_root` canonicalizes the root, rejects `..`
  escapes, and validates component-by-component with `check_path_for_symlinks`.
- **Rate limiting** — both limiters bound their key maps (`MAX_RATE_LIMITER_KEYS`,
  `MAX_WS_RATE_LIMITER_KEYS`) with tests.
- **`egglsp` position conversion** — UTF-16/UTF-32/UTF-8 ↔ byte conversion, CRLF
  handling, and surrogate-split rejection verified correct with round-trip tests.
- **`egglsp` transport** — `Content-Length` bounds-checked, malformed headers rejected
  rather than skipped, stdout/stderr on separate tasks (no pipe deadlock),
  `kill_on_drop(true)`.
- **`codegg-git` safety** — `risk_classes` gates destructive variants; `RepoPath::new`
  blocks `..`; `render_argv` emits `--` before paths (no argument injection);
  `check_git_forbidden_patterns.py` passes with 0 findings.
- **`codegg-document` transaction core** — inverse-range math correct, `validate()` runs
  before any mutation, revision counter saturates via `checked_add`.
- **`eggsentry` regexes** — all patterns anchored or character-class bounded; no nested
  quantifiers or ambiguous alternation, so no ReDoS on untrusted input.
- **Agent loop** — `turn_completion.rs:451` enforces `max_turns`; `execute_tool_calls_impl`
  assigns a result per call including denied/rejected (the pairing invariant holds);
  `decide_tool_retry` correctly refuses auto-retry for `NonIdempotent`/`ProcessExec`
  post-dispatch.
- **`install.sh`** — `set -eu`; strict checksum verification (64-char lowercase hex,
  two-column form, rejects `..`, `/`, `\`, leading `-`, embedded whitespace); symlink and
  `..` archive-member rejection; rollback paths guarded; every `rm -rf` operand
  non-empty-guarded; temp dirs cleaned via trap.
- **Desktop IPC** — every `#[tauri::command]` return derives `Serialize` with
  `camelCase`; frontend `invoke()` argument names match Rust parameters 1:1 across all 19
  commands; `capabilities/main.json` grants `permissions: []`, so no Tauri plugin command
  is reachable from the webview.
- **No lock-across-await** — a syntactic sweep of every `async fn` in `src/` and `crates/`
  found zero cases of a lock guard held across an `.await` point.

# Test failures

Four test runs were executed. **The test suite is functionally green — 12299/12299
tests pass.** Two defects surfaced in the test layer itself, both real and both
actionable.

| Run | Command | Result | Exit |
|---|---|---|---|
| 1 | `nextest run --workspace --profile ci` (default fail-fast) | 5606/12299 run, 5605 passed, **1 TIMEOUT**, 5 skipped, 6693 **not run** | 100 |
| 2 | `nextest run -p codegg --features server,plugins,lsp-test-support --profile ci` | 6466/9439 run, 6465 passed, **1 TIMEOUT**, 5 skipped | 100 |
| 3 | `cargo test --lib --exact <the timing-out test>` ×6 | **6/6 passed**, 0.00–0.16 s each | 0 |
| 4 | `nextest run --workspace --profile ci --no-fail-fast` | **12299/12299 passed**, 5 skipped, **1 LEAK**, 290 s | 0 |

## 40. Flaky test: `same_scope_requests_coalesce_to_one_publication` hangs under load

`src/agent/asset_refresh.rs:720`

It is the **only** test to fail in this audit — and it failed in *both* fail-fast runs
while passing 6/6 in isolation and passing in the full 12299-test sweep.

```
TIMEOUT [ 120.004s] codegg agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication
```

This is a hang, not slowness: the test completes in **0.00 s** in isolation, so it cannot
"get slower" — under CPU contention it fails to make progress at all.

**Root cause.** The test is `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`
(`:720`) and its mock builder blocks a runtime worker with `std::thread::park()`
(`:595`). Its wait loop is a bounded 20-iteration spin on
`tokio::task::yield_now()` (`:754-759`). Both are scheduling-dependent, so when the
4-worker runtime cannot get scheduled the unpark path is never reached and the test
hangs until nextest's slow-timeout kills it.

**Production code is not implicated.** I verified the coalescing path is sound:
`refresh_with_cancellation` uses `refresh_lock.try_lock()` (`:180`) falling back to
`lock().await`, and `status()` also uses a non-blocking `try_lock` for `in_flight`
(`:326`). There is no production deadlock here.

**Fix.** Two independent options, both cheap:
1. Add this test to the `threads-required = "num-cpus"` override list in
   `.config/nextest.toml`. That list already exists for exactly this failure mode —
   its comment records that `scheduler_cancellation` "had real-`sleep` token assertions
   [that] flaked twice under parallel load … while passing serially". This test lives
   in the `codegg` lib binary and is **not** covered by that override.
2. Replace the `yield_now()` spin and the blocking `park()` with a `tokio::sync::Notify`
   or a `oneshot` channel so the test never depends on worker scheduling.

**Caveat on attribution.** During runs 1 and 2 this machine was concurrently executing
`cargo install youtube-tui` and `cargo test -p eggsec` (a different repo), competing for
all 14 cores. I could not re-run the suite on an idle machine, so I cannot separate
"inherently flaky" from "flaky only under heavy external load". Run 4 was also under load
yet passed, which is consistent with a race rather than a deterministic failure. Either
way, a test that hangs for 120 s when the box is busy is a defect in the test.

## 41. Leaky test: `test_research_subagent_registry_includes_websearch_and_research`

```
LEAK [ 0.246s] (91/12299) codegg agent::definition::tests::test_research_subagent_registry_includes_websearch_and_research
```

nextest reports LEAK when a test process does not shut down within its grace period.
The test body is trivial — `src/agent/definition.rs:939-964` only builds
`ToolRegistry::with_defaults()` and asserts that `websearch`, `research`, and `webfetch`
survive a filter. It spawns nothing itself, so the non-clean shutdown originates in
registry construction.

`ToolRegistry` has **no `Drop` impl and no `shutdown`/`close` method**
(`grep -n "impl Drop for ToolRegistry\|fn shutdown\|fn close" src/tool/mod.rs` → no
matches), and **85 call sites** construct it via `ToolRegistry::with_defaults()`.

**Failure scenario:** in the long-lived daemon this is fine by design, but anything that
creates and drops a registry repeatedly — a test, an embedded host, a reconnect path —
accumulates whatever background resources construction started. I did not isolate which
resource (the deterministic-tool path at `src/tool/mod.rs:929` constructs an
`EggsactRuntime`, though it is gated behind `deterministic_config.enabled`, which is off
by default). Worth an explicit teardown path on the registry regardless of which
resource is responsible.

---

**Everything else passes.** All 12299 workspace tests pass, including the 215 integration
test files in `tests/`, all 11 member crates, and the feature-gated `server`, `plugins`,
and `lsp-test-support` code paths. `cargo fmt --check`, `clippy --workspace --all-targets
-D warnings`, `cargo check --workspace --all-targets`, and all 18 wired guards are clean.

**The five skipped tests are intentional**, not failures: four are opt-in asset
regenerators (`m002_regenerate_checked_in_receipt`, `m003_regenerate_checked_in_receipt`,
`generate_derived_asset`, `print_bm25_frontier` — all `#[ignore]`d by design and
documented as requiring explicit review) plus one local real-process smoke test
(`eggsearch_real_compat.rs:40`).

# A note on what this means

The suite is well-built and the guards are mostly effective — `check_websocket_bounds`,
`check_scheduler_bypass`, `check_identity_path_usage`, and `check-desktop-boundary` all
demonstrably catch injected violations. But **every one of the 39 product defects in
this report is invisible to it.** That is the actionable takeaway: the repo's
verification investment is concentrated on *architectural boundary* violations
(who may call what, which crate may import which), which is well covered, while
*behavioural* defects inside a correctly-layered module — a parser that trusts
line-orientation, a cleanup call with the wrong key, a documented config key nobody
reads — pass silently. Closing the four classes in "Systemic observations" would catch
the large majority of what is documented here.

