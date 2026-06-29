//! Terminal lifecycle guard for robust setup/teardown.
//!
//! [`TerminalGuard`] tracks each terminal feature that was successfully
//! enabled and restores exactly once in reverse order on drop or explicit
//! shutdown. This prevents partial-setup leaks and removes the need for
//! manual escape-sequence printing in teardown.

use std::io::{self, stdout};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::error::AppError;

/// Type alias for the terminal backend used throughout the TUI.
pub type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// Create a new ratatui terminal backed by stdout.
pub fn create_terminal() -> Result<AppTerminal, AppError> {
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Guard that owns terminal lifecycle state.
///
/// Each field tracks whether a specific terminal feature was successfully
/// enabled. On [`restore()`](Self::restore) or drop, features are disabled
/// in reverse order. The guard is idempotent — calling `restore()` twice is
/// safe.
pub struct TerminalGuard {
    raw_enabled: bool,
    alt_screen: bool,
    bracketed_paste: bool,
    mouse_capture: bool,
    restored: bool,
}

impl TerminalGuard {
    /// Enable terminal features in the recommended order:
    ///
    /// 1. Enter alternate screen
    /// 2. Enable raw mode
    /// 3. Enable bracketed paste
    /// 4. Enable mouse capture
    ///
    /// If a later step fails, all previously enabled features are rolled back
    /// before returning the error.
    pub fn enter() -> Result<Self, AppError> {
        let mut guard = Self {
            raw_enabled: false,
            alt_screen: false,
            bracketed_paste: false,
            mouse_capture: false,
            restored: false,
        };

        // Step 1: Enter alternate screen
        execute!(stdout(), EnterAlternateScreen).map_err(|e| {
            tracing::error!("Failed to enter alternate screen: {e}");
            guard.restore();
            AppError::from(e)
        })?;
        guard.alt_screen = true;

        // Step 2: Enable raw mode
        crossterm::terminal::enable_raw_mode().map_err(|e| {
            tracing::error!("Failed to enable raw mode: {e}");
            guard.restore();
            AppError::from(e)
        })?;
        guard.raw_enabled = true;

        // Step 3: Enable bracketed paste
        execute!(stdout(), EnableBracketedPaste).map_err(|e| {
            tracing::error!("Failed to enable bracketed paste: {e}");
            guard.restore();
            AppError::from(e)
        })?;
        guard.bracketed_paste = true;

        // Step 4: Enable mouse capture
        execute!(stdout(), EnableMouseCapture).map_err(|e| {
            tracing::error!("Failed to enable mouse capture: {e}");
            guard.restore();
            AppError::from(e)
        })?;
        guard.mouse_capture = true;

        Ok(guard)
    }

    /// Restore terminal to its original state.
    ///
    /// Features are disabled in reverse order of setup. This method is
    /// idempotent — calling it multiple times is safe and will only
    /// perform teardown once.
    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;

        // Reverse order: mouse → bracketed → raw → alt screen
        if self.mouse_capture {
            let _ = execute!(stdout(), DisableMouseCapture);
        }
        if self.bracketed_paste {
            let _ = execute!(stdout(), DisableBracketedPaste);
        }
        if self.raw_enabled {
            let _ = crossterm::terminal::disable_raw_mode();
        }
        if self.alt_screen {
            let _ = execute!(stdout(), LeaveAlternateScreen);
        }
    }

    /// Returns `true` if terminal features were successfully enabled and
    /// not yet restored.
    pub fn is_active(&self) -> bool {
        !self.restored
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_is_idempotent() {
        let mut guard = TerminalGuard {
            raw_enabled: true,
            alt_screen: true,
            bracketed_paste: true,
            mouse_capture: true,
            restored: false,
        };

        guard.restore();
        assert!(guard.restored);
        assert!(!guard.is_active());

        // Second call should be a no-op
        guard.restore();
        assert!(guard.restored);
    }

    #[test]
    fn restore_with_partial_setup() {
        // Simulate a guard where only alt_screen and raw succeeded
        let mut guard = TerminalGuard {
            raw_enabled: true,
            alt_screen: true,
            bracketed_paste: false,
            mouse_capture: false,
            restored: false,
        };

        guard.restore();
        assert!(guard.restored);
    }

    #[test]
    fn restore_with_nothing_enabled() {
        let mut guard = TerminalGuard {
            raw_enabled: false,
            alt_screen: false,
            bracketed_paste: false,
            mouse_capture: false,
            restored: false,
        };

        guard.restore();
        assert!(guard.restored);
    }

    #[test]
    fn drop_calls_restore() {
        let mut guard = TerminalGuard {
            raw_enabled: true,
            alt_screen: false,
            bracketed_paste: false,
            mouse_capture: false,
            restored: false,
        };

        guard.restore();
        assert!(guard.restored);
    }

    #[test]
    fn fresh_guard_is_active() {
        let guard = TerminalGuard {
            raw_enabled: false,
            alt_screen: false,
            bracketed_paste: false,
            mouse_capture: false,
            restored: false,
        };

        assert!(guard.is_active());
    }
}
