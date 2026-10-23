# LSP Semantic Disk Cache — Privacy & Threat Model

> Phase 16 · Workstream 2
>
> **Note:** This is a prospective design document. The current
> `LspCacheMode` enum supports `Disabled` and `Memory` only; disk
> persistence (`"disk"` mode) has not been implemented.
>
> **Snapshot:** Prospective design from Phase 16. Its *design* claims
> (recommendations, `schema_version`, encryption-at-rest) remain
> unimplemented and are stated prospectively below; they are not
> stale. Its *measured* defaults and API claims were re-verified
> against current source on 2026-10-23 — see "Source Verification"
> at the end.

## Risk Summary

| # | Threat | Likelihood | Impact | Mitigation Status |
|---|--------|-----------|--------|-------------------|
| 1 | Stale evidence served as fresh | Low | Medium | Mitigated (TTL, file hash, generation checks) |
| 2 | Cross-workspace information leak | Low | High | Mitigated (root-scoped keys, no cross-root lookup) |
| 3 | Plaintext source snippets on disk | Medium | High | **Requires mitigation** before disk persistence ships |
| 4 | Schema version downgrade attack | Low | Medium | Designed in (not implemented — no `schema_version` field exists today) |
| 5 | Cache poisoning via file modification | Low | Medium | Mitigated (file hash validation) |
| 6 | Unbounded disk growth | Low | Low | Mitigated (max_entries/max_bytes caps) |
| 7 | Secrets leaked from cached source | Medium | High | **Requires mitigation** before disk persistence ships |
| 8 | Cache directory accessible to other users | Low | High | Mitigated by OS permissions; platform-dependent |

---

## 1. What Would Be Stored

Disk persistence would serialize `LspContextPacket` entries. Each packet contains:

**Evidence items** (`Vec<LspContextItem>`):
- `kind` — evidence type (Diagnostic, Definition, Reference, Hover, etc.)
- `file` — absolute `PathBuf` to the source file
- `range` / `line` / `column` — location within the file
- `message` — human-readable text (diagnostic messages, hover content, completion text)
- `symbol` — optional symbol name
- `source` — optional `AgentContextSource` (where in the agent workflow the evidence originated)
- `provenance` — server ID, generation, operation name, freshness timestamp
- `score` — `LspContextScore`, the ranking metadata used to order items
- `payload` — optional `serde_json::Value` carrying structured payload for complex evidence types

Every one of these types is `Serialize`/`Deserialize` today (see
§10 "Current state"), so the list above is exactly what a disk
persistence layer would put on disk.

**Preview artifacts** (`Vec<LspPreviewArtifact>`):
- Unified diff patches with file paths and patch text
- Preview registry IDs

**Metadata**:
- `workspace_root` — absolute `PathBuf`
- `server_id`, `server_generation`
- `request` — serialized `LspContextRequest` (includes changed file paths, hunk descriptors)
- `budget` — the `LspContextBudget` used during collection
- `truncation` — what was dropped and why
- `notes` — operational notes (e.g., "LSP state: indexing")

**Cache keys** additionally contain:
- `input_hashes` — `BTreeMap<PathBuf, String>` mapping absolute file paths to SHA-256 content hashes
- `request_fingerprint` — serialized request JSON
- `capability_fingerprint`, `budget_fingerprint` — serialized config snapshots

---

## 2. Storage Location

Following the existing `egglsp::download::cache_dir()` pattern, disk cache entries would live under:

```
$CACHE_DIR/codegg/lsp-semantic-cache/<workspace_root_hash>/
```

Where `$CACHE_DIR` is `dirs::cache_dir()` (e.g., `~/Library/Caches` on macOS, `~/.cache` on Linux).

Each workspace root gets a subdirectory identified by a normalized hash of its canonical path, preventing path-traversal via symlinks or `..` components.

---

## 3. Who Can Read It Locally

- **Same-user processes**: Any process running under the same UID can read the cache directory. This includes other terminals, IDE extensions, CI scripts, or malware running as the user.
- **Disk encryption**: Varies by platform. macOS APFS encryption is enabled by default on modern hardware; Linux encryption is user-configured and not guaranteed. Windows BitLocker is edition-dependent.
- **Backup systems**: Time Machine, cloud sync (iCloud, Dropbox), or other backup tools may copy cache files off-machine.

---

## 4. How It Is Cleared

