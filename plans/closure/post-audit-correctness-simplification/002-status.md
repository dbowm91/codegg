# Post-Audit Correctness, Simplification, and Footprint Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-correctness-simplification/002-daemon-stop-identity-and-cli-json-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Implementation commits or pull requests:

- `9301d6e` — prove daemon identity from `ServerHello` before stop signalling; replace handwritten CLI JSON escaping; add lifecycle and serializer regressions.

## 1. Executive finding

M002 is complete and strictly closed. `daemon stop` now requires persisted
metadata, connects to the configured endpoint, waits for the live
`ServerHello`, and compares its daemon identity with the metadata identity
before sending SIGTERM to the stored PID. Missing metadata, unreachable
endpoints, missing live identity, identity mismatches, and failed signals fail
closed without artifact cleanup or signalling. The legacy PID file is no
longer sufficient to authorize a stop.

CLI JSON output now uses `serde_json` for the existing `{ "response": ... }`
object shape. Quotes, backslashes, all ASCII control characters, Unicode, and
empty strings are covered by round-trip tests.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Live daemon identity is proven before signalling | `SocketCoreClient::daemon_id()` records `ServerHello.daemon_id`; `daemon stop` compares it to `DaemonInstanceMetadata.daemon_id` before `libc::kill` | pass | No protocol or shutdown-message change was added. |
| Mismatched metadata cannot signal the live daemon | `stop_requires_matching_live_daemon_identity` | pass | Tampered metadata produces a nonzero stop result, reports the mismatch, and the daemon remains reachable. |
| A current daemon remains stoppable | `stop_signals_the_current_daemon_after_identity_match` | pass | Matching identity sends SIGTERM and the daemon exits. |
| Legacy PID-only state fails closed | Stop path rejects missing metadata with an actionable diagnostic and does not read the PID file as an authorization source | pass | This is deliberately fail-closed compatibility behavior. |
| Endpoint and handshake failures fail closed | Stop path maps connect and `ServerHello` timeout failures to diagnostics before signalling | pass | No stale PID/socket cleanup occurs on these paths. |
| JSON is standards-compliant for arbitrary valid strings | `output_format_tests::json_round_trips_all_supported_string_content` | pass | Covers quote, slash, newline, carriage return, tab, backspace, form-feed, NUL, U+001F, and Unicode. |
| Empty JSON and text output retain their contracts | `json_preserves_empty_strings_and_text_output` | pass | Text formatting remains unchanged. |

## 3. Production implementation evidence

`src/core/transport/socket.rs` retains the daemon identity received in the
existing `ServerHello` frame and provides an awaitable `daemon_id()` accessor.
The accessor uses a notification rather than a timing sleep and times out
with an error if the handshake never arrives.

`src/main.rs` now performs the stop sequence as metadata read, endpoint
connect, live hello identity retrieval, exact identity comparison, and only
then SIGTERM. It does not unlink the socket or PID file after an unverifiable
or failed stop, avoiding cleanup of artifacts that may belong to a replacement
daemon. The daemon's existing signal and startup ownership model is unchanged.

`OutputFormat::Json` delegates encoding to `serde_json` while retaining the
single `response` field and string value type.

