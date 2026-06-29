//! Shared async request state for TUI dialogs.
//!
//! Provides [`AsyncUiRequestState`], a small reusable state machine for
//! tracking the lifecycle of async operations in dialogs. This replaces
//! ad-hoc generation counters and boolean in-flight flags with a single
//! consistent type.

/// A shared request-state machine for async dialog operations.
///
/// Tracks a monotonically increasing request ID, loading flag, and
/// last error. Use [`begin`](Self::begin) to start a new request,
/// [`finish`](Self::finish) or [`fail`](Self::fail) when the result
/// arrives, and [`cancel`](Self::cancel) when the user dismisses the
/// dialog or supersedes the request.
///
/// Stale completions are detected by comparing the returned request ID
/// against the current value: if the IDs don't match, the completion
/// is from a superseded request and should be ignored.
#[derive(Debug, Clone)]
pub struct AsyncUiRequestState {
    /// Monotonically increasing generation counter.
    request_id: u64,
    /// Whether a request is currently in flight.
    loading: bool,
    /// Whether the current request was cancelled.
    cancelled: bool,
    /// Last error message from a failed request.
    last_error: Option<String>,
}

impl AsyncUiRequestState {
    /// Create a new request state in the idle (not-loading) state.
    pub fn new() -> Self {
        Self {
            request_id: 0,
            loading: false,
            cancelled: false,
            last_error: None,
        }
    }

    /// Begin a new request. Returns the new request ID.
    ///
    /// - Increments the generation counter.
    /// - Sets `loading` to `true`.
    /// - Clears `cancelled`.
    /// - Clears the previous transient error.
    pub fn begin(&mut self) -> u64 {
        self.request_id += 1;
        self.loading = true;
        self.cancelled = false;
        self.last_error = None;
        self.request_id
    }

    /// Cancel the current request.
    ///
    /// - Sets `cancelled` to `true`.
    /// - Sets `loading` to `false`.
    /// - Invalidates the current request by incrementing the counter.
    pub fn cancel(&mut self) {
        self.loading = false;
        self.cancelled = true;
        self.request_id += 1;
    }

    /// Check whether the given request ID matches the current one.
    ///
    /// Returns `true` if the request ID is current (i.e. the result
    /// should be applied), `false` if it's stale.
    pub fn is_current(&self, request_id: u64) -> bool {
        request_id == self.request_id
    }

    /// Attempt to finish a request with the given ID.
    ///
    /// Returns `true` if the request ID is current and the request
    /// was not cancelled (i.e. the result should be applied).
    /// Returns `false` if stale or cancelled.
    ///
    /// On success, sets `loading` to `false` and clears `last_error`.
    pub fn finish(&mut self, request_id: u64) -> bool {
        if !self.is_current(request_id) || self.cancelled {
            return false;
        }
        self.loading = false;
        self.last_error = None;
        true
    }

    /// Attempt to fail a request with the given ID and error message.
    ///
    /// Returns `true` if the request ID is current and the request
    /// was not cancelled (i.e. the error should be shown).
    /// Returns `false` if stale or cancelled.
    ///
    /// On success, sets `loading` to `false` and stores `last_error`.
    pub fn fail(&mut self, request_id: u64, error: String) -> bool {
        if !self.is_current(request_id) || self.cancelled {
            return false;
        }
        self.loading = false;
        self.last_error = Some(error);
        true
    }

    /// Whether a request is currently in flight.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Whether the current request was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// The current request ID.
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// The last error message, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Clear the loading state without modifying the request ID.
    ///
    /// Useful for setting loading to false on dialog close without
    /// invalidating the request ID (the close handler will cancel tasks).
    pub fn clear_loading(&mut self) {
        self.loading = false;
    }
}

impl Default for AsyncUiRequestState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_idle() {
        let s = AsyncUiRequestState::new();
        assert_eq!(s.request_id(), 0);
        assert!(!s.is_loading());
        assert!(!s.is_cancelled());
        assert!(s.last_error().is_none());
    }

    #[test]
    fn begin_increments_and_sets_loading() {
        let mut s = AsyncUiRequestState::new();
        let id1 = s.begin();
        assert_eq!(id1, 1);
        assert!(s.is_loading());
        assert!(!s.is_cancelled());
        assert!(s.last_error().is_none());

        let id2 = s.begin();
        assert_eq!(id2, 2);
        assert!(s.is_loading());
    }

    #[test]
    fn begin_clears_previous_error() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        s.fail(id, "oops".into());
        assert_eq!(s.last_error(), Some("oops"));

        let id2 = s.begin();
        assert!(s.last_error().is_none());
        assert_eq!(id2, 2);
    }

    #[test]
    fn finish_returns_true_for_current() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        assert!(s.finish(id));
        assert!(!s.is_loading());
        assert!(s.last_error().is_none());
    }

    #[test]
    fn finish_returns_false_for_stale() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        let _ = s.begin(); // supersede
        assert!(!s.finish(id));
        // Current request is still loading
        assert!(s.is_loading());
    }

    #[test]
    fn finish_returns_false_when_cancelled() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        s.cancel();
        assert!(!s.finish(id));
    }

    #[test]
    fn cancel_increments_and_clears_loading() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        s.cancel();
        assert!(!s.is_loading());
        assert!(s.is_cancelled());
        // Old request ID is now stale
        assert!(!s.is_current(id));
        // New request ID is one higher
        assert!(s.is_current(id + 1));
    }

    #[test]
    fn fail_stores_error_for_current() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        assert!(s.fail(id, "bad".into()));
        assert!(!s.is_loading());
        assert_eq!(s.last_error(), Some("bad"));
    }

    #[test]
    fn fail_ignores_stale() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        let _ = s.begin(); // supersede
        assert!(!s.fail(id, "old error".into()));
        assert!(s.last_error().is_none());
    }

    #[test]
    fn fail_ignores_cancelled() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        s.cancel();
        assert!(!s.fail(id, "cancelled error".into()));
    }

    #[test]
    fn clear_loading_does_not_affect_request_id() {
        let mut s = AsyncUiRequestState::new();
        let id = s.begin();
        s.clear_loading();
        assert!(!s.is_loading());
        // Request ID is still current
        assert!(s.is_current(id));
    }

    #[test]
    fn default_matches_new() {
        let s = AsyncUiRequestState::default();
        assert_eq!(s.request_id(), 0);
        assert!(!s.is_loading());
    }

    #[test]
    fn begin_after_cancel_resets_cancelled() {
        let mut s = AsyncUiRequestState::new();
        let _id = s.begin();
        s.cancel();
        assert!(s.is_cancelled());
        let id2 = s.begin();
        assert!(!s.is_cancelled());
        assert!(s.is_current(id2));
    }

    #[test]
    fn multiple_lifecycle_cycles() {
        let mut s = AsyncUiRequestState::new();
        // Cycle 1: begin -> finish
        let id1 = s.begin();
        assert!(s.finish(id1));
        assert!(!s.is_loading());

        // Cycle 2: begin -> cancel -> begin -> finish
        let id2 = s.begin();
        s.cancel();
        let id3 = s.begin();
        assert!(!s.finish(id2)); // stale
        assert!(s.finish(id3)); // current
    }
}