| Trigger | Behavior |
|---------|----------|
| `/lsp-cache-clear` (no argument) | Removes **every** cache entry — the in-memory equivalent of `--all` |
| `/lsp-cache-clear --all` | Same as the no-argument form: clears the whole cache |
| `/lsp-cache-clear <path>` | Removes only the entries for that workspace root (`clear_semantic_cache_for_root`) |
| TTL expiry | Entries older than `ttl_seconds` (default 300s) are dropped on read: `LspSemanticCache::get` finds the entry expired, counts a stale miss, and removes it from the map |
| Schema version mismatch | **Not implemented.** There is no `schema_version` field on the key or the packet today; see §8 for the designed-in rule |
| App uninstall | Depends on whether cache dir is in an app-specific location; `dirs::cache_dir()` is shared, so uninstall alone may not clear it |
| Manual deletion | User can `rm -rf` the cache directory at any time |

The clear commands are TUI slash commands dispatched through
`BuiltinSlashAction::LspCacheClear` / `LspCacheStatus`
(`src/tui/app/mod.rs`), and they act on the in-memory cache only. There
is no disk directory to remove today.

**Gap**: There is no proactive background cleanup of expired entries. Stale
entries are removed when they are read or when a clear command runs; nothing
sweeps the cache on a timer.

---

## 5. Workspace Root Scoping

- Every cache key includes `workspace_root: PathBuf`.
- Lookups are scoped: a key for `/workspace_a` will never match an entry for `/workspace_b`.
- `clear_for_root()` removes only entries for a specific root.
- **For disk persistence**: The workspace root should be normalized to its canonical form (`std::fs::canonicalize()`) and then hashed (SHA-256) to form the subdirectory name. This prevents:
  - Symlink-based path traversal
  - `..` or `.` components
  - Platform-specific path casing differences (case-insensitive macOS/Windows)
  - Different representations of the same path (trailing slashes, etc.)

---

## 6. Path Handling

**Current state**: Absolute paths are stored in cache keys (`workspace_root`, `input_hashes` keys, `LspContextItem.file`).

**For disk persistence, paths must be handled carefully**:

- **Cache key directory**: Use SHA-256 of the canonical workspace root. Never store the raw path in the directory name.
- **Serialized entry fields**: `workspace_root` and `LspContextItem.file` contain absolute paths. Options:
  1. **Strip prefix**: Store paths relative to workspace root. Reconstruct on read. Loses information for files outside the root.
  2. **Hash paths**: Store `SHA-256(absolute_path)` alongside relative paths. Enables integrity checks without leaking location.
  3. **Keep absolute but encrypt**: If disk confidentiality is a requirement, encrypt at rest (adds complexity).
- **Recommendation**: Store paths relative to workspace root for evidence items. Keep absolute paths in the cache key only (key is not persisted to disk in cleartext).

---

## 7. Source Text Snippets

`LspContextItem.message` and `LspContextItem.payload` contain:

- **Diagnostic messages**: Often contain file paths, variable names, and type information.
- **Hover content**: May include full function signatures, doc comments, and code examples.
- **Completion text**: Snippets of actual source code.
- **Symbol documentation**: Docstrings, which may reference internal APIs, credentials, or architecture details.

These are stored as **plaintext UTF-8** in the serialized packet. On disk, this means:

- Source code snippets are directly readable by any process with file access.
- Diagnostic messages may reveal security vulnerabilities (e.g., "insecure binding on port 8080").
- Hover content for configuration files may surface credential patterns.

**This is the primary privacy risk of disk caching.**

---

## 8. Schema/Version Handling

Every serialized entry should include a `schema_version: u32` field. On deserialization:

1. If `schema_version` < current → **drop the entry** (backward-incompatible change).
2. If `schema_version` > current → **drop the entry** (forward-incompatible; running older code).
3. If `schema_version` == current → proceed with validation.

**Rationale**: Prevents a version-downgrade attack where an attacker substitutes a crafted cache file from an older (less restrictive) schema version, potentially bypassing integrity checks or injecting stale data that would otherwise be caught.

---

## 9. Secrets in Source

Cached evidence can inadvertently contain:

- API keys embedded in source code (`sk-...`, `AKIA...`)
- Database connection strings (`postgres://user:password@host`)
- Hardcoded passwords, tokens, or certificates
- Internal infrastructure details (hostnames, ports, environment variables)
- Comments referencing security practices or vulnerabilities

**Mitigation strategies** (recommended in combination):

