# Self-Contained Installation Corrective Addendum

Status: closed (M001+M002 closed; closure at
plans/closure/self-contained-installation-corrective/002-status.md)

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

Related closed work:

- plans/subsystems/distribution-installation-roadmap.md
- plans/closure/distribution-installation/001-status.md
- plans/closure/distribution-installation/002-status.md
- plans/subsystems/search-eggsearch-integration-roadmap.md
- plans/closure/search-eggsearch-integration/005-status.md
- plans/closure/tool-surface-upstream-compatibility/009-status.md
- plans/closure/runtime-safety-resource-footprint/009-status.md

## 1. Purpose and corrective trigger

Make the supported CodeGG release installation self-contained for the normal terminal
workflow.

The desired user contract is one CodeGG installation, one `codegg` command, and no
separate installation of implementation-owned helpers such as eggsearch, eggsact or
the sandbox helper.

The prior distribution work intentionally standardized a single-executable archive.
Subsequent runtime architecture now requires installation-owned companion binaries,
so that historical artifact invariant is stale.

## 2. Current defects

### Sandbox helper

The root package builds `codegg-sandbox-helper`. Production sandbox execution
resolves that helper only from the canonical sibling directory of the running CodeGG
executable and deliberately refuses PATH/environment substitution.

The release contract, packaging scripts and installer currently require each archive
to contain exactly one top-level executable named `codegg`. A prebuilt Unix install
therefore cannot satisfy the sandbox helper's own trusted installation contract.

### Eggsearch

Eggsearch is CodeGG's default search backend. `SearchConfig` defaults to eggsearch,
with `fallback_to_builtin = false`, and the default stdio command is
`eggsearch mcp stdio`.

CodeGG has no eggsearch Cargo dependency. A fresh install therefore relies on an
independently installed `eggsearch` executable even though search is presented as a
normal CodeGG capability.

Upstream eggsearch 0.3.9 explicitly declares its stable downstream contract to be the
MCP tool surface and CLI; its Rust module tree is application-first and not
general-purpose semver-stable. Bundling a pinned eggsearch sidecar preserves the
stable boundary better than embedding its internal engines today.

### Eggsact

Eggsact is already correct for this product contract. CodeGG depends on eggsact 1.2.5
as a Rust library and invokes its `ToolRegistry` in-process. No eggsact executable or
MCP server is required. This corrective work must retain that behavior rather than
creating another sidecar.

## 3. Target release bundle

Replace the historical single-member archive invariant with an exact per-target
runfile manifest.

For supported Unix release targets the executable set is:

- `codegg`
- `codegg-sandbox-helper`
- `codegg-eggsearch` — the pinned upstream eggsearch executable renamed as a
  CodeGG-owned sidecar name

The archive may also contain the minimum third-party notice/license material required
to redistribute eggsearch, under a fixed allowlisted path. No config, credential,
cache, source tree or arbitrary files are permitted.

Windows packaging must use the corresponding executable suffixes and include only
helpers that the Windows runtime can actually invoke. If Windows remains an optional
artifact without an installer, keep that status explicit rather than pretending Unix
installer guarantees apply.

## 4. Runtime ownership

- CodeGG resolves installation-owned sidecars relative to its canonical executable,
  not from cwd.
- The sandbox helper keeps its current strict trusted-sibling rule.
- Default eggsearch bootstrap resolves `codegg-eggsearch` from the CodeGG
  installation first.
- An explicit user `[search.eggsearch].command` or explicit
  `[mcp.eggsearch]` remains a supported advanced override.
- Legacy PATH eggsearch may remain as a documented compatibility/source-build
  fallback, but a supported prebuilt install must never require it.
- No runtime code shells out to an eggsact executable.

## 5. External-dependency contract after closure

For the normal prebuilt user path, CodeGG itself should require no separately
installed Rust toolchain, eggsearch, eggsact, sandbox-helper package, Python runtime or
master-key environment variable.

Normal hosted-provider use still inherently requires network access and provider
credentials. Git and language servers are feature/workflow dependencies for Git/LSP
operations, not prerequisites for launching CodeGG and connecting to a provider.
Bootstrap installation may use the documented downloader available on the host; that
is distinct from a runtime dependency.

Linux sandbox availability depends on supported kernel/Landlock behavior rather than
an external package. Existing permission/sandbox fallback semantics remain
authoritative.

## 6. Milestones

### M001 — Multi-runfile release artifact and installer contract

Status: closed

Plan:
plans/implementation/self-contained-installation-corrective/001-managed-runfile-release-bundle.md

Closure:
plans/closure/self-contained-installation-corrective/001-status.md
(implementation `f9ec8602`)

Update release packaging, verification, checksums, installer tests and rollback rules
for the exact managed runfile bundle, including pinned eggsearch provenance.

### M002 — Managed runtime resolution and clean-host qualification

Status: closed

Plan:
plans/implementation/self-contained-installation-corrective/002-runtime-resolution-and-clean-host-qualification.md

Closure:
plans/closure/self-contained-installation-corrective/002-status.md
(implementation `cf7a06bb`)

Make default search resolve the bundled sidecar, add installation diagnostics, retain
advanced overrides, and qualify the install → `codegg` → `/connect` path on a
clean host/profile.

## 7. Why predecessor verification missed this

Distribution M001/M002 correctly verified the then-declared single-binary contract.
Runtime Safety M009 correctly verified that the sandbox helper is installation-owned,
but it did not reopen the already-closed release archive contract. Search closure
verified compatibility against a real external eggsearch process and therefore also
did not claim packaging ownership.

Each subsystem passed its local acceptance criteria; no cross-subsystem clean-install
test asserted that all required runtime executables were delivered together. M002
adds that missing control point.

## 8. Exit conditions

- Release archives have an exact reviewed runfile manifest rather than a
  single-member invariant.
- The installer installs/upgrades all runfiles transactionally enough that a failed
  update cannot leave `codegg` paired with an unrelated helper version without an
  explicit diagnostic/recovery path.
- Default sandbox invocation finds its trusted sibling on supported Unix releases.
- Default search finds the installation-owned eggsearch sidecar without PATH setup.
- Eggsact remains in-process.
- A clean-host smoke has no preinstalled eggsearch/eggsact/Rust and no
  `CODEGG_MASTER_KEY`, yet the supported release installs, launches, connects a
  provider and exercises the default search/helper path.

## 9. Non-goals

- Replacing eggsearch's MCP contract with direct dependencies on unstable internal
  modules.
- Reimplementing eggsearch inside CodeGG.
- Bundling Git, language servers, Python or provider credentials.
- Adding a daemon/service for eggsearch when the current shared stdio child lifecycle
  is sufficient.
- Expanding the release target matrix beyond separately approved distribution work.
