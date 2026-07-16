//! The fair queue itself.
//!
//! Selection is deterministic. The scheduler loops priority classes
//! from highest to lowest. Within a class, it round-robins across
//! workspace lanes using a cursor that persists across calls. Within
//! a lane, entries are FIFO. After `max_high_priority_burst`
//! consecutive high-priority admissions, the queue must admit at
//! least one eligible lower-priority entry (anti-starvation).
//!
//! Aging elevates the effective priority class. A `Normal` job older
//! than `aging_secs` is treated as `Interactive` for selection; the
//! persisted priority is never modified.

use std::collections::{BTreeMap, HashMap, VecDeque};

use codegg_core::jobs::{JobId, JobPriority};
use codegg_core::workspace::WorkspaceId;

use crate::scheduler::config::ResolvedSchedulerConfig;
use crate::scheduler::types::{QueueEntry, QueueInsertError, QueueRemovalReason};

/// Effective priority class. The scheduler uses this (not the
/// persisted `JobPriority`) to pick the next entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PriorityClass {
    Urgent = 0,
    Interactive = 1,
    Normal = 2,
    Background = 3,
    Maintenance = 4,
}

impl PriorityClass {
    pub fn from_priority(p: JobPriority) -> Self {
        match p {
            JobPriority::Urgent => PriorityClass::Urgent,
            JobPriority::Interactive => PriorityClass::Interactive,
            JobPriority::Normal => PriorityClass::Normal,
            JobPriority::Background => PriorityClass::Background,
            JobPriority::Maintenance => PriorityClass::Maintenance,
        }
    }

    /// Apply aging: after `aging_secs`, promote by one class. Cap at
    /// Urgent (we never promote above the highest persisted class).
    pub fn with_aging(p: JobPriority, age_secs: u64, aging_secs: u64) -> Self {
        let base = PriorityClass::from_priority(p);
        if aging_secs == 0 || age_secs < aging_secs {
            return base;
        }
        let promotions = (age_secs / aging_secs).min(3) as i32;
        let new_rank = (base as i32).saturating_sub(promotions).max(0);
        match new_rank {
            0 => PriorityClass::Urgent,
            1 => PriorityClass::Interactive,
            2 => PriorityClass::Normal,
            3 => PriorityClass::Background,
            _ => PriorityClass::Maintenance,
        }
    }
}

/// One workspace lane inside one priority class.
#[derive(Debug)]
pub struct WorkspaceLane {
    pub workspace_id: WorkspaceId,
    pub entries: VecDeque<QueueEntry>,
}

