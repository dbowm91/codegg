use std::fmt;
use std::time::SystemTime;

#[derive(Debug, Clone, Default)]
pub enum SessionStatus {
    #[default]
    Idle,
    Busy,
    Error,
    Compacting,
    Exporting,
}

impl SessionStatus {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            SessionStatus::Busy | SessionStatus::Compacting | SessionStatus::Exporting
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, SessionStatus::Error)
    }

    pub fn label(&self) -> &'static str {
        match self {
            SessionStatus::Idle => "idle",
            SessionStatus::Busy => "busy",
            SessionStatus::Error => "error",
            SessionStatus::Compacting => "compacting",
            SessionStatus::Exporting => "exporting",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SessionStatus::Idle => "○",
            SessionStatus::Busy => "◉",
            SessionStatus::Error => "✗",
            SessionStatus::Compacting => "⟳",
            SessionStatus::Exporting => "↑",
        }
    }
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub status: SessionStatus,
    pub started_at: Option<SystemTime>,
    pub last_activity: Option<SystemTime>,
    pub turn_count: usize,
    pub token_in: usize,
    pub token_out: usize,
    pub error_message: Option<String>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self) {
        let now = SystemTime::now();
        self.status = SessionStatus::Busy;
        self.started_at = Some(now);
        self.last_activity = Some(now);
    }

    pub fn idle(&mut self) {
        self.status = SessionStatus::Idle;
        self.last_activity = Some(SystemTime::now());
    }

    pub fn error(&mut self, msg: String) {
        self.status = SessionStatus::Error;
        self.error_message = Some(msg);
        self.last_activity = Some(SystemTime::now());
    }

    pub fn compacting(&mut self) {
        self.status = SessionStatus::Compacting;
        self.last_activity = Some(SystemTime::now());
    }

    pub fn exporting(&mut self) {
        self.status = SessionStatus::Exporting;
        self.last_activity = Some(SystemTime::now());
    }

    pub fn record_turn(&mut self, tokens_in: usize, tokens_out: usize) {
        self.turn_count += 1;
        self.token_in += tokens_in;
        self.token_out += tokens_out;
        self.last_activity = Some(SystemTime::now());
    }

    pub fn duration(&self) -> Option<std::time::Duration> {
        self.started_at
            .and_then(|s| SystemTime::now().duration_since(s).ok())
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.status, SessionStatus::Idle)
    }

    pub fn is_active(&self) -> bool {
        !self.is_idle() && !self.status.is_terminal()
    }
}
