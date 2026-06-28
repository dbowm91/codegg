//! TUI runtime diagnostics.
//!
//! Lightweight counters that track slow renders, slow commands, event bus
//! lag, and loop stalls. All fields are updated infrequently (only when
//! thresholds are crossed) so they add negligible overhead per frame.

use std::collections::VecDeque;
use std::time::Duration;

/// Record of a single slow event-loop iteration.
#[derive(Debug, Clone)]
pub struct SlowLoopRecord {
    pub elapsed: Duration,
    pub timestamp: std::time::Instant,
}

/// Record of a single slow command execution.
#[derive(Debug, Clone)]
pub struct SlowCommandRecord {
    pub name: String,
    pub elapsed: Duration,
    pub timestamp: std::time::Instant,
}

/// Bounded ring-buffer capacity for slow-command records.
const MAX_SLOW_COMMAND_RECORDS: usize = 8;

/// Bounded ring-buffer capacity for slow-render records.
const MAX_SLOW_RENDER_RECORDS: usize = 4;

/// Bounded ring-buffer capacity for component render panic records.
const MAX_COMPONENT_RENDER_PANIC_RECORDS: usize = 8;

/// Record of a single slow render frame.
#[derive(Debug, Clone)]
pub struct SlowRenderRecord {
    pub elapsed_ms: u128,
    pub streaming_active: bool,
    pub timestamp: std::time::Instant,
}

/// Record of a component-level render panic.
#[derive(Debug, Clone)]
pub struct ComponentRenderPanicRecord {
    pub component: &'static str,
    pub timestamp: std::time::Instant,
}

/// TUI runtime diagnostics accumulator.
///
/// Updated only when thresholds are crossed, so per-frame cost is
/// essentially zero (one comparison + branch).
#[derive(Default)]
pub struct TuiDiagnostics {
    /// Number of event-loop iterations that exceeded 250 ms.
    pub slow_loop_count: u64,
    /// Number of render frames that exceeded 16 ms while streaming.
    pub slow_render_count: u64,
    /// Number of command handlers that exceeded 250 ms.
    pub slow_command_count: u64,
    /// Cumulative count of events dropped by the broadcast receiver.
    pub dropped_bus_events: u64,
    /// Record of the most recent slow loop iteration.
    pub last_slow_loop: Option<SlowLoopRecord>,
    /// Ring buffer of recent slow command records (most recent last).
    pub recent_slow_commands: VecDeque<SlowCommandRecord>,
    /// Ring buffer of recent slow render records (most recent last).
    pub recent_slow_renders: VecDeque<SlowRenderRecord>,
    /// Last render error message (if any).
    pub last_render_error: Option<String>,
    /// Number of render panics.
    pub render_panic_count: u64,
    /// Number of component-level render panics (guarded surfaces).
    pub component_render_panic_count: u64,
    /// Ring buffer of recent component render panic records.
    pub recent_component_render_panics: VecDeque<ComponentRenderPanicRecord>,
}

impl TuiDiagnostics {
    /// Record a slow event-loop iteration.
    pub fn record_slow_loop(&mut self, elapsed: Duration) {
        self.slow_loop_count += 1;
        self.last_slow_loop = Some(SlowLoopRecord {
            elapsed,
            timestamp: std::time::Instant::now(),
        });
    }

    /// Record a slow render frame.
    pub fn record_slow_render(&mut self, elapsed_ms: u128, streaming_active: bool) {
        self.slow_render_count += 1;
        if self.recent_slow_renders.len() >= MAX_SLOW_RENDER_RECORDS {
            self.recent_slow_renders.pop_front();
        }
        self.recent_slow_renders.push_back(SlowRenderRecord {
            elapsed_ms,
            streaming_active,
            timestamp: std::time::Instant::now(),
        });
    }

    /// Record a slow command execution.
    pub fn record_slow_command(&mut self, name: &str, elapsed: Duration) {
        self.slow_command_count += 1;
        if self.recent_slow_commands.len() >= MAX_SLOW_COMMAND_RECORDS {
            self.recent_slow_commands.pop_front();
        }
        self.recent_slow_commands.push_back(SlowCommandRecord {
            name: name.to_string(),
            elapsed,
            timestamp: std::time::Instant::now(),
        });
    }

    /// Accumulate dropped bus events.
    pub fn add_dropped_bus_events(&mut self, n: u64) {
        self.dropped_bus_events += n;
    }

    /// Record a component-level render panic.
    pub fn record_component_render_panic(&mut self, component: &'static str) {
        self.component_render_panic_count += 1;
        if self.recent_component_render_panics.len() >= MAX_COMPONENT_RENDER_PANIC_RECORDS {
            self.recent_component_render_panics.pop_front();
        }
        self.recent_component_render_panics
            .push_back(ComponentRenderPanicRecord {
                component,
                timestamp: std::time::Instant::now(),
            });
    }