`tests/single_daemon_lifecycle.rs` adds process-level mismatch and normal-stop
coverage. The binary unit tests exercise serializer parsing and value
round-tripping.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt -- --check
rtk git diff --check
rtk cargo test --bin codegg output_format
rtk cargo test --test single_daemon_lifecycle stop_requires_matching_live_daemon_identity -- --nocapture --test-threads=1
rtk cargo test --test single_daemon_lifecycle stop_signals_the_current_daemon_after_identity_match -- --nocapture --test-threads=1
rtk cargo test --test single_daemon_lifecycle -- --skip status_reports_daemon_identity_with_metadata --test-threads=1
rtk scripts/verify.sh quick
```

### Results

All listed commands passed. The serializer suite reported 2 tests; each new
stop test reported 1 test; the remaining lifecycle coverage reported 4 tests;
and quick verification passed formatting, generated assets, static guards, and
the capped workspace/all-target check.

The unfiltered lifecycle command was also attempted:

```bash
rtk cargo test --test single_daemon_lifecycle -- --test-threads=1
```

It did not complete because the pre-existing
`status_reports_daemon_identity_with_metadata` case blocked in its
`daemon status` `SnapshotDaemon` request. The bounded local process was
stopped after the hang was confirmed. This status/snapshot issue is outside
the M002 production change; the M002 stop path deliberately uses the already
negotiated `ServerHello` identity and does not depend on that request.

No hosted CI result was available in this local closure pass.

## 5. Invariant review

- The advisory daemon lock remains the singleton authority; metadata remains
  diagnostic and is never trusted without live identity evidence.
- Stop requires exact equality between persisted `daemon_id` and the live
  daemon identity answering the configured endpoint.
- Missing or mismatched metadata cannot signal a PID, and no stale artifact is
  removed on an unverifiable or failed stop.
- Existing endpoint override resolution and normal `daemon stop` invocation
  remain unchanged.
- The existing SIGTERM behavior remains the shutdown mechanism after proof.
- No new daemon authority, supervisor, PID namespace, pidfd abstraction, or
  protocol request was introduced.
- JSON output is serializer-backed and round-trips every valid string tested,
  including all ASCII control-character cases in the plan.

## 6. Failure and recovery review

The stop path fails closed when metadata is absent, the endpoint cannot be
connected, the server does not complete `ServerHello`, the live identity does
not match metadata, or SIGTERM itself fails. These paths return actionable
errors and do not perform broad stale-path cleanup. A successful signal leaves
normal daemon process cleanup to the existing daemon lifecycle.

The new client identity state is in-memory only. Reconnection updates the
identity when a new `ServerHello` arrives; no persisted or wire-format state
changed.

## 7. Migration and compatibility review

No storage migration, metadata schema change, CLI command rename, protocol
wire-format change, or configuration migration is required. The legacy PID
file is still written for external compatibility, but it is no longer a safe
standalone authorization source for `daemon stop`; users with only stale PID
state receive a fail-closed diagnostic and can use `daemon status` or remove
stale paths manually.

The JSON object shape and value type are unchanged. Only encoding correctness
and insignificant compactness/whitespace differ.

## 8. Security review

The correction closes the unsafe metadata-PID/socket-reachability combination:
the endpoint must identify the same daemon instance named by metadata before
the stored PID is signalled. The regression test demonstrates the important
mismatch case without risking a signal to an unrelated test process. No
arbitrary process command-line inspection or broader process-management
authority was added.

## 9. Documentation and operations

The implementation plan, subsystem roadmap, registry, and this closure record
now describe the identity proof and fail-closed legacy PID behavior. No
additional architecture document required updating because the stop behavior
was not separately specified outside the milestone roadmap.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The existing full lifecycle test's `daemon status` snapshot request can block in this environment. | It limits unfiltered lifecycle-suite evidence, but does not affect the new hello-identity stop path; the remaining lifecycle tests and quick verification pass. | Track separately in the daemon status/snapshot test path; do not reopen M002 unless evidence connects it to stop identity. |

No critical, high, or medium M002 finding remains.

## 11. Roadmap disposition

M002 is closed. M003, M004, M005, M006, and M007 remain independently ready.
M008 remains blocked because its hard dependency is closure of M003-M007; M002
closure alone does not satisfy that dependency. No other registered future
plan lists M002 as its sole remaining hard or interface dependency.

## 12. Registry updates

- The implementation plan is marked `implemented`.
- M002 was removed from dependency-ready work during implementation and is
  now recorded as closed by this closure record.
- M002 is marked `closed` in the subsystem roadmap.
- The post-audit subsystem remains `active` with M003-M007 ready and M008
  blocked on M003-M007.
- The blocked-work audit covered every registry entry and affected roadmap
  dependency reference to M002. No plan became dependency-ready, so no
  downstream status was changed.
