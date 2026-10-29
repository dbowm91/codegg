# M002 Key-to-Paint Measurement Correction — 2026-10-29

The earlier visible-host key-to-paint samples (p50 280.686–288.019 ms,
p95 285.984–306.322 ms) are not suitable as an editor-latency baseline. The
E2E helper timed the WebDriver key send and DOM polling, then waited for two
`requestAnimationFrame` callbacks but resolved after a fixed 250-ms timeout
if those callbacks did not arrive. The recorded values clustered close to
that fallback, so the sample may have been dominated by test instrumentation.
This is a measurement defect, not evidence of a product regression.

The helper in `apps/desktop/e2e/specs/m004-session.e2e.ts` starts the clock
inside the page's captured `keydown` handler and resolves only after the caret
column changes and crosses two animation frames. It observes current DOM state
because React may replace the caret element during rendering. If the key,
rendered update, or frames fail to arrive within five seconds, the sample
rejects instead of substituting a duration. WebDriver polling is outside the
timed interval.

## Corrected visible sample

The first corrected visible `m004-session.e2e.ts` run captured 31 cold-state
key-to-paint samples: p50 **25 ms**, p95 **39 ms**. The final visible session
run captured a fresh set: unloaded p50 **30 ms**, p95 **36 ms**; four-worker
CPU load p50 **27 ms**, p95 **34 ms**. Each condition has 31 samples.
Diagnostics remained `no_warm_server`; the measured path moved only the local
caret and did not start an LSP process. Cold request-to-result was p50 **1 ms**
/ p95 **1 ms** both unloaded and loaded (31 samples each), with the same
`no_warm_server` status. Host: Apple M4 Pro, macOS 26.6.2, ARM64 execution,
Rust 1.89 ARM64 daemon and Rust 1.90 ARM64 desktop E2E build. The shell reports
x86_64 under Rosetta. The first instrumentation revision was still invalid:
it observed a caret node that React replaced; a later run confirmed the
viewport's new caret reached the expected column while that observer timed
out. The current root-observing, animation-frame probe produced the sample
above.

The final visible session E2E passed 10/10, including reconnect, reload, native
close, and both loaded/unloaded latency cases. The test-only E2E capability
shows and focuses the native `main` window before sampling; the production
capability remains unchanged. These are visible-host cold-state observations,
not a before/after performance claim or a warm-server comparison. Coordinated
warm/cold request-to-result evidence, including the intentional production
debounce, is in `plans/closure/daemon-lsp-frontend-convergence/002-status.md`.
Root x86_64 linking also fails because the host has only ARM64 zlib, libiconv,
and lzma libraries; explicit ARM64 `RUSTC` overrides build both daemon and
desktop binaries.
