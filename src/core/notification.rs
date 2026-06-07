use std::collections::HashSet;
use std::collections::VecDeque;

use chrono::{DateTime, Timelike, Utc};
use sqlx::Row;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotificationKind {
    TurnCompleted,
    TurnFailed,
    AwaitingInput,
    PermissionRequired,
    QuestionRequired,
    SubagentCompleted,
    SubagentFailed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone)]
pub struct NotificationEvent {
    pub id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: NotificationKind,
    pub priority: NotificationPriority,
    pub message: String,
    pub dedupe_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuietHours {
    pub start_hour: u8,
    pub end_hour: u8,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NotificationPolicy {
    pub enabled: bool,
    pub visual: bool,
    pub desktop: bool,
    pub audio: bool,
    pub tts: bool,
    pub coalesce_window_ms: u64,
    pub speak_kinds: HashSet<NotificationKind>,
    pub interrupt_on: HashSet<NotificationKind>,
    pub quiet_hours: Option<QuietHours>,
}

impl Default for NotificationPolicy {
    fn default() -> Self {
        let mut speak_kinds = HashSet::new();
        speak_kinds.insert(NotificationKind::PermissionRequired);
        speak_kinds.insert(NotificationKind::QuestionRequired);
        speak_kinds.insert(NotificationKind::TurnFailed);

        let mut interrupt_on = HashSet::new();
        interrupt_on.insert(NotificationKind::PermissionRequired);
        interrupt_on.insert(NotificationKind::QuestionRequired);
        interrupt_on.insert(NotificationKind::TurnFailed);

        Self {
            enabled: true,
            visual: true,
            desktop: true,
            audio: true,
            tts: true,
            coalesce_window_ms: 1200,
            speak_kinds,
            interrupt_on,
            quiet_hours: None,
        }
    }
}

impl NotificationPolicy {
    pub fn from_config(config: &crate::config::schema::Config) -> Self {
        let mut policy = Self::default();

        if let Some(ref notif_config) = config.notifications {
            if let Some(enabled) = notif_config.enabled {
                policy.enabled = enabled;
            }
            if let Some(true) = notif_config.on_task_complete {
                policy.speak_kinds.insert(NotificationKind::TurnCompleted);
            }
            if let Some(true) = notif_config.on_error {
                policy.speak_kinds.insert(NotificationKind::TurnFailed);
            }
            if let Some(ref audio) = notif_config.audio {
                if let Some(enabled) = audio.enabled {
                    policy.audio = enabled;
                    policy.tts = enabled;
                }
                if let Some(ref kinds) = audio.speak {
                    for kind_str in kinds {
                        if let Some(kind) = parse_notification_kind(kind_str) {
                            policy.speak_kinds.insert(kind);
                        }
                    }
                }
                if let Some(ref kinds) = audio.interrupt_on {
                    for kind_str in kinds {
                        if let Some(kind) = parse_notification_kind(kind_str) {
                            policy.interrupt_on.insert(kind);
                        }
                    }
                }
            }
            if let Some(ref qh_config) = notif_config.quiet_hours {
                if let (Some(start), Some(end)) = (qh_config.start_hour, qh_config.end_hour) {
                    policy.quiet_hours = Some(crate::core::notification::QuietHours {
                        start_hour: start,
                        end_hour: end,
                        timezone: qh_config.timezone.clone(),
                    });
                }
            }
        }

        policy
    }

    pub fn is_quiet_hours(&self) -> bool {
        if let Some(ref qh) = self.quiet_hours {
            let now = chrono::Local::now();
            let hour = now.hour() as u8;
            if qh.start_hour <= qh.end_hour {
                hour >= qh.start_hour && hour < qh.end_hour
            } else {
                hour >= qh.start_hour || hour < qh.end_hour
            }
        } else {
            false
        }
    }
}

pub struct NotificationRouter {
    policy: NotificationPolicy,
    queue: Mutex<VecDeque<NotificationEvent>>,
    recent_dedupe: Mutex<VecDeque<String>>,
}

impl NotificationRouter {
    pub fn new(policy: NotificationPolicy) -> Self {
        Self {
            policy,
            queue: Mutex::new(VecDeque::new()),
            recent_dedupe: Mutex::new(VecDeque::new()),
        }
    }

    pub async fn emit(&self, event: NotificationEvent) {
        if !self.policy.enabled {
            return;
        }

        // Quiet hours: suppress events below High priority
        if self.policy.is_quiet_hours() && event.priority < NotificationPriority::High {
            return;
        }

        // Deduplicate
        if let Some(ref key) = event.dedupe_key {
            let mut dedupe = self.recent_dedupe.lock().await;
            if dedupe.contains(key) {
                return;
            }
            dedupe.push_back(key.clone());
            while dedupe.len() > 100 {
                dedupe.pop_front();
            }
        }

        // Coalesce: if last event is same kind within window, merge
        let mut queue = self.queue.lock().await;
        if let Some(last) = queue.back() {
            if last.kind == event.kind
                && last.session_id == event.session_id
                && (event.created_at - last.created_at).num_milliseconds()
                    < self.policy.coalesce_window_ms as i64
            {
                // Coalesce: keep the later event with merged message
                let merged_id = event.id.clone();
                let merged_priority = if event.priority > last.priority {
                    event.priority.clone()
                } else {
                    last.priority.clone()
                };
                let merged_message = if last.message != event.message {
                    format!("{}; {}", last.message, event.message)
                } else {
                    event.message.clone()
                };
                let merged = NotificationEvent {
                    id: merged_id,
                    session_id: event.session_id.clone(),
                    turn_id: event.turn_id.clone(),
                    kind: event.kind.clone(),
                    priority: merged_priority,
                    message: merged_message,
                    dedupe_key: event.dedupe_key.clone(),
                    created_at: event.created_at,
                };
                queue.pop_back();
                queue.push_back(merged);
                return;
            }
        }

        queue.push_back(event);
    }

    pub async fn next_speech(&self) -> Option<NotificationEvent> {
        let mut queue = self.queue.lock().await;
        let mut best_idx = None;
        let mut best_priority = NotificationPriority::Low;

        for (i, event) in queue.iter().enumerate() {
            if self.policy.speak_kinds.contains(&event.kind)
                && event.priority >= best_priority
            {
                best_idx = Some(i);
                best_priority = event.priority.clone();
            }
        }

        best_idx.map(|i| queue.remove(i).unwrap())
    }

    pub fn should_interrupt(&self, kind: &NotificationKind) -> bool {
        self.policy.interrupt_on.contains(kind)
    }

    pub fn is_tts_enabled(&self) -> bool {
        self.policy.tts
    }

    pub fn is_desktop_enabled(&self) -> bool {
        self.policy.desktop
    }

    pub async fn persist_notification(&self, pool: &sqlx::SqlitePool, event: &NotificationEvent) {
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO notification_history (id, session_id, turn_id, kind, priority, message, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&event.id)
        .bind(&event.session_id)
        .bind(&event.turn_id)
        .bind(format!("{:?}", event.kind))
        .bind(format!("{:?}", event.priority))
        .bind(&event.message)
        .bind(event.created_at.to_rfc3339())
        .execute(pool)
        .await;
    }

    pub async fn get_notification_history(
        &self,
        pool: &sqlx::SqlitePool,
        session_id: Option<&str>,
        limit: usize,
    ) -> Vec<NotificationEvent> {
        let mut query = "SELECT id, session_id, turn_id, kind, priority, message, created_at FROM notification_history".to_string();

        if session_id.is_some() {
            query.push_str(" WHERE session_id = ?");
        }

        query.push_str(" ORDER BY created_at DESC LIMIT ?");

        let mut q = sqlx::query(&query);

        if let Some(sid) = session_id {
            q = q.bind(sid);
        }

        q = q.bind(limit as i64);

        let rows = q
            .fetch_all(pool)
            .await
            .unwrap_or_default();

        rows.into_iter()
            .filter_map(|row| {
                let id: String = row.get(0);
                let session_id: Option<String> = row.get(1);
                let turn_id: Option<String> = row.get(2);
                let kind_str: String = row.get(3);
                let priority_str: String = row.get(4);
                let message: String = row.get(5);
                let created_at_str: String = row.get(6);

                let kind = parse_notification_kind_from_db(&kind_str)?;
                let priority = parse_priority_from_db(&priority_str)?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .ok()?
                    .with_timezone(&Utc);

                Some(NotificationEvent {
                    id,
                    session_id,
                    turn_id,
                    kind,
                    priority,
                    message,
                    dedupe_key: None,
                    created_at,
                })
            })
            .collect()
    }
}

pub struct AudioArbiter {
    router: std::sync::Arc<NotificationRouter>,
    speaking: tokio::sync::Mutex<bool>,
    interrupt: tokio::sync::watch::Sender<bool>,
}

impl AudioArbiter {
    pub fn new(router: std::sync::Arc<NotificationRouter>) -> Self {
        let (interrupt, _) = tokio::sync::watch::channel(false);
        Self {
            router,
            speaking: tokio::sync::Mutex::new(false),
            interrupt,
        }
    }

    pub fn request_interrupt(&self) {
        let _ = self.interrupt.send(true);
    }

    /// Start the speech consumption loop. Call this once at daemon startup.
    pub fn start(self: &std::sync::Arc<Self>) {
        let arbiter = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(500));
            loop {
                interval.tick().await;
                arbiter.process_queue().await;
            }
        });
    }

    async fn process_queue(&self) {
        if *self.speaking.lock().await {
            // Check if we should interrupt current speech
            if *self.interrupt.borrow() {
                stop_speech().await;
                *self.speaking.lock().await = false;
                let _ = self.interrupt.send(false);
            } else {
                return;
            }
        }

        if let Some(event) = self.router.next_speech().await {
            let mut speaking = self.speaking.lock().await;
            *speaking = true;
            drop(speaking);

            speak_text(&event.message).await;

            let mut speaking = self.speaking.lock().await;
            *speaking = false;
        }
    }
}

