# Harness Lab Continuation D002 — Closure Status

Status: closed
Implementation head: `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6`
Exact-head CI: [40383901325](https://github.com/dbowm91/codegg/actions/runs/40383901325) — success

## Result

Native tool success or failure is carried as a typed terminal outcome from execution through core events and projection into ACP. A genuine failed tool remains failed for the same tool-call identity; permission denial and cancellation retain their own outcomes. Error-like text from a successful tool does not override typed success. Projection filtering and existing visibility rules continue to bound public payloads, and replay uses the same terminal meaning.

Harness Lab consumes typed ACP status and failure class as its primary evidence, with a labeled compatibility path for historical receipts. A gap marks observation incomplete and does not become a guessed tool failure. Rust and Python regression fixtures cover status mapping, identity, redaction, replay, and retry-follow-up behavior.

## Verification

The exact-head hosted run passed all guards, strict workspace Clippy, the Harness Lab Python suite, and the complete workspace test sweep. D002's acceptance matrix is covered by deterministic local fixtures; no provider call or wire-breaking change was introduced. No unresolved D002 condition remains.
