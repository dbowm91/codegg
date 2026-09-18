# Self-Contained Installation M002 — Runtime Resolution and Clean-Host Qualification

Status: ready (unblocked by M001 closure at
plans/closure/self-contained-installation-corrective/001-status.md;
Provider /connect Restoration M003 precondition closed at
plans/closure/provider-connect-restoration-corrective/003-status.md)

Corrective roadmap:
plans/subsystems/self-contained-installation-corrective-addendum.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee
Upstream eggsearch reviewed: 0.3.9
Upstream eggsact reviewed: 1.2.5

## 1. Objective

Make runtime resolution consume the installation-owned runfiles from M001 and prove
the complete basic-user path on a host/profile that has none of CodeGG's implementation
helpers preinstalled.

## 2. Managed eggsearch resolution

Current `bootstrap_eggsearch` calls `egg_cfg.command()`, whose default is
`eggsearch`, then asks the MCP service to spawn it through normal executable
resolution.

Change the default resolution contract:

1. explicit `[mcp.eggsearch]` remains authoritative when configured;
2. explicit `[search.eggsearch].command` remains an advanced override;
3. otherwise resolve canonical sibling `codegg-eggsearch` relative to the
   canonical running `codegg` executable;
4. retain PATH `eggsearch` only as an explicitly documented legacy/source-build
   compatibility fallback if needed. A prebuilt release test must not depend on it.

Distinguish “field absent” from “field defaulted to eggsearch” in config accessors so
the runtime can tell an explicit override from the managed default.

Use a small installation-runfile resolver shared where sensible with sandbox helper
resolution, while retaining the sandbox helper's stricter trust checks. Do not make cwd
or inherited helper-specific environment variables authoritative.

## 3. Version/tool-surface checks

At bootstrap/doctor time, record whether the managed sidecar is present and its
reported version. The existing MCP initialize/tool discovery remains the executable
compatibility authority.

Required eggsearch tools remain at least `web_search` and `web_fetch`; recommended
tools remain the current repo/search/security/research/batch/evidence set. For the
bundled pinned version, closure expects the complete current ten-tool upstream surface.

A missing/corrupt managed sidecar must produce an installation-specific actionable
diagnostic, not “install eggsearch” as the primary remedy.

Keep normal startup non-fatal if search cannot initialize; do not convert optional
search degradation into a CodeGG process crash.

## 4. Sandbox helper qualification

Exercise the installed `codegg-sandbox-helper` through the real trusted sibling
resolver. On Linux with Landlock available, run a deterministic default/workspace
sandbox smoke and assert the helper status channel reports enforcement.

On Unix where the selected sandbox backend is unsupported, verify the existing
documented permission/fallback behavior and distinguish unsupported kernel/platform
from missing installation runfile.

Do not introduce PATH fallback for the sandbox helper.

## 5. Eggsact qualification

Add or retain a static/runtime assertion showing deterministic tools are backed by the
in-process eggsact 1.2.5 library path. A clean-host test must have no `eggsact`
executable and still expose the CodeGG curated deterministic tool set.

Do not expose all upstream 86 tools merely to prove installation. Preserve CodeGG's
curated/progressive-disclosure surface.

## 6. Doctor/install diagnostics

Extend doctor internals with an installation/runfile report:

- CodeGG executable location/version;
- expected sibling names and present/missing/invalid state;
- managed eggsearch version + MCP tool-coverage result;
- sandbox helper resolvability and supported-kernel probe;
- eggsact in-process version/contract indication if practical.

Never print credentials, master keys, auth headers or secret-bearing config.

The CLI command-surface corrective plan may change the user-facing doctor syntax; this
milestone owns the diagnostic data, not that syntax.

## 7. Clean-host acceptance harness

Add a reproducible VM/container/temporary-home qualification path for the supported
release artifact. The fixture must ensure:

- no Rust/Cargo on PATH for the runtime smoke;
- no `eggsearch` or `eggsact` executable on PATH;
- no `CODEGG_MASTER_KEY`, `CODEGG_ENCRYPTION_KEY` or
  `OPENCODE_ENCRYPTION_KEY`;
- no pre-existing CodeGG config/credentials;
- only the packaged CodeGG installation and ordinary OS runtime facilities.

Test:

1. install the verified release artifact;
2. `codegg --version`;
3. installation/search doctor;
4. launch the TUI;
5. use the restored `/connect` path with a deterministic local fake provider and
   create a credential through M001;
6. confirm the connection/models projection;
7. exercise one eggsearch wrapper through the managed sidecar against deterministic
   fixtures or an isolated compatible test configuration;
8. exercise one in-process eggsact deterministic tool;
9. on supported Linux, execute one sandboxed command through the packaged helper.

The harness may use fixture servers supplied by the test environment; the installed
CodeGG process itself must not rely on a separately installed product/helper.

## 8. External dependencies and documentation

After closure document the actual basic-use dependencies precisely:

Required for hosted-provider operation:

- network connectivity to the selected upstream;
- valid provider credentials.

Not required merely to launch/connect/use the packaged core:

- Rust/Cargo;
- separately installed eggsearch;
- separately installed eggsact;
- manual CodeGG master-key environment variable;
- Python.

Git is required only for Git-backed repository operations that need it; LSP servers
are required only for language-server features. Do not classify optional workflow
tools as installation prerequisites.

The bootstrap installer's downloader/shell requirements remain installation-time
requirements and must be documented separately from runtime dependencies.

## 9. Regression guards

- config test proving absent eggsearch command resolves managed sidecar while explicit
  override remains explicit;
- packaged-path test with PATH stripped of eggsearch;
- trusted sandbox helper sibling test from installed layout;
- static scan/no-process test for eggsact;
- docs test/grep preventing reintroduction of “install eggsearch separately” in the
  supported prebuilt quick start once release assets qualify.

## 10. Verification and closure

Run the focused search backend/fake-MCP suites, deterministic-tool tests, sandbox
tests, release installer tests, strict workspace Clippy, formatting and
`scripts/verify.sh quick`.

Closure evidence must include the exact artifact/version used by the clean-host smoke
and the observed absence of Rust, external eggsearch/eggsact and master-key env vars.

Do not close solely from unit tests. The cross-subsystem installed-layout smoke is the
specific verification missing from the predecessor roadmaps.
