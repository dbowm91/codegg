# LSP Semantic Disk Cache — Privacy & Threat Model

> Phase 16 · Workstream 2

## Risk Summary

| # | Threat | Likelihood | Impact | Mitigation Status |
|---|--------|-----------|--------|-------------------|
| 1 | Stale evidence served as fresh | Low | Medium | Mitigated (TTL, file hash, generation checks) |
| 2 | Cross-workspace information leak | Low | High | Mitigated (root-scoped keys, no cross-root lookup) |
| 3 | Plaintext source snippets on disk | Medium | High | **Requires mitigation** before disk persistence ships |
| 4 | Schema version downgrade attack | Low | Medium | Mitigated (version check + drop) |
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
- `provenance` — server ID, generation, operation name, freshness timestamp

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
| `/lsp-cache-clear` | Removes all entries for a specific workspace root |
| `/lsp-cache-clear --all` | Removes the entire cache directory |
| TTL expiry | Entries older than `ttl_seconds` (default 300s) are dropped on read; not proactively cleaned from disk |
| Schema version mismatch | Entry is dropped silently when version does not match current code |
| App uninstall | Depends on whether cache dir is in an app-specific location; `dirs::cache_dir()` is shared, so uninstall alone may not clear it |
| Manual deletion | User can `rm -rf` the cache directory at any time |

**Gap**: There is no proactive background cleanup of expired disk entries. Stale entries persist on disk until accessed or manually cleared.

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

- Default config: `mode = "disabled"` — no disk writes occur.
- To enable: User must explicitly set `mode = "disk"` in `[lsp_semantic_cache]` config.
- The cache never enables itself. No auto-upgrade from memory to disk mode.
- `/lsp-cache-status` displays current mode and entry count.
- `/lsp-cache-clear --all` removes all disk artifacts.

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
