# Eggwork Worker M002 — Consume Authorized Gregg-Backed Eggwork Telemetry

Status: **blocked** — upstream `eggstack/eggwork` optional Gregg telemetry M002 authenticated endpoint/client not yet implemented and closed
Class: protocol / observability
Source: `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`
Upstream interface authority: `eggstack/eggwork: plans/implementation/optional-gregg-host-telemetry/002-authenticated-node-telemetry.md` and ADR-0006.
Reviewed CodeGG baseline (2026-10-19): `src/scheduler/eggwork.rs::EggworkNodePosture`, `node_postures()`, `src/scheduler/snapshot.rs`, `src/core/daemon_construct.rs` plus existing client/protocol surface.

## 1. Objective

Consume Eggwork's authenticated, sanitized node-level host observations from configured fixed targets and expose them as bounded operator-facing CodeGG node diagnostics. **Never access Gregg directly from CodeGG.** No placement or admission policy change in this milestone.

## 2. Existing seams

`EggworkNodePosture` currently contains reachability, draining, active/max slots, policy-satisfaction, isolation/network/resource **capability booleans**, a bounded feature list and age. No host usage metrics. `node_postures()` is an on-demand cache; no confirmed production caller exposes the full projection. Ordinary code constructs `EggworkExecutor` only when named nodes exist. Terminal `ExecutorMetrics` currently defaults and cannot be populated with Gregg **host-wide** observations.

## 3. Work packages

A. Once upstream API is closed, inspect current `eggwork-client` and wire schema; pin one reviewed revision of `eggwork-client` and `eggwork-core` consistently with the existing CodeGG lockfile and `crates/eggwork-test-node` fixture. Consume the frozen optional `NodeTelemetryV1` (exact upstream type/route to be confirmed). Unsupported older-node route must be distinct from authorized-but-disabled, stale, unavailable and malformed telemetry. Never relax mTLS certificate/identity validation.

B. Extend a CodeGG-owned read-only node status DTO/projection with aggregated CPU/logical cores, memory available/used/total, optional swap/load/disk capacity/I/O/network, observation timestamp/age, provider state and capability missingness. Bound arrays/text and intentionally omit interface identities, file paths, hostname, per-Gregg cron output, secrets and source errors. Join telemetry to node identity only after successful authenticated node_id equality check. Do not treat Gregg's observed host state as `NodeCapabilities` execution enforcement.

C. Use one controller-owned cache/probe coordinator per named node, not per-session, per-agent or per-UI instance. Audit `node_postures()` cache lifetime, stale behavior and concurrent caller coalescing. Authentication/probe errors remain typed; enforce server and CodeGG-side response/time bounds. Do not perform new Gregg requests, open port 11310, or add daemon process management.

D. Surface a read-only operator `NodeList/NodeStatus`-style request (names may adapt to established Core protocol) through the CodeGG daemon/client and a minimal TUI/CLI diagnostic if consistent with existing UI. Keep it independent of job dispatch. If remote diagnostics require an authorization policy, reuse/enforce existing operator grants before network calls. Cold/no-config nodes return meaningful explicit status.

E. Preserve exact input/terminal semantics: do not synthesize per-attempt CPU/RSS/elapsed from Gregg; `ExecutorCompletion.metrics` remains per-execution only. Real numerical per-attempt metrics require a distinct Eggwork cgroup measurement contract.

## 4. Tests / evidence

- Real mTLS Eggwork test node with configured synthetic Gregg provider: authorized remote status contains current/sanitized metrics and correct node id; unauthorized readers cannot access it.
- No provider, old Eggwork node (404/unsupported), Gregg down/slow/stale/malformed, clock skew, version mismatch, metric unsupported on Windows/macOS, and empty nodes: distinct bounded diagnostics, zero fabricated values.
- Two sessions/clients requesting same node do not amplify Gregg polling; repeated user refresh respects TTL, no unbounded awaiting or task growth.
- Telemetry failure does not block builds, change resource/isolation guarantees, send local fallback, or change queue/admission.
- Status payload round-trip through daemon protocol to actual frontend, including optional fields in legacy clients; prove read-only authorization.

Focused `cargo test -p codegg-protocol --locked`, `cargo test -p codegg-client --locked`, `cargo test --test eggwork_remote_execution_live --locked` where supported; `python3 scripts/check_eggwork_target_routing.py`, `python3 scripts/check_scheduler_bypass.py`, `scripts/verify.sh quick`, broader verification per `AGENTS.md`.

## 5. Compatibility, closure, stop

Additive optional wire fields/requests only; no persistent host-metric database migration. Document `greggd` loopback bind requirement on worker as **Eggwork** deployment guidance; CodeGG needs Eggwork mTLS only. Closure in `plans/closure/eggwork-worker-telemetry-and-targeting/002-status.md` must identify upstream exact pin, test evidence and remaining limitations.

Stop if upstream API remains unfrozen, direct Gregg exposure is proposed, a new unauthenticated status route is needed, or a host-wide metric is used as proof of per-execution consumption/admission.
