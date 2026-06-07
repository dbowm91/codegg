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

    /// Drain up to `max_items` speakable events from the queue and synthesize
    /// a single `NotificationEvent` describing them. If only one event is
    /// drained, it is returned as-is. Speakable events are filtered against
    /// `policy.speak_kinds` (same predicate as [`next_speech`]). Non-speakable
    /// events are left in the queue.
    ///
    /// Dedupe behavior: events that share a `dedupe_key` within the drained
    /// batch are collapsed (the first occurrence is kept). This is a
    /// defense-in-depth check; [`emit`](Self::emit) already drops duplicates
    /// via `recent_dedupe`, but the cache is bounded to 100 keys and can cycle.
    pub async fn next_speech_batch(&self, max_items: usize) -> Option<NotificationEvent> {
        if max_items == 0 {
            return None;
        }

        let mut queue = self.queue.lock().await;

        // Collect indices of speakable events in queue order.
        let mut indices: Vec<usize> = Vec::new();
        for (i, event) in queue.iter().enumerate() {
            if self.policy.speak_kinds.contains(&event.kind) {
                indices.push(i);
                if indices.len() >= max_items {
                    break;
                }
            }
        }

        if indices.is_empty() {
            return None;
        }

        // Remove from highest index first to keep earlier indices valid.
        let mut events: Vec<NotificationEvent> = indices
            .iter()
            .rev()
            .map(|&i| queue.remove(i).unwrap())
            .collect();
        events.reverse();

        if events.len() == 1 {
            return events.into_iter().next();
        }

        // Defense in depth: collapse duplicate dedupe_keys within the batch.
        let mut seen: HashSet<String> = HashSet::new();
        events.retain(|e| match &e.dedupe_key {
            Some(key) => seen.insert(key.clone()),
            None => true,
        });

        if events.is_empty() {
            return None;
        }

        if events.len() == 1 {
            return events.into_iter().next();
        }

        // Sort by priority (descending: Urgent first).
        events.sort_by(|a, b| b.priority.cmp(&a.priority));

        let message = render_speech_batch(&events);
        let priority = events
            .iter()
            .map(|e| e.priority.clone())
            .max()
            .unwrap_or(NotificationPriority::Normal);

        Some(NotificationEvent {
            id: format!("batch-{}", uuid::Uuid::new_v4()),
            session_id: None,
            turn_id: None,
            kind: NotificationKind::AwaitingInput,
            priority,
            message,
            dedupe_key: None,
            created_at: Utc::now(),
        })
    }

    /// Current queue length. Useful for tests and diagnostics.
    pub async fn queue_len(&self) -> usize {
        self.queue.lock().await.len()
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

        // Drain a batch of speakable events. `next_speech_batch` returns the
        // single event as-is when only one is drained, so the single-event
        // case has no synthesis overhead.
        if let Some(event) = self.router.next_speech_batch(16).await {
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

/// Render a batch of speakable events into a single speech string.
///
/// Single event: `"<display_name>: <original_message>"` (or just the message
/// when no session id is available).
///
/// Multiple events of the same kind: `"<N> sessions <verb>: a, b, c."`
/// (uses word form for small counts: "Three sessions completed: a, b, c.").
///
/// Multiple kinds: `"<N> sessions need attention. <name1> <verb1>. <name2> <verb2>. ..."`
/// with events emitted in priority order (urgent first).
pub fn render_speech_batch(events: &[NotificationEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }

    if events.len() == 1 {
        let event = &events[0];
        let name = display_name(event.session_id.as_deref());
        if name.is_empty() || event.message.is_empty() {
            return event.message.clone();
        }
        return format!("{}: {}", name, event.message);
    }

    // Group by kind in priority order. Caller is expected to have sorted
    // `events` by priority descending, so the first occurrence of a kind
    // corresponds to its highest-priority event.
    let mut groups: Vec<(NotificationKind, Vec<String>)> = Vec::new();
    for event in events {
        let name = display_name(event.session_id.as_deref());
        if let Some(last) = groups.last_mut() {
            if last.0 == event.kind {
                last.1.push(name);
                continue;
            }
        }
        groups.push((event.kind.clone(), vec![name]));
    }

    if groups.len() == 1 {
        let (kind, names) = groups.into_iter().next().unwrap();
        let prefix = kind_phrase_plural(&kind, names.len());
        format!("{}: {}.", prefix, names.join(", "))
    } else {
        let n = events.len();
        let mut parts = vec![format!("{} sessions need attention.", number_word(n))];
        for (kind, names) in &groups {
            parts.push(format!("{} {}.", names.join(", "), kind_verb_phrase(kind)));
        }
        parts.join(" ")
    }
}

fn display_name(session_id: Option<&str>) -> String {
    match session_id {
        Some(s) => s.chars().take(8).collect(),
        None => String::new(),
    }
}

fn number_word(n: usize) -> String {
    match n {
        1 => "One".to_string(),
        2 => "Two".to_string(),
        3 => "Three".to_string(),
        4 => "Four".to_string(),
        5 => "Five".to_string(),
        6 => "Six".to_string(),
        7 => "Seven".to_string(),
        8 => "Eight".to_string(),
        9 => "Nine".to_string(),
        10 => "Ten".to_string(),
        _ => n.to_string(),
    }
}

fn kind_phrase_plural(kind: &NotificationKind, count: usize) -> String {
    match kind {
        NotificationKind::TurnCompleted => {
            format!("{} sessions completed", number_word(count))
        }
        NotificationKind::TurnFailed => format!("{} sessions failed", number_word(count)),
        NotificationKind::AwaitingInput => {
            format!("{} sessions await input", number_word(count))
        }
        NotificationKind::PermissionRequired => {
            format!("{} sessions need permission", number_word(count))
        }
        NotificationKind::QuestionRequired => {
            format!("{} sessions have a question", number_word(count))
        }
        NotificationKind::SubagentCompleted => {
            format!("{} subagents completed", number_word(count))
        }
        NotificationKind::SubagentFailed => {
            format!("{} subagents failed", number_word(count))
        }
        NotificationKind::Error => format!("{} sessions errored", number_word(count)),
    }
}

fn kind_verb_phrase(kind: &NotificationKind) -> &'static str {
    match kind {
        NotificationKind::TurnCompleted => "completed",
        NotificationKind::TurnFailed => "failed",
        NotificationKind::AwaitingInput => "awaits input",
        NotificationKind::PermissionRequired => "needs permission",
        NotificationKind::QuestionRequired => "has a question",
        NotificationKind::SubagentCompleted => "subagent completed",
        NotificationKind::SubagentFailed => "subagent failed",
        NotificationKind::Error => "errored",
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

    fn policy_with_turn_completed() -> NotificationPolicy {
        let mut policy = NotificationPolicy::default();
        policy.speak_kinds.insert(NotificationKind::TurnCompleted);
        policy
    }

    fn make_event(
        id: &str,
        kind: NotificationKind,
        priority: NotificationPriority,
        session_id: Option<&str>,
        message: &str,
        dedupe_key: Option<&str>,
    ) -> NotificationEvent {
        NotificationEvent {
            id: id.to_string(),
            session_id: session_id.map(str::to_string),
            turn_id: None,
            kind,
            priority,
            message: message.to_string(),
            dedupe_key: dedupe_key.map(str::to_string),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn emit_deduplicates_within_batch() {
        let router = Arc::new(NotificationRouter::new(policy_with_turn_completed()));
        let a = make_event(
            "1",
            NotificationKind::TurnCompleted,
            NotificationPriority::Low,
            Some("session-aaa"),
            "first",
            Some("dup-key"),
        );
        let b = make_event(
            "2",
            NotificationKind::TurnCompleted,
            NotificationPriority::Low,
            Some("session-bbb"),
            "second",
            Some("dup-key"),
        );
        router.emit(a).await;
        router.emit(b).await;

        // Only one event should be in the queue: emit() drops the duplicate
        // dedupe_key before queuing.
        assert_eq!(router.queue_len().await, 1);

        let batch = router.next_speech_batch(16).await.expect("batch event");
        // Single drained event is returned as-is (no synthesis), so the
        // original `message` and `session_id` are preserved.
        assert_eq!(batch.session_id.as_deref(), Some("session-aaa"));
        assert_eq!(batch.message, "first");
        assert_eq!(router.queue_len().await, 0);
    }

    #[tokio::test]
    async fn next_speech_batch_collects_multiple_sessions() {
        let router = Arc::new(NotificationRouter::new(policy_with_turn_completed()));
        router
            .emit(make_event(
                "1",
                NotificationKind::TurnCompleted,
                NotificationPriority::Low,
                Some("parser-refactor"),
                "done",
                None,
            ))
            .await;
        router
            .emit(make_event(
                "2",
                NotificationKind::TurnCompleted,
                NotificationPriority::Low,
                Some("tui-polish-session"),
                "done",
                None,
            ))
            .await;
        router
            .emit(make_event(
                "3",
                NotificationKind::TurnCompleted,
                NotificationPriority::Low,
                Some("search-agent"),
                "done",
                None,
            ))
            .await;

        let batch = router.next_speech_batch(16).await.expect("batch event");
        // First 8 chars of each session id.
        assert!(batch.message.contains("parser-r"), "got: {}", batch.message);
        assert!(batch.message.contains("tui-poli"), "got: {}", batch.message);
        assert!(batch.message.contains("search-a"), "got: {}", batch.message);
        assert!(
            batch.message.contains("Three") || batch.message.contains("3"),
            "expected count word, got: {}",
            batch.message
        );
        assert_eq!(router.queue_len().await, 0);
    }

    #[tokio::test]
    async fn next_speech_batch_prioritizes_urgent() {
        // Use a policy that speaks both kinds so the batch covers both events.
        let mut policy = NotificationPolicy::default();
        policy.speak_kinds.insert(NotificationKind::TurnCompleted);
        let router = Arc::new(NotificationRouter::new(policy));
        router
            .emit(make_event(
                "1",
                NotificationKind::TurnCompleted,
                NotificationPriority::Low,
                Some("parser-refactor"),
                "done",
                None,
            ))
            .await;
        router
            .emit(make_event(
                "2",
                NotificationKind::PermissionRequired,
                NotificationPriority::Urgent,
                Some("search-agent"),
                "needs approval",
                None,
            ))
            .await;

        let batch = router
            .next_speech_batch(16)
            .await
            .expect("batch event should be produced");
        let urgent_pos = batch
            .message
            .find("search-a")
            .expect("urgent session prefix in message");
        let low_pos = batch
            .message
            .find("parser-r")
            .expect("low session prefix in message");
        assert!(
            urgent_pos < low_pos,
            "urgent event should appear before low event: {}",
            batch.message
        );
        assert_eq!(batch.priority, NotificationPriority::Urgent);
    }

    #[tokio::test]
    async fn next_speech_batch_respects_max_items() {
        let router = Arc::new(NotificationRouter::new(policy_with_turn_completed()));
        // Use unique first-8-char prefixes so the test can identify which
        // events are in the synthesized message.
        let ids = [
            "alpha-0001",
            "bravo-0002",
            "charlie-03",
            "delta-0004",
            "echo-005",
        ];
        for (i, sid) in ids.iter().enumerate() {
            router
                .emit(make_event(
                    &format!("{i}"),
                    NotificationKind::TurnCompleted,
                    NotificationPriority::Low,
                    Some(sid),
                    "done",
                    None,
                ))
                .await;
        }

        let batch = router.next_speech_batch(2).await.expect("batch event");
        assert!(batch.message.contains("alpha-00"), "got: {}", batch.message);
        assert!(batch.message.contains("bravo-00"), "got: {}", batch.message);
        assert!(!batch.message.contains("charlie"), "got: {}", batch.message);
        assert!(!batch.message.contains("delta-00"), "got: {}", batch.message);
        assert!(!batch.message.contains("echo-005"), "got: {}", batch.message);
        assert_eq!(router.queue_len().await, 3);
    }

    #[tokio::test]
    async fn next_speech_batch_priority_preserved_within_kind() {
        let router = Arc::new(NotificationRouter::new(NotificationPolicy::default()));
        // All events share the same kind (TurnFailed). Priorities differ.
        router
            .emit(make_event(
                "1",
                NotificationKind::TurnFailed,
                NotificationPriority::Normal,
                Some("alpha-session"),
                "low-prio-fail",
                None,
            ))
            .await;
        router
            .emit(make_event(
                "2",
                NotificationKind::TurnFailed,
                NotificationPriority::High,
                Some("beta-session"),
                "high-prio-fail",
                None,
            ))
            .await;
        router
            .emit(make_event(
                "3",
                NotificationKind::TurnFailed,
                NotificationPriority::Normal,
                Some("gamma-session"),
                "another-low",
                None,
            ))
            .await;

        let batch = router.next_speech_batch(16).await.expect("batch event");
        let high_pos = batch
            .message
            .find("beta-ses")
            .expect("high priority event in message");
        let first_pos = batch
            .message
            .find("alpha-se")
            .expect("first event in message");
        let gamma_pos = batch
            .message
            .find("gamma-se")
            .expect("third event in message");
        assert!(
            high_pos < first_pos && high_pos < gamma_pos,
            "high priority should be first in same-kind batch: {}",
            batch.message
        );
        assert_eq!(batch.priority, NotificationPriority::High);
    }

    #[tokio::test]
    async fn next_speech_batch_empty_returns_none() {
        let router = Arc::new(NotificationRouter::new(NotificationPolicy::default()));
        let batch = router.next_speech_batch(16).await;
        assert!(batch.is_none());
    }

    #[tokio::test]
    async fn next_speech_batch_zero_max_items_returns_none() {
        let router = Arc::new(NotificationRouter::new(NotificationPolicy::default()));
        router
            .emit(make_event(
                "1",
                NotificationKind::PermissionRequired,
                NotificationPriority::Urgent,
                Some("session-aaa"),
                "perm",
                None,
            ))
            .await;
        let batch = router.next_speech_batch(0).await;
        assert!(batch.is_none());
        assert_eq!(router.queue_len().await, 1);
    }

    #[tokio::test]
    async fn next_speech_batch_single_event_returned_as_is() {
        let router = Arc::new(NotificationRouter::new(NotificationPolicy::default()));
        router
            .emit(make_event(
                "1",
                NotificationKind::PermissionRequired,
                NotificationPriority::Urgent,
                Some("session-aaa"),
                "original message text",
                None,
            ))
            .await;

        let event = router.next_speech_batch(16).await.expect("event");
        assert_eq!(event.kind, NotificationKind::PermissionRequired);
        assert!(event.message.contains("original message text"));
        assert_eq!(router.queue_len().await, 0);
    }

    #[test]
    fn render_speech_batch_single_event() {
        let event = make_event(
            "1",
            NotificationKind::TurnCompleted,
            NotificationPriority::Normal,
            Some("alpha-session"),
            "build green",
            None,
        );
        let message = render_speech_batch(&[event]);
        // First 8 chars of "alpha-session" is "alpha-se".
        assert!(message.contains("alpha-se"), "got: {}", message);
        assert!(message.contains("build green"), "got: {}", message);
    }

    #[test]
    fn render_speech_batch_mixed_kinds() {
        let events = vec![
            make_event(
                "1",
                NotificationKind::TurnCompleted,
                NotificationPriority::Low,
                Some("parser-refactor"),
                "done",
                None,
            ),
            make_event(
                "2",
                NotificationKind::TurnFailed,
                NotificationPriority::High,
                Some("tui-polish"),
                "boom",
                None,
            ),
        ];
        let message = render_speech_batch(&events);
        // First 8 chars: "parser-r" and "tui-poli".
        assert!(message.contains("parser-r"), "got: {}", message);
        assert!(message.contains("tui-poli"), "got: {}", message);
        assert!(message.contains("2") || message.contains("Two"), "got: {}", message);
        assert!(message.contains("attention"), "got: {}", message);
    }
}
