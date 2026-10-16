# Desktop Frontend and IDE Post-Closure Corrective C005 — Closure

Status: closed — closed-transport fixture ordering made deterministic

Source plan: `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/005-order-closed-transport-fixture.md`
Implementation: `3d395876`
Hosted CI: [`38850114895`](https://github.com/dbowm91/codegg/actions/runs/38850114895) — success
Hosted Desktop E2E: [`38850114846`](https://github.com/dbowm91/codegg/actions/runs/38850114846) — success

## 1. Closure finding

The fake daemon now waits on a test-owned oneshot signal after sending the
subscribe response. The test sends the signal only after
`SessionProjectionDriver::attach` returns, then verifies the driver publishes
`Disconnected` after peer death and that `stop` reports the subscription
released. This removes the response/EOF race without changing production
client behavior or increasing timeouts.

## 2. Evidence

| Requirement | Evidence | Result |
|---|---|---|
| Attach completes before peer death | `closed_transport_publishes_disconnected` uses a oneshot barrier | pass |
| Disconnect behavior remains asserted | Existing disconnected-state wait and release assertion remain | pass |
| Full workspace suite passes | Hosted CI `38850114895`: **13,961 passed, 0 failed, 6 skipped** | pass |
| Desktop E2E remains green | Hosted run `38850114846` on the exact source head | pass |

## 3. Unblock audit

No new desktop or IDE milestone is unblocked. Historical C001–C004 closure
records remain unchanged.