pub async fn speak_text(text: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = tokio::process::Command::new("say")
            .arg(text)
            .output()
            .await;
    }
    #[cfg(target_os = "linux")]
    {
        if tokio::process::Command::new("spd-say")
            .arg(text)
            .output()
            .await
            .is_err()
        {
            let _ = tokio::process::Command::new("espeak-ng")
                .arg(text)
                .output()
                .await;
        }
    }
}

pub async fn stop_speech() {
    #[cfg(target_os = "macos")]
    {
        let _ = tokio::process::Command::new("pkill")
            .arg("say")
            .output()
            .await;
    }
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("pkill")
            .arg("spd-say")
            .output()
            .await;
    }
}

fn parse_notification_kind(s: &str) -> Option<NotificationKind> {
    match s {
        "turn_completed" => Some(NotificationKind::TurnCompleted),
        "turn_failed" => Some(NotificationKind::TurnFailed),
        "awaiting_input" => Some(NotificationKind::AwaitingInput),
        "permission_required" => Some(NotificationKind::PermissionRequired),
        "question_required" => Some(NotificationKind::QuestionRequired),
        "subagent_completed" => Some(NotificationKind::SubagentCompleted),
        "subagent_failed" => Some(NotificationKind::SubagentFailed),
        "error" => Some(NotificationKind::Error),
        _ => None,
    }
}

