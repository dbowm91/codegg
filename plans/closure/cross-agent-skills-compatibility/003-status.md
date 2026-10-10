# Cross-Agent Skill Compatibility M003 — Closure Status

Status: closed

Source plan: `plans/implementation/cross-agent-skills-compatibility/003-package-metadata-and-resources.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: M002 closure at `plans/closure/cross-agent-skills-compatibility/002-status.md`.

Implementation commits: `d0164e95`, `d7a146e1`, `8ef173bf`, `b1e03ce0`.

## 1. Finding

M003 is closed. Portable frontmatter retains unknown vendor fields as inert metadata, Claude may derive a missing name from its package folder, and resource inventory is recursive but bounded and lazy.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Strict portable name/description contract | portable parser fixtures, including blank description rejection | pass |
| Preserve native CodeGG behavior | native Markdown/package tests; empty native description fixture | pass |
| Claude package-folder name fallback | Claude-only missing-name fixture | pass |
| Unknown metadata and tool hints remain inert | unknown extension and `allowed-tools` parser/registry fixtures | pass |
| Nested references are discoverable and bounded | nested resource inventory/handle tests | pass |
| Traversal and symlink escape fail closed | resource boundary integration tests | pass |

## 3. Implementation evidence

Unknown top-level extension keys are retained with bounded diagnostics. `ResourceDescriptor` keeps basename compatibility and includes a relative path for nested packages; the skill tool reports relative resource paths. Inventory caps depth, total entries, and resources, sorts results, and never traverses symlink targets. Resource handles remain lazy and revalidate containment and read bounds.

The package digest includes resource inventory paths, sizes, and modification timestamps while avoiding eager resource-body reads. It is an inventory-change signal, not a content hash of every resource body; resource content remains lazy.

## 4. Verification executed

- `rtk cargo test -p codegg --lib skills::` — 49 passed.
- `rtk cargo test --test skills_registry` — 24 passed.
- `rtk cargo test --test plugin_contributions` — 8 passed.
- `rtk scripts/verify.sh quick` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.

The first registry run exposed that inventory skipped symlink entries expected by the established descriptor contract. Symlink names are now represented with inert zero-size metadata, without target reads; `ResourceHandle` still rejects escapes. The corrected suite passed.

## 5. Adversarial fixtures

Coverage includes malformed YAML, empty and invalid names, blank descriptions, mismatched package directories, oversized frontmatter, unknown extensions, `allowed-tools`, nested relative resources, traversal, invalid UTF-8, size bounds, symlink escape, and script-file inventory without execution.

## 6. Security and authorization

Resource bodies are not read during discovery. Symlink targets are not traversed during inventory; requested reads pass through canonical containment and byte limits. Metadata, scripts, and tool hints cannot grant permissions or trigger processes.

## 7. Runtime and pinning

Skill activation still resolves through the immutable snapshot. Resource inventory changes affect package identity; no active turn's snapshot is mutated. Existing activation/pinning tests passed.

## 8. Platform findings

Linux nested-resource and Unix symlink tests passed. Cross-platform symlink execution was not available. Portable parsing and path normalization use standard Rust path handling; no Windows/macOS result is claimed.

## 9. Documentation

Updated portable naming, Claude fallback, inert metadata, nested-resource limits, and relative-resource descriptions in the architecture and user skill guides.

## 10. Deviations and remaining findings

Resource bodies are intentionally not hashed or read at inventory time; the digest tracks the bounded inventory metadata, preserving lazy reads. This matches the bounded/lazy resource authority and is documented. No unresolved M003 security or compatibility finding remains.

## 11. Dependency and registry disposition

M003 closed after M002 and unblocked M004. M004 and M005 have since closed sequentially. Registry and roadmap record the completed line as closed.

## 12. Final disposition

M003 acceptance criteria are met. Closure accepted.
