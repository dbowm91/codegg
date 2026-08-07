# Runtime Safety, Resource Control, and Footprint Milestone 006 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/006-deprecated-parser-and-dependency-maintenance.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Accepted implementation and review commits:

- `f3be7db` — replace the deprecated YAML parser boundary
- `bc8e425` — begin M006 closure review
- PR #72 on `planning/runtime-safety-resource-footprint`

## 1. Executive finding

M006 is production-complete and conditionally closed. All production YAML
reads now pass through one format-neutral codec, the deprecated `serde_yaml`
dependency is absent from the active graph, supported compatibility inputs
remain readable, and dependency maintenance is documented as a manual,
bounded process.

Strict closure is conditional only because the accepted pushed revision did
not receive a hosted check run. PR #72 reports no checks, and the repository
workflow has no manual dispatch path. Local full verification is green; no
production or test failure is open. The exact remaining evidence is recorded
in section 10.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Complete YAML usage inventory | `src/agent/mod.rs`, `src/command/mod.rs`, `src/skills/parser.rs`, `src/skills/candidate.rs`, `src/skills/mod.rs`, `crates/codegg-config/src/document.rs`, tests and fixtures | pass | Agent, command, skill, and dynamic metadata reads are classified as compatibility reads; no production YAML writer was found. |
| One parser boundary | `crates/codegg-config/src/document.rs`; `codegg_config::parse_yaml` re-export | pass | Parser-specific APIs and values stop at the codec; callers receive `serde_json::Value` or typed Serde results plus `DocumentParseError`. |
| Direct-import guard | `scripts/check_yaml_parser_boundary.py` | pass | Normal scan and `--self-test` both pass; direct parser names are permitted only in the codec and focused tests. |
| Maintained parser selection | `serde_norway = 0.9.42` | pass | The project is a public, non-archived Serde-compatible fork. RustSec identifies `serde_yml` as unsound/unmaintained and recommends maintained alternatives. |
| Supported YAML compatibility | Codec, agent, command, skills, and registry tests | pass | Mappings, sequences, scalars, nested metadata, multiline strings, and existing frontmatter structures load successfully. |
| Typed malformed-input diagnostics | Codec and loader tests | pass | Malformed YAML preserves source name and line/column where available; invalid UTF-8, oversized input, and multi-document streams are rejected without panic. |
| Duplicate-key behavior | `document` codec test | pass | The replacement’s observed behavior is explicit last-value-wins compatibility; no new stricter rejection was introduced. |
| Reload and last-known-good behavior | Config watcher/load-path review plus full existing reload tests | pass | Parsing happens before merged configuration publication; failures return diagnostics without partial replacement of the in-memory configuration. |
| No unintended rewrite | YAML call-site inventory and write-path review | pass | YAML remains read-only compatibility input. Existing generated or rewritten formats remain TOML/JSON5/JSON. |
| TOML/JSON5 stability | `codegg-config` tests and full workspace verification | pass | No TOML/JSON5 parser or writer behavior was changed. |
| Manual dependency maintenance | `docs/dependency-maintenance.md` | pass | The procedure is periodic/manual and adds no bot, scheduled workflow, matrix, or release automation. |

### Parser decision and rejected alternatives

The selected parser is `serde_norway 0.9.42`, owned by the `codegg-config`
dependency boundary. It retains the required Serde YAML API and dynamic
`Value` model, supports the repository MSRV policy (Rust 1.81+), and avoids a
native runtime dependency. Its upstream repository is public and non-archived:
<https://github.com/cafkafk/serde-norway>. The crate documentation records the
0.9.42 release and Rust 1.71.1 minimum:
<https://docs.rs/crate/serde_norway/0.9.42>.

The RustSec advisory for the removed `serde_yml` line records the unsoundness
and unmaintained status that motivated this work:
<https://rustsec.org/advisories/RUSTSEC-2025-0068>.

`serde_yml` was rejected because its upstream repository is archived.
`serde_yaml_ng` was not selected because the RustSec advisory identifies its
continued use of the unmaintained `unsafe-libyaml` line. `serde-saphyr` was
not selected because the current release requires the post-policy 2024
edition and does not provide the dynamic YAML DOM required by existing skill
and frontmatter metadata paths. `noyalib` was not selected because its current
MSRV exceeds the repository policy. No custom parser or permanent dual-parser
fallback was introduced.

## 3. Production implementation evidence

- `crates/codegg-config/src/document.rs` defines the format-neutral YAML
  boundary, typed error classes, source locations, UTF-8 and 1 MiB input
  bounds, and the only production `serde_norway` call.
- `src/agent/mod.rs` routes typed agent definitions and frontmatter through
  the codec.
- `src/command/mod.rs` routes typed command definitions and dynamic command
  metadata through the codec.
- `src/skills/parser.rs`, `src/skills/candidate.rs`, and `src/skills/mod.rs`
  use the codec and `serde_json::Value` for dynamic metadata.
- `serde_yaml` and its `unsafe-libyaml` dependency were removed from the
  manifests and lockfile; `serde_norway` and
  `unsafe-libyaml-norway` are the replacement graph.
- No agent, command, skill, authority, protocol, storage, or daemon execution
  contract changed.

## 4. Verification executed

Focused local checks:

```text
cargo test -p codegg-config document --lib -- --test-threads=1       5 passed
cargo test -p codegg-config --lib -- --test-threads=1                74 passed
cargo check -p codegg --all-targets                                 pass
cargo test --lib command -- --test-threads=1                       622 passed
cargo test --lib agent -- --test-threads=1                         342 passed
cargo test --lib skills -- --test-threads=1                         34 passed
cargo test --test skills_registry -- --test-threads=1               24 passed
python3 scripts/check_yaml_parser_boundary.py --self-test          pass
python3 scripts/check_yaml_parser_boundary.py                      pass
scripts/verify.sh quick                                             pass
scripts/verify.sh full                                              pass
```

The full verification included formatting, built-in-asset and static guards,
workspace Clippy, the single-threaded workspace test suite, and the
server/plugins/LSP feature check. The accepted executable revision was
`bc8e425` before this documentation-only closure status change.

Dependency checks also confirmed that `serde_yaml` is absent from the active
production graph and that direct parser references are confined to the codec,
manifest, maintenance guard, and documentation.

Hosted:

- PR #72 has no check run for the accepted pushed revision.
- The repository workflow does not expose `workflow_dispatch`, so an exact
  revision hosted verification could not be initiated from this workstream.
- This is an evidence gap, not a failed compile, test, or production finding.

## 5. Invariant review

- Existing YAML inputs remain readable through `serde_norway` during the
  compatibility window.
- Parser selection does not broaden agent, command, or skill authority.
- Duplicate keys are explicit last-value-wins compatibility; type mismatches,
  unknown fields, malformed indentation, truncation, non-UTF-8 input, and
  multi-document streams are handled by typed errors or existing Serde rules.
- Anchors, aliases, tags, and merge keys remain delegated to the selected
  parser; no repository fixture depends on unsupported extensions, and no
  broader support was added by application code.
- Source names and parser line/column locations remain available in errors.
- Reload publication remains atomic at the existing loader boundary, so a
  failed parse cannot replace last-known-good state with partial data.
- No YAML serialization path was added. Existing canonical TOML/JSON5/JSON
  writes remain unchanged.
- Malformed user input is bounded and cannot reach a parser panic through the
  tested loaders. YAML input is capped at 1 MiB; skill frontmatter retains its
  surrounding 64 KiB limit.
- All direct production parser calls are centralized and statically guarded.

## 6. Failure and recovery review

Malformed YAML returns `DocumentParseError` with format, source name, error
class, message, and optional location. Invalid UTF-8, oversized input, and
multi-document streams fail before typed configuration is returned. Loader
precedence and discovery remain outside the codec. No partial configuration,
agent, command, or skill result is published on a parse failure. The change
does not add concurrency, restart, process, or scheduler behavior.

## 7. Migration and compatibility review

There is no database, storage, protocol, or runtime-asset migration. Existing
YAML files are read in place and are never silently rewritten. New generated
or rewritten configuration/assets continue to use their existing canonical
TOML, JSON5, or JSON formats. The deprecated parser is fully removed from the
active production graph; its removal condition is therefore satisfied for
this milestone. Future removal of YAML itself remains a separate product
decision requiring evidence that all compatibility consumers have migrated.

## 8. Security review

No authority, credential, endpoint, protocol, or execution boundary changed.
The parser performs no network or code execution. Input size is bounded before
parsing, and the replacement remains in one reviewed module. The replacement
uses the translated Rust `unsafe-libyaml-norway` dependency internally, but
CodeGG has no native runtime library dependency and invokes it only for
read-only compatibility parsing. No critical, high, or medium finding remains.

## 9. Documentation and operations

- Added `docs/dependency-maintenance.md` with the manual review procedure.
- Updated agent, command, and skill architecture documentation to identify
  YAML as compatibility input and TOML/JSON5 as canonical generated formats.
- Added the parser-boundary guard and its self-test to `scripts/verify.sh quick`.
- No dependency bot, scheduled audit, new CI lane, release automation, broad
  parser corpus, or conversion framework was introduced.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low / operational | No hosted `verify` result is attached to the accepted pushed revision; PR #72 reports no checks and the workflow has no dispatch trigger | Strict hosted-evidence criterion from the implementation plan cannot be proven in this environment | Run the existing hosted `verify` workflow against the final pushed branch SHA when the repository exposes a trigger, then update this record to `closed` only if all required steps pass. |

No production correctness, compatibility, security, or test finding remains.

## 11. Roadmap disposition

M006 is conditionally closed. M003 remains blocked on the strict M001/M002
supported-Linux and hosted evidence conditions. M007 remains blocked on its
hard dependencies M002, M003, M005, and M006; M004 remains only a soft final
measurement input. M008 remains blocked until M001–M007 have accepted
dispositions.

The dependency graph was audited after M006 implementation and again during
closure. No future plan became unblocked: M007 still lacks M002, M003, and
strict M005 in addition to M006; M008 still lacks the upstream milestones.

## 12. Registry updates

- M006 moved from closing review to the runtime-safety milestone dispositions
  as `conditionally closed`.
- The dependency-ready M006 row was removed; no plan was promoted to ready.
- M003, M007, and M008 remain blocked with their existing named blockers.
- The runtime-safety roadmap remains active because earlier evidence gates and
  downstream milestone dependencies remain open.
- No corrective implementation plan is required: the only unresolved item is
  the external hosted evidence trigger, and no production correctness finding
  remains.