impl WorkspaceLane {
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            entries: VecDeque::new(),
        }
    }

    pub fn push(&mut self, entry: QueueEntry) {
        self.entries.push_back(entry);
    }

    pub fn pop_front(&mut self) -> Option<QueueEntry> {
        self.entries.pop_front()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One priority lane. Holds ordered workspace lanes and a round-robin
/// cursor for fairness across workspaces within the same class.
#[derive(Debug)]
pub struct LaneQueue {
    pub class: PriorityClass,
    pub lanes: HashMap<WorkspaceId, WorkspaceLane>,
    /// Last workspace admitted from this class. `None` means
    /// "admit any". Compared by workspace id so persistent across
    /// class reorders.
    pub cursor: Option<WorkspaceId>,
}

impl LaneQueue {
    pub fn new(class: PriorityClass) -> Self {
        Self {
            class,
            lanes: HashMap::new(),
            cursor: None,
        }
    }

    pub fn total(&self) -> usize {
        self.lanes.values().map(|l| l.len()).sum()
    }

    /// Pick the next entry using round-robin across workspaces,
    /// skipping empty lanes, and skipping lanes whose `WorkspaceId`
    /// matches the cursor's previous pick (so the cursor advances
    /// when an entry is admitted).
    pub fn select_next(&mut self) -> Option<&mut WorkspaceLane> {
        if self.lanes.is_empty() {
            return None;
        }
        // Iterate in stable insertion order; WorkspaceId is not Ord.
        let cursor = self.cursor.clone();
        let mut keys: Vec<WorkspaceId> = self.lanes.keys().cloned().collect();
        // Stable secondary order: by string repr. This is
        // deterministic for tests.
        keys.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut pick: Option<WorkspaceId> = None;
        for ws in &keys {
            if let Some(c) = &cursor {
                if ws == c {
                    continue;
                }
            }
            if !self.lanes.get(ws).map(|l| l.is_empty()).unwrap_or(true) {
                pick = Some(ws.clone());
                break;
            }
        }
        // Fallback: every non-empty lane is the cursor, so admit from
        // the cursor's lane and clear it (anti-starvation).
        if pick.is_none() {
            for ws in &keys {
                if !self.lanes.get(ws).map(|l| l.is_empty()).unwrap_or(true) {
                    pick = Some(ws.clone());
                    break;
                }
            }
        }
        pick.map(|ws| self.lanes.get_mut(&ws).expect("lane exists"))
    }

    pub fn admit(&mut self, entry: QueueEntry) {
        let ws = entry.workspace_id.clone();
        let lane = self
            .lanes
            .entry(ws.clone())
            .or_insert_with(|| WorkspaceLane::new(ws.clone()));
        lane.push(entry);
        self.cursor = Some(ws);
    }

    pub fn remove_by_id(&mut self, job_id: &JobId) -> Option<QueueEntry> {
        let keys: Vec<WorkspaceId> = self.lanes.keys().cloned().collect();
        for ws in keys {
            if let Some(lane) = self.lanes.get_mut(&ws) {
                if let Some(pos) = lane.entries.iter().position(|e| &e.job_id == job_id) {
                    return lane.entries.remove(pos);
                }
            }
        }
        None
    }

    /// Snapshot of lane sizes per workspace for diagnostics.
    pub fn snapshot_counts(&self) -> Vec<(WorkspaceId, usize)> {
        self.lanes
            .iter()
            .map(|(ws, lane)| (ws.clone(), lane.len()))
            .collect()
    }
}

/// The fair queue. Holds one `LaneQueue` per `PriorityClass` plus a
/// per-class counter for high-priority burst tracking.
#[derive(Debug)]
pub struct FairJobQueue {
    cfg: ResolvedSchedulerConfig,
    /// Lane queues keyed by class. Empty classes are removed lazily
    /// to keep selection cheap; insertion may recreate them.
    lanes: BTreeMap<PriorityClass, LaneQueue>,
    /// Consecutive high-priority admissions since the last
    /// non-high-priority admission. Reset on every non-Urgent /
    /// non-Interactive admission.
    high_priority_burst: u32,
    /// Per-workspace queued counts for snapshot/bounds enforcement.
    per_workspace_count: HashMap<WorkspaceId, usize>,
    /// Total queued.
    total_count: usize,
    /// Map of job_id -> workspace_id so removals can update the
    /// per-workspace counter.
    job_index: HashMap<JobId, WorkspaceId>,
}

impl FairJobQueue {
    pub fn new(cfg: ResolvedSchedulerConfig) -> Self {
        Self {
            cfg,
            lanes: BTreeMap::new(),
            high_priority_burst: 0,
            per_workspace_count: HashMap::new(),
            total_count: 0,
            job_index: std::collections::HashMap::new(),
        }
    }

    pub fn config(&self) -> &ResolvedSchedulerConfig {
        &self.cfg
    }

    pub fn total(&self) -> usize {
        self.total_count
    }

    pub fn per_workspace(&self) -> &HashMap<WorkspaceId, usize> {
        &self.per_workspace_count
    }

    pub fn lanes(&self) -> &BTreeMap<PriorityClass, LaneQueue> {
        &self.lanes
    }

    /// Insert an entry. Deduplicates by job id (existing entry kept
    /// in place). Returns the previous entry if any.
    pub fn insert(&mut self, entry: QueueEntry) -> Result<Option<QueueEntry>, QueueInsertError> {
        if self.job_index.contains_key(&entry.job_id) {
            return Ok(None);
        }

        // Enforce bounds. Bounded queue: never silently drop existing
        // queued jobs. New jobs are rejected if bounds are exceeded.
        if self.total_count >= self.cfg.queue.max_total {
            return Err(QueueInsertError::Overflow);
        }
        let ws_count = self
            .per_workspace_count
            .get(&entry.workspace_id)
            .copied()
            .unwrap_or(0);
        if ws_count >= self.cfg.queue.max_per_workspace {
            return Err(QueueInsertError::Overflow);
        }

        let class = entry.effective_class;
        let queue = self
            .lanes
            .entry(class)
            .or_insert_with(|| LaneQueue::new(class));
        queue.admit(entry.clone());
        self.per_workspace_count
            .entry(entry.workspace_id.clone())
            .and_modify(|c| *c += 1)
            .or_insert(1);
        self.total_count += 1;
        self.job_index
            .insert(entry.job_id.clone(), entry.workspace_id.clone());
        Ok(None)
    }

    /// Remove by job id; returns the removed entry along with the
    /// reason it was removed (for diagnostics).
    pub fn remove(
        &mut self,
        job_id: &JobId,
        reason: QueueRemovalReason,
    ) -> Option<(QueueEntry, QueueRemovalReason)> {
        let mut removed: Option<QueueEntry> = None;
        for queue in self.lanes.values_mut() {
            if let Some(entry) = queue.remove_by_id(job_id) {
                removed = Some(entry);
                break;
            }
        }
        if let Some(entry) = removed {
            if let Some(ws) = self.job_index.remove(&entry.job_id) {
                if let Some(c) = self.per_workspace_count.get_mut(&ws) {
                    *c = c.saturating_sub(1);
                }
            }
            self.total_count = self.total_count.saturating_sub(1);
            return Some((entry, reason));
        }
        None
    }

    /// Re-evaluate aging on every entry. Called on every wake / tick.
    /// Promotes entries across classes by mutating the in-memory
    /// copy. The persisted `JobPriority` is never changed.
    pub fn recompute_aging(&mut self, now: chrono::DateTime<chrono::Utc>) {
        let mut promotions: Vec<(PriorityClass, PriorityClass, QueueEntry)> = Vec::new();
        for (class, queue) in self.lanes.iter_mut() {
            for lane in queue.lanes.values_mut() {
                for entry in lane.entries.iter_mut() {
                    let prior = entry.effective_class;
                    entry.recompute_aging(&self.cfg, now);
                    if entry.effective_class != prior {
                        promotions.push((*class, entry.effective_class, entry.clone()));
                    }
                }
            }
        }
        for (from, to, entry) in promotions {
            if let Some(q) = self.lanes.get_mut(&from) {
                q.remove_by_id(&entry.job_id);
            }
            let queue = self.lanes.entry(to).or_insert_with(|| LaneQueue::new(to));
            queue.admit(entry);
        }
    }

    /// Choose the next entry, applying anti-starvation (after
    /// `max_high_priority_burst` consecutive high-priority
    /// admissions, force a non-high-priority admission if any
    /// eligible entry exists).
    pub fn select_next(&mut self) -> Option<SelectionOutcome> {
        if self.total_count == 0 {
            return None;
        }
        let max_burst = self.cfg.fairness.max_high_priority_burst.max(1);

        // Decide which class to draw from.
        let mut chosen: Option<PriorityClass> = None;
        for (class, queue) in self.lanes.iter() {
            if queue.total() == 0 {
                continue;
            }
            if matches!(class, PriorityClass::Urgent | PriorityClass::Interactive) {
                if self.high_priority_burst < max_burst {
                    chosen = Some(*class);
                    break;
                }
                // burst exceeded: only pick high-priority if no
                // lower-priority entry exists.
                let has_lower = self.lanes.iter().any(|(c, q)| c > class && q.total() > 0);
                if !has_lower {
                    chosen = Some(*class);
                    break;
                }
                // else keep iterating; a lower class will be picked.
            } else {
                chosen = Some(*class);
                break;
            }
        }

        let class = chosen?;
        let queue = self.lanes.get_mut(&class).expect("class exists");
        let lane = queue.select_next()?;
        if lane.entries.is_empty() {
            return None;
        }
        let entry = lane.entries.pop_front().expect("non-empty");
        // update counters
        if let Some(c) = self.per_workspace_count.get_mut(&entry.workspace_id) {
            *c = c.saturating_sub(1);
        }
        self.total_count = self.total_count.saturating_sub(1);
        self.job_index.remove(&entry.job_id);

        // burst accounting
        if matches!(class, PriorityClass::Urgent | PriorityClass::Interactive) {
            self.high_priority_burst = self.high_priority_burst.saturating_add(1);
        } else {
            self.high_priority_burst = 0;
        }
        queue.cursor = Some(entry.workspace_id.clone());

        Some(SelectionOutcome { entry, class })
    }

    /// Bounded peek: returns up to N candidates (oldest first) the
    /// scheduler may try to admit, in selection order. The scheduler
    /// walks this list when one candidate is blocked, advancing to
    /// the next.
    pub fn peek_candidates(&mut self, limit: usize) -> Vec<QueueEntry> {
        let mut out: Vec<QueueEntry> = Vec::with_capacity(limit);
        for _ in 0..limit {
            let entry = if let Some(outcome) = self.select_next() {
                outcome.entry
            } else {
                break;
            };
            // Re-insert to preserve queue state.
            let _ = self.insert(entry.clone());
            out.push(entry);
        }
        out
    }
}

/// The result of [`FairJobQueue::select_next`]: the entry plus the
/// class it was drawn from. The class is used for diagnostic events
/// and snapshot accounting.
#[derive(Debug, Clone)]
pub struct SelectionOutcome {
    pub entry: QueueEntry,
    pub class: PriorityClass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use codegg_core::jobs::JobId;

    fn cfg() -> ResolvedSchedulerConfig {
        ResolvedSchedulerConfig::default()
    }

    fn entry(prio: JobPriority, ws: &str) -> QueueEntry {
        let now = Utc::now();
        QueueEntry {
            job_id: JobId::new_unchecked(format!("{}-{}", ws, prio.as_str())),
            workspace_id: WorkspaceId::new_unchecked(ws.to_string()),
            priority: prio,
            submitted_at: now,
            enqueued_at: now,
            effective_class: PriorityClass::from_priority(prio),
        }
    }

    fn unique_entry(prio: JobPriority, ws: &str, suffix: &str) -> QueueEntry {
        let now = Utc::now();
        QueueEntry {
            job_id: JobId::new_unchecked(format!("{}-{}-{}", ws, prio.as_str(), suffix)),
            workspace_id: WorkspaceId::new_unchecked(ws.to_string()),
            priority: prio,
            submitted_at: now,
            enqueued_at: now,
            effective_class: PriorityClass::from_priority(prio),
        }
    }

    #[test]
    fn insert_and_remove_updates_counters() {
        let mut q = FairJobQueue::new(cfg());
        q.insert(unique_entry(JobPriority::Normal, "ws1", "a"))
            .unwrap();
        q.insert(unique_entry(JobPriority::Normal, "ws1", "b"))
            .unwrap();
        assert_eq!(q.total(), 2);
        let job = JobId::new_unchecked("ws1-normal-a");
        let removed = q.remove(&job, QueueRemovalReason::Admitted);
        assert!(removed.is_some());
        assert_eq!(q.total(), 1);
    }

    #[test]
    fn dedup_by_job_id() {
        let mut q = FairJobQueue::new(cfg());
        q.insert(entry(JobPriority::Normal, "ws1")).unwrap();
        let prior = q.insert(entry(JobPriority::Normal, "ws1")).unwrap();
        assert!(prior.is_none()); // no prior entry (already present, dedup)
        assert_eq!(q.total(), 1);
    }

    #[test]
    fn overflow_returns_error() {
        let mut cfg = cfg();
        cfg.queue.max_total = 1;
        let mut q = FairJobQueue::new(cfg);
        q.insert(entry(JobPriority::Normal, "ws1")).unwrap();
        let err = q.insert(entry(JobPriority::Normal, "ws2")).unwrap_err();
        assert_eq!(err, QueueInsertError::Overflow);
    }

    #[test]
    fn round_robin_within_class() {
        let mut q = FairJobQueue::new(cfg());
        q.insert(entry(JobPriority::Normal, "ws1")).unwrap();
        q.insert(entry(JobPriority::Normal, "ws2")).unwrap();
        let a = q.select_next().unwrap();
        let b = q.select_next().unwrap();
        let c = q.select_next();
        assert_eq!(a.entry.workspace_id.as_str(), "ws1");
        assert_eq!(b.entry.workspace_id.as_str(), "ws2");
        assert!(c.is_none());
    }

    #[test]
    fn urgent_admitted_before_normal() {
        let mut q = FairJobQueue::new(cfg());
        q.insert(entry(JobPriority::Normal, "ws1")).unwrap();
        q.insert(entry(JobPriority::Urgent, "ws2")).unwrap();
        let a = q.select_next().unwrap();
        assert_eq!(a.entry.priority, JobPriority::Urgent);
    }

    #[test]
    fn aging_promotes_old_entries() {
        let mut cfg = cfg();
        cfg.fairness.aging_secs = 5;
        let mut q = FairJobQueue::new(cfg);
        let mut e = entry(JobPriority::Normal, "ws1");
        e.submitted_at = Utc::now() - chrono::Duration::seconds(15);
        q.insert(e).unwrap();
        q.recompute_aging(Utc::now());
        // After aging, Normal (rank 2) promoted by 3 -> rank 0 (Urgent).
        let a = q.select_next().unwrap();
        assert_eq!(a.class, PriorityClass::Urgent);
    }

    #[test]
    fn aging_promotes_one_class_when_in_window() {
        let mut cfg = cfg();
        cfg.fairness.aging_secs = 5;
        let mut q = FairJobQueue::new(cfg);
        let mut e = entry(JobPriority::Normal, "ws1");
        e.submitted_at = Utc::now() - chrono::Duration::seconds(7);
        q.insert(e).unwrap();
        q.recompute_aging(Utc::now());
        // After one aging window, Normal (rank 2) -> Interactive (rank 1).
        let a = q.select_next().unwrap();
        assert_eq!(a.class, PriorityClass::Interactive);
    }

    #[test]
    fn burst_floor_prevents_starvation() {
        let mut cfg = cfg();
        cfg.fairness.max_high_priority_burst = 2;
        let mut q = FairJobQueue::new(cfg);
        // 4 urgent, 1 normal
        for i in 0..4 {
            q.insert(entry(JobPriority::Urgent, &format!("u{i}")))
                .unwrap();
        }
        q.insert(entry(JobPriority::Normal, "n1")).unwrap();

        let mut classes: Vec<PriorityClass> = Vec::new();
        for _ in 0..5 {
            if let Some(s) = q.select_next() {
                classes.push(s.class);
            }
        }
        // After 2 high-priority admissions, the next pick must be
        // Normal (or any non-Urgent/Interactive).
        assert!(classes[0] == PriorityClass::Urgent);
        assert!(classes[1] == PriorityClass::Urgent);
        assert!(classes[2] != PriorityClass::Urgent);
    }

    #[test]
    fn peek_candidates_does_not_drain() {
        let mut q = FairJobQueue::new(cfg());
        for i in 0..3 {
            q.insert(entry(JobPriority::Normal, &format!("w{i}")))
                .unwrap();
        }
        let _ = q.peek_candidates(3);
        assert_eq!(q.total(), 3);
    }
}