1. **Encryption at rest**: Encrypt the cache directory using the platform keychain (macOS Keychain, Linux kernel keyring, Windows DPAPI). This is the strongest mitigation but adds significant complexity.
2. **Content filtering**: Run a lightweight secrets scanner (similar to `eggsentry`) on evidence before caching. Strip or redact items containing known secret patterns.
3. **Opt-in only**: Never enable disk persistence without explicit user action. The default must remain `disabled`.
4. **Short TTL**: Even on disk, enforce aggressive TTLs (e.g., 5 minutes). Limits the window during which secrets are exposed.
5. **Relative paths only**: Never persist absolute filesystem paths, which leak directory structure.

---

## 10. How Users Disable It

- Default config: `mode = "disabled"` — no cache writes occur at all.
- The config surface is the **top-level** `lsp_semantic_cache` object in the
  JSON/JSONC config (`codegg.jsonc` / `codegg.json` / `config.json` — this
  project's config format is JSON, not TOML):

  ```jsonc
  "lsp_semantic_cache": {
    "mode": "disabled",   // "disabled" (default) | "memory"
    "max_entries": 64,
    "max_bytes": 4194304,
    "ttl_seconds": 300
  }
  ```

- The mode mapping is **fail-closed**: `convert_lsp_cache_config`
  (`src/tool/mod.rs`) recognizes exactly `"memory"` and maps every other
  value — including a hypothetical `"disk"` — to `LspCacheMode::Disabled`.
  A config typo therefore disables the cache rather than selecting an
  unimplemented mode.
- The cache never enables itself. No auto-upgrade from memory to disk mode.
- `/lsp-cache-status` displays the current mode (`memory` or `disabled`),
  entry count, byte size, hits, misses, stale misses, and evictions.
- `/lsp-cache-clear` (and `/lsp-cache-clear --all`) clears the **in-memory**
  cache; `/lsp-cache-clear <path>` clears one workspace root. There are no
  disk artifacts to remove today.

### Current state: what actually exists today

| Property | Status |
|----------|--------|
| `LspCacheMode` variants | `Disabled` (default) and `Memory` only — no `Disk` variant |
| Persistence | In-memory only. `crates/egglsp/src/cache.rs` contains no filesystem write at all |
| Serialization | `LspContextPacket`, `LspContextItem`, `LspCacheKey`, and `LspPreviewArtifact` **all derive `Serialize`/`Deserialize`**, and `crates/egglsp/tests/lsp_cache_benchmark.rs` already round-trips a packet through JSON (`test_packet_serialization_performance`) |
| What is missing | The **store**, not the encoding. A disk mode would be a new persistence layer over an already-serializable packet type |
| Wire path | `LspTool::lsp_cache_config` (`src/tool/mod.rs`) carries an optional `egglsp::cache::LspCacheConfig`; `None` means `LspCacheConfig::default()` (disabled) |

---

## Threat Scenarios

### T1: Stale Evidence Served as Fresh

**Scenario**: A cached `LspContextPacket` is served to the agent after the underlying source files have changed, causing the agent to act on outdated diagnostics or symbol information.

**Mitigations** (already in place):
- TTL expiry (default 300s) on every `get()` call.
- File content hash comparison (`input_hashes` in key vs. current hashes).
- Server generation mismatch detection.

**Residual risk**: Low. The triple-check (TTL + hash + generation) is robust. Disk persistence does not weaken these checks since they are re-evaluated on every cache hit.

---

### T2: Cross-Workspace Information Leak

**Scenario**: Cache entries from workspace A are served to requests targeting workspace B, leaking proprietary code or configuration.

**Mitigations** (already in place):
- `workspace_root` is part of every cache key.
- No cross-root lookup exists in the codebase (verified in `LspSemanticCache::get()`).
- `clear_for_root()` is scoped to a single root.

**Residual risk**: Low. Root-scoping is structurally enforced. Disk persistence would use hashed root subdirectories, adding an additional isolation layer.

---

### T3: Plaintext Source Snippets on Disk

**Scenario**: An attacker with local file access (malware, co-located container, shared machine) reads cached evidence files containing source code, diagnostic messages, or documentation.

**Likelihood**: Medium (depends on platform encryption and deployment context).
**Impact**: High (source code disclosure, intellectual property loss).

**Recommended mitigations**:
1. Encrypt cache files at rest using platform keychain.
2. Redact or filter sensitive content before writing.
3. Use restrictive file permissions (`0600`) on cache files.
4. Document that disk cache is not suitable for shared or untrusted environments.

---

### T4: Schema Version Downgrade Attack

**Scenario**: An attacker replaces a cache file with one crafted for an older schema version, bypassing integrity or security checks added in newer versions.

**Mitigations** (designed in):
- Every entry includes `schema_version`.
- Version mismatch causes silent drop.

**Residual risk**: Low. The version check is simple and hard to bypass. Combined with file hash validation, this is well-mitigated.

---

### T5: Cache Poisoning via File Modification

**Scenario**: An attacker modifies a cached entry to inject stale or incorrect evidence, causing the agent to make wrong decisions.

**Mitigations** (already in place for memory; would carry to disk):
- File content hashes (`input_hashes`) are validated on every cache hit. If the source file changed, the entry is dropped.
- Server generation is checked. A restart invalidates all cached entries.
- TTL expiry provides a time-bounded window.

**Residual risk**: Low. An attacker who can modify cache files can likely also modify source files, making cache poisoning redundant.

---

### T6: Unbounded Disk Growth

**Scenario**: Disk cache grows without bound, consuming storage.

**Mitigations** (designed in):
- `max_entries` cap (default 64).
- `max_bytes` cap (default 4MB).
- LRU eviction within caps.

**Residual risk**: Low. Caps are enforced at insert time. For disk persistence, a background cleanup of expired entries would be advisable.

---

### T7: Secrets Leaked from Cached Source

**Scenario**: Cached evidence contains API keys, passwords, or connection strings from source code. An attacker reads the cache file.

**Likelihood**: Medium (depends on whether the user's codebase contains inline secrets).
**Impact**: High (credential exposure).

**Recommended mitigations**:
1. Run `eggsentry`-style secret scanning on evidence before caching. Drop or redact matches.
2. Document the risk prominently in user-facing config documentation.
3. Consider a "safe mode" that only caches non-text evidence (diagnostics without messages, symbol names without hover content).

---

### T8: Cache Directory Accessible to Other Users

**Scenario**: On a multi-user system, another user reads the cache directory.

**Mitigations**:
- `dirs::cache_dir()` typically resolves to user-specific directories (`~/Library/Caches`, `~/.cache/username`).
- OS file permissions restrict access to the owning user by default.

**Residual risk**: Low on standard configurations. High on misconfigured shared systems. Mitigated by using `0700` for the cache directory and `0600` for files.

---

## Recommendations

| Priority | Recommendation | Rationale |
|----------|---------------|-----------|
| **P0** | Never enable disk cache without explicit user opt-in | Prevents accidental exposure |
| **P0** | Use `0600`/`0700` file permissions on cache directory and files | Limits local exposure |
| **P1** | Encrypt cache at rest using platform keychain | Strongest mitigation for T3/T7 |
| **P1** | Filter/redact evidence containing secret patterns before caching | Reduces payload sensitivity |
| **P1** | Store paths relative to workspace root, not absolute | Prevents directory structure leakage |
| **P2** | Proactive cleanup of expired disk entries | Prevents unbounded growth from stale files |
| **P2** | Document that disk cache is not suitable for shared/untrusted environments | User awareness |
| **P3** | Consider a "safe mode" that excludes text-heavy evidence | Reduces attack surface |

---

## Decision Point

Disk persistence of the LSP semantic cache is **not recommended** without at least:

1. Encryption at rest (P1).
2. Content filtering for secrets (P1).
3. Explicit opt-in with prominent warning (P0).

The in-memory-only cache (`mode = "memory"`) has acceptable privacy characteristics: no data persists after process exit, no disk exposure, and all integrity checks (TTL, hash, generation) are enforced. Disk persistence trades privacy for performance, and the trade-off should be user-driven, not default.

---

## Source Verification

Re-verified 2026-10-23 against `crates/egglsp/src/cache.rs`,
`crates/egglsp/src/context.rs`, `crates/egglsp/src/download.rs`,
`crates/egglsp/tests/lsp_cache_benchmark.rs`,
`crates/codegg-config/src/schema.rs`, `crates/codegg-config/src/paths.rs`,
`src/tool/mod.rs`, `src/tool/lsp.rs`, and `src/tui/command.rs`.
This document's design/forward-looking claims are prospective by intent;
only the measured claims below were checked.

**Corrections made (2026-10-23).**

- **§4, clear-command behavior.** The previous revision said
  `/lsp-cache-clear` "removes all entries for a specific workspace root".
  The bare form clears the **whole** cache: the TUI handler
  (`BuiltinSlashAction::LspCacheClear` in `src/tui/app/mod.rs`) treats an
  empty argument and `--all` identically and calls
  `LspTool::clear_semantic_cache`. The root-scoped form is
  `/lsp-cache-clear <path>`, which calls
  `LspTool::clear_semantic_cache_for_root` →
  `LspSemanticCache::clear_for_root`.
- **§4, TTL row.** Entries expire on read, not on a timer: a read of an
  expired entry counts a stale miss and removes the entry from the map.
  There is no proactive sweep.
- **§4, schema-version row.** There is no `schema_version` field on
  `LspCacheKey` or `LspContextPacket` today; the row now says the rule is
  **not implemented**, and Risk Summary row 4 now reads "Designed in
  (not implemented)" instead of "Mitigated".
- **§10, config surface.** The config format for this project is
  **JSON/JSONC** (`codegg.jsonc` / `codegg.json` / `config.json` per
  `crates/codegg-config/src/paths.rs`), so the previous `[lsp_semantic_cache]`
  TOML-table syntax was wrong; the section now shows the JSONC object and
  names the key as the **top-level** `lsp_semantic_cache` field on `Config`
  (`crates/codegg-config/src/schema.rs`). Added the fail-closed mode
  mapping in `convert_lsp_cache_config` (`src/tool/mod.rs`): only
  `"memory"` is recognized, and every other value maps to
  `LspCacheMode::Disabled`.
- **§10, status output.** `/lsp-cache-status` reports mode, entries,
  bytes, hits, misses, stale misses, and evictions — not mode and entry
  count alone.
- **§1, evidence-item fields.** `LspContextItem` also carries `source`
  (`Option<AgentContextSource>`) and `score` (`LspContextScore`); both
  are listed now.
- **Added §10 "Current state".** The previous revision did not record
  that `LspContextPacket`, `LspContextItem`, `LspCacheKey`, and
  `LspPreviewArtifact` all derive `Serialize`/`Deserialize`
  (`crates/egglsp/src/context.rs`, `crates/egglsp/src/cache.rs`), and that
  `crates/egglsp/tests/lsp_cache_benchmark.rs` already round-trips a
  packet through JSON (`test_packet_serialization_performance`). The
  missing piece in a disk mode is therefore the **store**, not the
  encoding — the document now says so explicitly, because "the packet
  types are not serializable" would be the wrong reason to believe disk
  persistence is blocked.

**Confirmed accurate, no change needed.**

- `LspCacheMode` (`crates/egglsp/src/cache.rs`) still has exactly 2
  variants, `Disabled` (default) and `Memory` — no `Disk` variant has
  been added, so the document's central premise still holds.
- Cache defaults are exactly as documented
  (`LspCacheConfig::default` in `crates/egglsp/src/cache.rs`):
  `max_entries: 64`, `max_bytes: 4 * 1024 * 1024` (4 MiB),
  `ttl_seconds: 300`.
- `max_entries`/`max_bytes` enforcement is LRU-style, evicting oldest
  entries (`LspSemanticCache::evict_if_needed`).
- `clear_for_root(root)` exists and is scoped to one root;
  `test_clear_for_root_does_not_affect_other_roots` asserts no cross-root
  effect.
- Cache keys carry `workspace_root: PathBuf` and
  `input_hashes: BTreeMap<PathBuf, String>`, and key equality compares
  both. The key carries **no** workspace-content hash, no schema version,
  and no requesting principal — as documented.
- `egglsp::download::cache_dir()` exists
  (`crates/egglsp/src/download.rs`) and resolves to
  `dirs::cache_dir()/codegg/lsp`, matching the storage-location pattern
  in section 2. That directory holds **downloaded LSP server binaries**,
  not semantic-cache entries.
- `crates/egglsp/src/cache.rs` contains no filesystem write: the module
  has no `fs::write`/`File::create`, so nothing persists today.
- `LspPreviewArtifact` carries unified diff patches
  (`patches: Vec<PreviewFilePatch>`), as section 1 states.
- `LspContextItem.payload` is `Option<serde_json::Value>`, as section 7
  states.
- The `/lsp-cache-clear` and `/lsp-cache-status` commands exist and are
  registered in `src/tui/command.rs`
  (`BuiltinSlashAction::LspCacheClear` / `LspCacheStatus`); the previous
  revision's `src/tui/app/mod.rs:5728`/`:12529` line references had
  drifted and are now given as symbols.