    /// Format a human-readable summary for the /tui-stats command.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Slow loops:      {}", self.slow_loop_count));
        lines.push(format!("Slow renders:    {}", self.slow_render_count));
        lines.push(format!("Slow commands:   {}", self.slow_command_count));
        lines.push(format!("Dropped events:  {}", self.dropped_bus_events));
        lines.push(format!("Render panics:   {}", self.render_panic_count));
        lines.push(format!(
            "Component panics: {}",
            self.component_render_panic_count
        ));
        if let Some(ref err) = self.last_render_error {
            lines.push(format!("Last render err: {}", err));
        }
        if let Some(ref rec) = self.last_slow_loop {
            lines.push(format!(
                "Last slow loop:  {}ms ago",
                rec.timestamp.elapsed().as_millis()
            ));
        }
        if let Some(cmd) = self.recent_slow_commands.back() {
            lines.push(format!(
                "Last slow cmd:   '{}' took {}ms",
                cmd.name,
                cmd.elapsed.as_millis()
            ));
        }
        if let Some(panic_rec) = self.recent_component_render_panics.back() {
            lines.push(format!(
                "Last comp panic: '{}' {}ms ago",
                panic_rec.component,
                panic_rec.timestamp.elapsed().as_millis()
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_diagnostics_are_all_zero() {
        let d = TuiDiagnostics::default();
        assert_eq!(d.slow_loop_count, 0);
        assert_eq!(d.slow_render_count, 0);
        assert_eq!(d.slow_command_count, 0);
        assert_eq!(d.dropped_bus_events, 0);
        assert_eq!(d.render_panic_count, 0);
        assert!(d.last_slow_loop.is_none());
        assert!(d.recent_slow_commands.is_empty());
        assert!(d.recent_slow_renders.is_empty());
        assert!(d.last_render_error.is_none());
    }

    #[test]
    fn record_slow_loop_increments_and_stores() {
        let mut d = TuiDiagnostics::default();
        d.record_slow_loop(Duration::from_millis(300));
        assert_eq!(d.slow_loop_count, 1);
        let rec = d.last_slow_loop.as_ref().unwrap();
        assert_eq!(rec.elapsed, Duration::from_millis(300));
    }

    #[test]
    fn record_slow_command_caps_ring_buffer() {
        let mut d = TuiDiagnostics::default();
        for i in 0..12 {
            d.record_slow_command(&format!("cmd{}", i), Duration::from_millis(300));
        }
        assert_eq!(d.slow_command_count, 12);
        assert_eq!(d.recent_slow_commands.len(), MAX_SLOW_COMMAND_RECORDS);
        assert_eq!(d.recent_slow_commands.front().unwrap().name, "cmd4");
        assert_eq!(d.recent_slow_commands.back().unwrap().name, "cmd11");
    }

    #[test]
    fn record_slow_render_caps_ring_buffer() {
        let mut d = TuiDiagnostics::default();
        for _ in 0..8 {
            d.record_slow_render(20, true);
        }
        assert_eq!(d.slow_render_count, 8);
        assert_eq!(d.recent_slow_renders.len(), MAX_SLOW_RENDER_RECORDS);
    }

    #[test]
    fn add_dropped_bus_events_accumulates() {
        let mut d = TuiDiagnostics::default();
        d.add_dropped_bus_events(5);
        d.add_dropped_bus_events(3);
        assert_eq!(d.dropped_bus_events, 8);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn summary_includes_all_fields() {
        let mut d = TuiDiagnostics::default();
        d.slow_render_count = 2;
        d.slow_command_count = 3;
        d.dropped_bus_events = 4;
        d.render_panic_count = 5;
        d.last_render_error = Some("test error".to_string());
        d.record_slow_loop(Duration::from_millis(300));
        d.record_slow_command("test_cmd", Duration::from_millis(250));
        d.record_component_render_panic("messages");
        d.record_component_render_panic("sidebar");
        let s = d.summary();
        assert!(s.contains("Slow loops:      1"));
        assert!(s.contains("Slow renders:    2"));
        assert!(s.contains("Slow commands:   4"));
        assert!(s.contains("Dropped events:  4"));
        assert!(s.contains("Render panics:   5"));
        assert!(s.contains("Component panics: 2"));
        assert!(s.contains("Last render err: test error"));
        assert!(s.contains("Last slow cmd:   'test_cmd'"));
        assert!(s.contains("Last comp panic: 'sidebar'"));
    }

    #[test]
    fn record_component_render_panic_increments_and_stores() {
        let mut d = TuiDiagnostics::default();
        d.record_component_render_panic("sidebar");
        assert_eq!(d.component_render_panic_count, 1);
        assert_eq!(d.recent_component_render_panics.len(), 1);
        assert_eq!(d.recent_component_render_panics[0].component, "sidebar");
    }

    #[test]
    fn record_component_render_panic_caps_ring_buffer() {
        let mut d = TuiDiagnostics::default();
        for i in 0..12 {
            d.record_component_render_panic(match i % 3 {
                0 => "messages",
                1 => "sidebar",
                _ => "dialog",
            });
        }
        assert_eq!(d.component_render_panic_count, 12);
        assert_eq!(
            d.recent_component_render_panics.len(),
            MAX_COMPONENT_RENDER_PANIC_RECORDS
        );
    }

    #[test]
    fn default_component_panic_fields_are_zero() {
        let d = TuiDiagnostics::default();
        assert_eq!(d.component_render_panic_count, 0);
        assert!(d.recent_component_render_panics.is_empty());
    }
}