fn parse_notification_kind_from_db(s: &str) -> Option<NotificationKind> {
    match s {
        "TurnCompleted" => Some(NotificationKind::TurnCompleted),
        "TurnFailed" => Some(NotificationKind::TurnFailed),
        "AwaitingInput" => Some(NotificationKind::AwaitingInput),
        "PermissionRequired" => Some(NotificationKind::PermissionRequired),
        "QuestionRequired" => Some(NotificationKind::QuestionRequired),
        "SubagentCompleted" => Some(NotificationKind::SubagentCompleted),
        "SubagentFailed" => Some(NotificationKind::SubagentFailed),
        "Error" => Some(NotificationKind::Error),
        _ => parse_notification_kind(s),
    }
}

fn parse_priority_from_db(s: &str) -> Option<NotificationPriority> {
    match s {
        "Low" => Some(NotificationPriority::Low),
        "Normal" => Some(NotificationPriority::Normal),
        "High" => Some(NotificationPriority::High),
        "Urgent" => Some(NotificationPriority::Urgent),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn default_policy_has_expected_speak_kinds() {
        let policy = NotificationPolicy::default();
        assert!(policy.speak_kinds.contains(&NotificationKind::PermissionRequired));
        assert!(policy.speak_kinds.contains(&NotificationKind::QuestionRequired));
        assert!(policy.speak_kinds.contains(&NotificationKind::TurnFailed));
        assert!(!policy.speak_kinds.contains(&NotificationKind::TurnCompleted));
    }

    #[test]
    fn default_policy_interrupt_on() {
        let policy = NotificationPolicy::default();
        assert!(policy.interrupt_on.contains(&NotificationKind::PermissionRequired));
        assert!(policy.interrupt_on.contains(&NotificationKind::TurnFailed));
        assert!(!policy.interrupt_on.contains(&NotificationKind::TurnCompleted));
    }

    #[tokio::test]
    async fn emit_deduplicates() {
        let router = Arc::new(NotificationRouter::new(NotificationPolicy::default()));
        let event = NotificationEvent {
            id: "1".into(),
            session_id: None,
            turn_id: None,
            kind: NotificationKind::TurnCompleted,
            priority: NotificationPriority::Low,
            message: "done".into(),
            dedupe_key: Some("key-1".into()),
            created_at: Utc::now(),
        };
        router.emit(event.clone()).await;
        router.emit(event).await;

        let speech = router.next_speech().await;
        assert!(speech.is_none());
    }

    #[tokio::test]
    async fn next_speech_returns_highest_priority() {
        let router = Arc::new(NotificationRouter::new(NotificationPolicy::default()));
        router.emit(NotificationEvent {
            id: "1".into(),
            session_id: None,
            turn_id: None,
            kind: NotificationKind::PermissionRequired,
            priority: NotificationPriority::Urgent,
            message: "perm".into(),
            dedupe_key: None,
            created_at: Utc::now(),
        }).await;
        router.emit(NotificationEvent {
            id: "2".into(),
            session_id: None,
            turn_id: None,
            kind: NotificationKind::TurnFailed,
            priority: NotificationPriority::High,
            message: "fail".into(),
            dedupe_key: None,
            created_at: Utc::now(),
        }).await;

        let speech = router.next_speech().await;
        assert!(speech.is_some());
        assert_eq!(speech.unwrap().kind, NotificationKind::PermissionRequired);
    }

    #[test]
    fn priority_ordering() {
        assert!(NotificationPriority::Urgent > NotificationPriority::High);
        assert!(NotificationPriority::High > NotificationPriority::Normal);
        assert!(NotificationPriority::Normal > NotificationPriority::Low);
    }
}
