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
    pub lanes: BTreeMap<WorkspaceId, WorkspaceLane>,
    /// Last workspace admitted from this class. `None` means
    /// "admit any". Compared by workspace id so persistent across
    /// class reorders.
    pub cursor: Option<WorkspaceId>,
}

impl LaneQueue {
    pub fn new(class: PriorityClass) -> Self {
        Self {
            class,
            lanes: BTreeMap::new(),
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
    ///
    /// Returns a mutable reference to the lane containing the head
    /// entry; the caller pops. `WorkspaceId` derives `Ord` so the
    /// `BTreeMap` iteration order is already sorted.
    pub fn select_next(&mut self) -> Option<&mut WorkspaceLane> {
        if self.lanes.is_empty() {
            return None;
        }
        // First pass: find a non-empty lane whose id differs from the cursor.
        let pick = self
            .lanes
            .keys()
            .find(|ws| {
                if let Some(c) = &self.cursor {
                    if *ws == c {
                        return false;
                    }
                }
                !self.lanes.get(*ws).map(|l| l.is_empty()).unwrap_or(true)
            })
            .cloned();
        // Fallback: every non-empty lane is the cursor, so admit from
        // the cursor's lane (anti-starvation).
        let pick = pick.or_else(|| {
            self.lanes
                .keys()
                .find(|ws| !self.lanes.get(*ws).map(|l| l.is_empty()).unwrap_or(true))
                .cloned()
        });
        pick.map(|ws| self.lanes.get_mut(&ws).expect("lane exists"))
    }

    pub fn admit(&mut self, entry: QueueEntry) {
        let ws = entry.workspace_id.clone();
        let lane = self
            .lanes
            .entry(ws.clone())
            .or_insert_with(|| WorkspaceLane::new(ws.clone()));
        lane.push(entry);
    }

    pub fn remove_by_id(&mut self, job_id: &JobId) -> Option<QueueEntry> {
        let keys: Vec<WorkspaceId> = self.lanes.keys().cloned().collect();
        for ws in keys {
            if let Some(removed) = self.remove_by_id_in_workspace(job_id, &ws) {
                return Some(removed);
            }
        }
        None
    }

    fn remove_by_id_in_workspace(
        &mut self,
        job_id: &JobId,
        workspace_id: &WorkspaceId,
    ) -> Option<QueueEntry> {
        let lane = self.lanes.get_mut(workspace_id)?;
        let pos = lane.entries.iter().position(|e| &e.job_id == job_id)?;
        lane.entries.remove(pos)
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
        if let Some(workspace_id) = self.job_index.get(&entry.job_id).cloned() {
            // The job is indexed. If we can't find it in any lane it
            // means the index is stale (e.g. a prior `remove` partially
            // succeeded, or a concurrent reconcile desynced state).
            // Repair the index and fall through to a normal insert
            // rather than fabricating a "previous" entry out of the
            // new payload — callers rely on the dedup signal being
            // truthful.
            let existing = self
                .lanes
                .values()
                .filter_map(|queue| queue.lanes.get(&workspace_id))
                .flat_map(|lane| lane.entries.iter())
                .find(|queued| queued.job_id == entry.job_id)
                .cloned();
            match existing {
                Some(previous) => return Ok(Some(previous)),
                None => {
                    tracing::warn!(
                        job_id = %entry.job_id,
                        workspace_id = %workspace_id,
                        "fair_queue insert: stale job_index repaired"
                    );
                    self.job_index.remove(&entry.job_id);
                }
            }
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
        let workspace_id = entry.workspace_id.clone();
        let job_id = entry.job_id.clone();
        queue.admit(entry);
        self.per_workspace_count
            .entry(workspace_id.clone())
            .and_modify(|c| *c += 1)
            .or_insert(1);
        self.total_count += 1;
        self.job_index.insert(job_id, workspace_id);
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
        if let Some(workspace_id) = self.job_index.get(job_id).cloned() {
            // First try the indexed workspace — the common path. If
            // we miss, fall back to scanning every workspace in case
            // the index points at the wrong workspace (rare race /
            // storage drift).
            let mut found = false;
            for queue in self.lanes.values_mut() {
                if let Some(entry) = queue.remove_by_id_in_workspace(job_id, &workspace_id) {
                    removed = Some(entry);
                    found = true;
                    break;
                }
            }
            if !found {
                tracing::warn!(
                    job_id = %job_id,
                    workspace_id = %workspace_id,
                    "fair_queue remove: index miss, falling back to full scan"
                );
                for queue in self.lanes.values_mut() {
                    if let Some(entry) = queue.remove_by_id(job_id) {
                        removed = Some(entry);
                        break;
                    }
                }
            }
        } else {
            // No index entry — also possible under drift; scan
            // all queues as a last resort so leaks don't accumulate.
            for queue in self.lanes.values_mut() {
                if let Some(entry) = queue.remove_by_id(job_id) {
                    removed = Some(entry);
                    break;
                }
            }
        }
        if let Some(entry) = removed {
            // Always trust the entry's workspace_id (which is the
            // actual lane we removed from) over the index — if the
            // index was stale we may have decremented the wrong
            // workspace's counter via the path that consulted it.
            self.job_index.remove(&entry.job_id);
            if let Some(c) = self.per_workspace_count.get_mut(&entry.workspace_id) {
                *c = c.saturating_sub(1);
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

        let class = self.pick_class()?;
        // Drop the borrow on `self.lanes` before mutating other fields.
        let entry = {
            let queue = self.lanes.get_mut(&class).expect("class exists");
            let lane = queue.select_next()?;
            if lane.entries.is_empty() {
                return None;
            }
            lane.entries.pop_front().expect("non-empty")
        };
        // update counters
        if let Some(c) = self.per_workspace_count.get_mut(&entry.workspace_id) {
            *c = c.saturating_sub(1);
        }
        self.total_count = self.total_count.saturating_sub(1);
        self.job_index.remove(&entry.job_id);

        // burst accounting
        self.bump_burst(class);
        if let Some(queue) = self.lanes.get_mut(&class) {
            queue.cursor = Some(entry.workspace_id.clone());
        }

        Some(SelectionOutcome { entry, class })
    }

    /// Decide which priority class the scheduler should draw from,
    /// applying anti-starvation. Pure read of `self.high_priority_burst`
    /// and the lanes; safe to share between `select_next` and
    /// `peek_candidates`.
    fn pick_class(&self) -> Option<PriorityClass> {
        let max_burst = self.cfg.fairness.max_high_priority_burst.max(1);
        for (class, queue) in self.lanes.iter() {
            if queue.total() == 0 {
                continue;
            }
            if matches!(class, PriorityClass::Urgent | PriorityClass::Interactive) {
                if self.high_priority_burst < max_burst {
                    return Some(*class);
                }
                // burst exceeded: only pick high-priority if no
                // lower-priority entry exists.
                let has_lower = self.lanes.iter().any(|(c, q)| c > class && q.total() > 0);
                if !has_lower {
                    return Some(*class);
                }
                // else keep iterating; a lower class will be picked.
            } else {
                return Some(*class);
            }
        }
        None
    }

    /// Apply the per-class admission burst accounting rules.
    fn bump_burst(&mut self, class: PriorityClass) {
        if matches!(class, PriorityClass::Urgent | PriorityClass::Interactive) {
            self.high_priority_burst = self.high_priority_burst.saturating_add(1);
        } else {
            self.high_priority_burst = 0;
        }
    }

    /// Bounded peek: returns up to N candidates (oldest first) the
    /// scheduler may try to admit, in selection order. The scheduler
    /// walks this list when one candidate is blocked, advancing to
    /// the next. Peek is idempotent — queue order, per-lane cursors,
    /// and burst accounting are restored after the call returns.
    pub fn peek_candidates(&mut self, limit: usize) -> Vec<QueueEntry> {
        let mut out: Vec<QueueEntry> = Vec::with_capacity(limit);
        if self.total_count == 0 || limit == 0 {
            return out;
        }
        let saved_burst = self.high_priority_burst;
        let saved_cursors: BTreeMap<PriorityClass, Option<WorkspaceId>> = self
            .lanes
            .iter()
            .map(|(c, q)| (*c, q.cursor.clone()))
            .collect();

        for _ in 0..limit {
            let class = match self.pick_class() {
                Some(c) => c,
                None => break,
            };
            let head = {
                let queue = match self.lanes.get_mut(&class) {
                    Some(q) => q,
                    None => break,
                };
                let lane = match queue.select_next() {
                    Some(l) => l,
                    None => break,
                };
                if lane.entries.is_empty() {
                    break;
                }
                lane.entries.front().cloned()
            };
            let Some(entry) = head else {
                break;
            };
            // Advance cursor + burst virtually so subsequent peeks
            // walk the same selection order as `select_next` would.
            if let Some(queue) = self.lanes.get_mut(&class) {
                queue.cursor = Some(entry.workspace_id.clone());
            }
            self.bump_burst(class);
            out.push(entry);
        }

        // Restore state — peek must be idempotent.
        self.high_priority_burst = saved_burst;
        for (class, cursor) in saved_cursors {
            if let Some(q) = self.lanes.get_mut(&class) {
                q.cursor = cursor;
            }
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
    use chrono::{TimeZone, Utc};
    use codegg_core::jobs::JobId;

    fn test_now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    }

    fn cfg() -> ResolvedSchedulerConfig {
        ResolvedSchedulerConfig::default()
    }

    fn entry(prio: JobPriority, ws: &str) -> QueueEntry {
        let now = test_now();
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
        let now = test_now();
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
        assert!(prior.is_some());
        assert_eq!(q.total(), 1);
    }

    #[test]
    fn insert_does_not_advance_cursor() {
        let mut q = LaneQueue::new(PriorityClass::Normal);
        q.admit(entry(JobPriority::Normal, "ws1"));
        q.admit(entry(JobPriority::Normal, "ws2"));
        assert!(q.cursor.is_none());
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
        e.submitted_at = test_now() - chrono::Duration::seconds(15);
        q.insert(e).unwrap();
        q.recompute_aging(test_now());
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
        e.submitted_at = test_now() - chrono::Duration::seconds(7);
        q.insert(e).unwrap();
        q.recompute_aging(test_now());
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

    #[test]
    fn peek_candidates_does_not_consume_high_priority_burst() {
        let mut cfg = cfg();
        cfg.fairness.max_high_priority_burst = 2;
        let mut q = FairJobQueue::new(cfg);
        for i in 0..3 {
            q.insert(unique_entry(
                JobPriority::Urgent,
                &format!("urgent-{i}"),
                "job",
            ))
            .unwrap();
        }
        q.insert(unique_entry(JobPriority::Normal, "normal", "job"))
            .unwrap();

        let _ = q.peek_candidates(2);
        assert_eq!(q.select_next().unwrap().class, PriorityClass::Urgent);
        assert_eq!(q.select_next().unwrap().class, PriorityClass::Urgent);
        assert_eq!(q.select_next().unwrap().class, PriorityClass::Normal);
    }

    /// B1 regression: peek must preserve lane ordering. A lane that
    /// had `[A, B, C]` front-to-back must still be `[A, B, C]` after
    /// `peek_candidates(1)` (and any other call). The previous
    /// implementation popped + re-inserted via `insert()`, which
    /// pushed to the back and rotated the order.
    #[test]
    fn peek_candidates_preserves_lane_ordering() {
        let mut q = FairJobQueue::new(cfg());
        let a = unique_entry(JobPriority::Normal, "ws1", "a");
        let b = unique_entry(JobPriority::Normal, "ws1", "b");
        let c = unique_entry(JobPriority::Normal, "ws1", "c");
        let a_id = a.job_id.clone();
        let b_id = b.job_id.clone();
        let c_id = c.job_id.clone();
        q.insert(a).unwrap();
        q.insert(b).unwrap();
        q.insert(c).unwrap();

        // Peek must be a no-op on ordering.
        let peeked = q.peek_candidates(1);
        assert_eq!(peeked.len(), 1);
        assert_eq!(peeked[0].job_id, a_id);

        // Drain and confirm the original FIFO is intact.
        let first = q.select_next().unwrap();
        assert_eq!(first.entry.job_id, a_id);
        let second = q.select_next().unwrap();
        assert_eq!(second.entry.job_id, b_id);
        let third = q.select_next().unwrap();
        assert_eq!(third.entry.job_id, c_id);
    }

    /// B1 regression: peek must restore per-lane cursors so the
    /// next real `select_next` lands on the same workspace the
    /// cursor was pointing at before the peek.
    #[test]
    fn peek_candidates_does_not_advance_lane_cursors() {
        let mut q = FairJobQueue::new(cfg());
        q.insert(unique_entry(JobPriority::Normal, "ws1", "a"))
            .unwrap();
        q.insert(unique_entry(JobPriority::Normal, "ws1", "b"))
            .unwrap();
        q.insert(unique_entry(JobPriority::Normal, "ws2", "a"))
            .unwrap();
        // Pop ws1-normal-a so cursor points at ws1 and ws1 lane
        // still has ws1-normal-b left.
        let first = q.select_next().unwrap();
        assert_eq!(first.entry.job_id.as_str(), "ws1-normal-a");

        // Peek should not perturb the cursor: the next non-cursor
        // workspace alphabetically is ws2.
        let _ = q.peek_candidates(3);
        let next = q.select_next().unwrap();
        assert_eq!(next.entry.workspace_id.as_str(), "ws2");
    }

    /// B2 regression: a stale `job_index` (claiming the job lives in
    /// a workspace where the lane doesn't have it) must NOT cause
    /// `insert` to fabricate a fake "previous" out of the new
    /// payload. It should repair the index and insert as if fresh.
    #[test]
    fn insert_repairs_stale_job_index() {
        let mut q = FairJobQueue::new(cfg());
        let e = entry(JobPriority::Normal, "ws1");
        let id = e.job_id.clone();
        // Pre-poison the index without actually inserting the entry
        // into any lane.
        q.job_index.insert(id.clone(), e.workspace_id.clone());
        q.total_count = 0; // pretend the lane was drained elsewhere
        q.per_workspace_count.insert(e.workspace_id.clone(), 0);

        // insert should detect the desync, repair the index, and
        // treat the entry as a fresh insert (Ok(None)).
        let previous = q.insert(e).unwrap();
        assert!(
            previous.is_none(),
            "stale index must not fabricate a previous entry"
        );
        assert_eq!(q.total(), 1);
        // Index now points at ws1.
        assert_eq!(q.job_index.get(&id).map(|w| w.as_str()), Some("ws1"));
    }

    /// B3 regression: `remove` with a stale `job_index` pointing at
    /// the wrong workspace must still find and remove the entry by
    /// falling back to a full scan, rather than leaking it.
    #[test]
    fn remove_falls_back_to_full_scan_on_index_miss() {
        let mut q = FairJobQueue::new(cfg());
        let e = entry(JobPriority::Normal, "ws1");
        let id = e.job_id.clone();
        q.insert(e.clone()).unwrap();

        // Poison: rewrite the index to point at a wrong workspace
        // and bump the per-workspace count for ws1 so a naive remove
        // would desync.
        q.job_index
            .insert(id.clone(), WorkspaceId::new_unchecked("bogus"));

        let removed = q.remove(&id, QueueRemovalReason::Dropped);
        assert!(removed.is_some());
        assert_eq!(q.total(), 0);
        // Index cleared, counter repaired.
        assert!(!q.job_index.contains_key(&id));
        assert_eq!(q.per_workspace_count.get(&e.workspace_id).copied(), Some(0));
    }
}
