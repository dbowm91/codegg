# Hooks Architecture Review (2026-05-25)

## Status: INCOMPLETE

The architecture document at `architecture/hooks.md` contains an incorrect claim at line 191 regarding stream error behavior.

---

## Critical Claim Under Review

**Document says (line 191):**
> **Important**: Stream errors now break the loop instead of returning early, ensuring `AgentEnd` and `SessionEnd` hooks run.

**Previous review found:** This claim is FALSE. Stream errors do NOT ensure `AgentEnd` hooks run.

---

## Actual Behavior - Verified Against `src/agent/loop.rs`

### Code Structure Analysis

The main agent turn loop is at `loop.rs:1313`:

```rust
loop {
    // ... turn setup ...

    // AgentStart hooks run here (inside loop)
    if let Some(ref hr) = self.hook_registry {
        for err in hr.run_hooks(HookEvent::AgentStart, &agent_start_ctx).await {
            tracing::error!("AgentStart hook error: {}", err);
        }
    }

    self.compact_if_needed(&mut request.messages).await;
    harden_history(&mut request.messages);

    // STREAM ERROR - line 1365-1370
    let events = match self.stream_with_retry(&request).await {
        Ok(events) => events,
        Err(e) => {
            tracing::error!("Stream error: {}", e);
            break;  // <-- BREAK here exits the loop immediately
        }
    };

    // Tool execution happens here (skipped on break)
    // AgentEnd hooks run here (inside loop, SKIPPED on break)

    processor.reset();

    // AgentEnd hooks at lines 1518-1533 (inside loop, SKIPPED on break)
    if let Some(ref hr) = self.hook_registry {
        for err in hr.run_hooks(HookEvent::AgentEnd, &agent_end_ctx).await {
            tracing::error!("AgentEnd hook error: {}", err);
        }
    }
}  // <-- END OF LOOP at line 1534

// OUTSIDE THE LOOP - these run regardless of break:
self.drain_follow_up(&mut request, &mut all_events, &mut processor).await;

// SessionEnd hooks at lines 1539-1554 (outside loop, RUN on break)
let session_end_ctx = crate::hooks::HookContext { ... };
if let Some(ref hr) = self.hook_registry {
    for err in hr.run_hooks(crate::hooks::HookEvent::SessionEnd, &session_end_ctx).await {
        tracing::error!("SessionEnd hook error: {}", err);
    }
}
```

### What Actually Happens on Stream Error

| Hook | Location | Runs on Stream Error? |
|------|----------|----------------------|
| `AgentStart` | Inside loop, before stream | NO - loop breaks before this turn's processing |
| `AgentEnd` | Inside loop | **NO** - break at line 1369 skips this |
| `SessionEnd` | Outside loop | **YES** - runs after loop exits |

### Correction Required

The documentation claim that "stream errors now break the loop instead of returning early, ensuring `AgentEnd` and `SessionEnd` hooks run" is **INCORRECT**.

**Actual behavior:**
- Stream errors DO break the loop (confirmed)
- `SessionEnd` hooks DO run (outside the loop)
- `AgentEnd` hooks do NOT run (inside the loop, skipped by break)

---

## Discrepancies Found

### 1. Critical Documentation Error (line 191)

**Current:** "Stream errors now break the loop instead of returning early, ensuring `AgentEnd` and `SessionEnd` hooks run."

**Should be:** "Stream errors break the loop. `SessionEnd` hooks run after loop exit. `AgentEnd` hooks are NOT executed when stream errors occur."

---

## Recommendations

### For Documentation (`architecture/hooks.md`)

1. **Line 191 - Fix the stream error claim:**
   ```markdown
   **Important**: Stream errors break the loop. `SessionEnd` hooks run after loop exit.
   `AgentEnd` hooks do NOT run on stream errors since they are inside the loop that is broken.
   ```

2. **Consider adding a note about this limitation** in the integration points section.

### For Code (`src/agent/loop.rs`)

If it is desired that `AgentEnd` hooks run even on stream errors, the code would need refactoring to move `AgentEnd` hook execution outside the loop, or to handle stream errors differently (e.g., return error instead of break, though this would change the flow).

**Current workaround:** If `AgentEnd` hooks must run on stream error, consider:
- Moving `AgentEnd` hook calls outside the main loop but keeping them in the function
- Or wrapping the stream call differently to avoid the `break` pattern

This is an architectural decision - the current behavior may be intentional since stream errors typically indicate a fatal issue with the provider connection.

---

## Summary

| Item | Status |
|------|--------|
| Architecture claim at line 191 | **INCORRECT** |
| `SessionEnd` hooks on stream error | Correct (runs) |
| `AgentEnd` hooks on stream error | **Incorrect (skipped)** |
| Documentation needs update | **YES** |
| Code change needed for `AgentEnd` | **Optional** - depends on intended behavior |

---

## Files Reviewed

- `architecture/hooks.md` - Line 191 claim is incorrect
- `.opencode/skills/hooks/SKILL.md` - Does not make the specific claim (lines 260-290 cover tool hooks correctly)
- `src/hooks/mod.rs` - Shell hooks system, 206 lines, matches docs
- `src/agent/loop.rs` - Confirmed actual behavior at lines 1313-1557