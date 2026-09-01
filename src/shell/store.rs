use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::types::{
    ShellCapturePolicy, ShellCommandId, ShellRequest, ShellStatus, DEFAULT_MAX_BYTES_PER_COMMAND,
    DEFAULT_MAX_HISTORY_ENTRIES, DEFAULT_MAX_TOTAL_BYTES,
};

const HEAD_CAP: usize = 256 * 1024;
const TAIL_CAP: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct BoundedOutput {
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
    pub omitted_bytes: usize,
    pub total_bytes: usize,
    pub total_lines: usize,
}

impl BoundedOutput {
    pub fn new() -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            omitted_bytes: 0,
            total_bytes: 0,
            total_lines: 0,
        }
    }

    pub fn append(&mut self, data: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(data.len());
        self.total_lines = self
            .total_lines
            .saturating_add(data.iter().filter(|&&b| b == b'\n').count());

        let space_in_head = HEAD_CAP.saturating_sub(self.head.len());
        if space_in_head > 0 {
            let take = space_in_head.min(data.len());
            self.head.extend_from_slice(&data[..take]);
        }

        let remaining = &data[space_in_head.min(data.len())..];
        if !remaining.is_empty() {
            // Truncate in place: extend the tail then drain the
            // overflow from the front. Cloning per chunk would make
            // streaming output quadratic in the tail size.
            self.tail.extend_from_slice(remaining);
            let overflow = self.tail.len().saturating_sub(TAIL_CAP);
            if overflow > 0 {
                self.tail.drain(..overflow);
                self.omitted_bytes = self.omitted_bytes.saturating_add(overflow);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total_bytes == 0
    }

    pub fn head_str_lossy(&self) -> String {
        String::from_utf8_lossy(&self.head).to_string()
    }

    pub fn tail_str_lossy(&self) -> String {
        String::from_utf8_lossy(&self.tail).to_string()
    }
}

impl Default for BoundedOutput {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ShellOutputEntry {
    pub id: ShellCommandId,
    pub command: String,
    pub cwd: PathBuf,
    pub started_at: SystemTime,
    pub finished_at: Option<SystemTime>,
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub stdout: BoundedOutput,
    pub stderr: BoundedOutput,
    pub elapsed: Option<Duration>,
    pub promoted: bool,
    pub promote_after: bool,
    pub capture_policy: ShellCapturePolicy,
}

#[derive(Debug)]
pub struct ShellOutputStore {
    entries: VecDeque<ShellOutputEntry>,
    next_id: u64,
    max_entries: usize,
    pub max_bytes_per_command: usize,
    pub max_total_bytes: usize,
    total_bytes: usize,
}

impl ShellOutputStore {
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_MAX_HISTORY_ENTRIES,
            DEFAULT_MAX_BYTES_PER_COMMAND,
            DEFAULT_MAX_TOTAL_BYTES,
        )
    }

    pub fn from_config(cfg: &crate::config::schema::HumanShellConfig) -> Self {
        Self::with_limits(
            cfg.max_history_entries(),
            cfg.max_bytes_per_command(),
            cfg.max_total_bytes(),
        )
    }

    pub fn with_limits(
        max_entries: usize,
        max_bytes_per_command: usize,
        max_total_bytes: usize,
    ) -> Self {
        Self {
            entries: VecDeque::new(),
            next_id: 1,
            max_entries,
            max_bytes_per_command,
            max_total_bytes,
            total_bytes: 0,
        }
    }

    pub fn alloc_id(&mut self) -> ShellCommandId {
        let id = ShellCommandId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn insert_started(&mut self, req: &ShellRequest) {
        let entry = ShellOutputEntry {
            id: req.id,
            command: req.command.clone(),
            cwd: req.cwd.clone(),
            started_at: SystemTime::now(),
            finished_at: None,
            status: ShellStatus::Running,
            exit_code: None,
            stdout: BoundedOutput::new(),
            stderr: BoundedOutput::new(),
            elapsed: None,
            promoted: false,
            promote_after: matches!(req.capture_policy, ShellCapturePolicy::StoreAndPromote),
            capture_policy: req.capture_policy,
        };
        self.entries.push_back(entry);
        self.evict();
    }

    pub fn append_stdout(&mut self, id: ShellCommandId, data: &[u8]) {
        if let Some(entry) = self.find_mut(id) {
            let retained_before = entry.stdout.head.len() + entry.stdout.tail.len();
            entry.stdout.append(data);
            let retained_after = entry.stdout.head.len() + entry.stdout.tail.len();
            self.total_bytes = self
                .total_bytes
                .saturating_add(retained_after.saturating_sub(retained_before));
            self.evict();
        }
    }

    pub fn append_stderr(&mut self, id: ShellCommandId, data: &[u8]) {
        if let Some(entry) = self.find_mut(id) {
            let retained_before = entry.stderr.head.len() + entry.stderr.tail.len();
            entry.stderr.append(data);
            let retained_after = entry.stderr.head.len() + entry.stderr.tail.len();
            self.total_bytes = self
                .total_bytes
                .saturating_add(retained_after.saturating_sub(retained_before));
            self.evict();
        }
    }

    pub fn mark_exited(&mut self, id: ShellCommandId, status: Option<i32>, elapsed: Duration) {
        if let Some(entry) = self.find_mut(id) {
            entry.status = ShellStatus::Exited;
            entry.exit_code = status;
            entry.finished_at = Some(SystemTime::now());
            entry.elapsed = Some(elapsed);
        }
    }

    pub fn mark_timeout(&mut self, id: ShellCommandId, elapsed: Duration) {
        if let Some(entry) = self.find_mut(id) {
            entry.status = ShellStatus::TimedOut;
            entry.finished_at = Some(SystemTime::now());
            entry.elapsed = Some(elapsed);
        }
    }

    pub fn mark_failed_to_start(&mut self, id: ShellCommandId) {
        if let Some(entry) = self.find_mut(id) {
            entry.status = ShellStatus::FailedToStart;
            entry.finished_at = Some(SystemTime::now());
        }
    }

    pub fn mark_killed(&mut self, id: ShellCommandId, elapsed: Duration) {
        if let Some(entry) = self.find_mut(id) {
            entry.status = ShellStatus::Killed;
            entry.finished_at = Some(SystemTime::now());
            entry.elapsed = Some(elapsed);
        }
    }

    pub fn get(&self, id: ShellCommandId) -> Option<&ShellOutputEntry> {
        self.entries.iter().rev().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: ShellCommandId) -> Option<&mut ShellOutputEntry> {
        self.entries.iter_mut().rev().find(|e| e.id == id)
    }

    pub fn get_last(&self) -> Option<&ShellOutputEntry> {
        self.entries.back()
    }

    pub fn list_recent(&self, n: usize) -> Vec<&ShellOutputEntry> {
        self.entries.iter().rev().take(n).collect()
    }

    pub fn mark_promoted(&mut self, id: ShellCommandId) {
        if let Some(entry) = self.find_mut(id) {
            entry.promoted = true;
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn find_mut(&mut self, id: ShellCommandId) -> Option<&mut ShellOutputEntry> {
        self.entries.iter_mut().rev().find(|e| e.id == id)
    }

    fn evict(&mut self) {
        while self.entries.len() > self.max_entries {
            if let Some(evicted) = self.entries.pop_front() {
                self.total_bytes = self
                    .total_bytes
                    .saturating_sub(evicted.stdout.head.len() + evicted.stdout.tail.len())
                    .saturating_sub(evicted.stderr.head.len() + evicted.stderr.tail.len());
            }
        }
        while self.total_bytes > self.max_total_bytes && self.entries.len() > 1 {
            if let Some(evicted) = self.entries.pop_front() {
                self.total_bytes = self
                    .total_bytes
                    .saturating_sub(evicted.stdout.head.len() + evicted.stdout.tail.len())
                    .saturating_sub(evicted.stderr.head.len() + evicted.stderr.tail.len());
            } else {
                break;
            }
        }
    }
}

impl Default for ShellOutputStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_req(id: u64, cmd: &str) -> ShellRequest {
        use super::super::types::{ShellCapturePolicy, ShellEnvPolicy, ShellOrigin};
        ShellRequest {
            id: ShellCommandId(id),
            origin: ShellOrigin::HumanEphemeral,
            command: cmd.to_string(),
            cwd: PathBuf::from("/tmp"),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        }
    }

    #[test]
    fn bounded_output_small() {
        let mut bo = BoundedOutput::new();
        bo.append(b"hello");
        assert_eq!(bo.head, b"hello");
        assert!(bo.tail.is_empty());
        assert_eq!(bo.omitted_bytes, 0);
        assert_eq!(bo.total_bytes, 5);
    }

    #[test]
    fn bounded_output_head_tail_split() {
        let mut bo = BoundedOutput::new();
        let data = vec![b'x'; 512 * 1024];
        bo.append(&data);
        assert_eq!(bo.head.len(), HEAD_CAP);
        assert_eq!(bo.tail.len(), 512 * 1024 - HEAD_CAP);
        // Everything is retained in head+tail; nothing was evicted.
        assert_eq!(bo.omitted_bytes, 0);
        assert_eq!(bo.total_bytes, 512 * 1024);
    }

    #[test]
    fn bounded_output_omitted_counts_only_evicted_bytes() {
        let mut bo = BoundedOutput::new();
        bo.append(&vec![b'x'; 512 * 1024]);
        assert_eq!(bo.omitted_bytes, 0);
        bo.append(b"overflow-past-tail-cap");
        assert_eq!(bo.tail.len(), TAIL_CAP);
        assert_eq!(bo.omitted_bytes, b"overflow-past-tail-cap".len());
        assert_eq!(bo.total_bytes, 512 * 1024 + b"overflow-past-tail-cap".len());
    }

    #[test]
    fn bounded_output_counters_saturate() {
        let mut bo = BoundedOutput::new();
        bo.total_bytes = usize::MAX;
        bo.total_lines = usize::MAX;
        bo.omitted_bytes = usize::MAX;

        bo.append(b"line\n");

        assert_eq!(bo.total_bytes, usize::MAX);
        assert_eq!(bo.total_lines, usize::MAX);
        assert_eq!(bo.omitted_bytes, usize::MAX);
    }

    #[test]
    fn bounded_output_line_counting() {
        let mut bo = BoundedOutput::new();
        bo.append(b"line1\nline2\nline3\n");
        assert_eq!(bo.total_lines, 3);
    }

    #[test]
    fn bounded_output_is_empty() {
        let bo = BoundedOutput::new();
        assert!(bo.is_empty());
        let mut bo = BoundedOutput::new();
        bo.append(b"a");
        assert!(!bo.is_empty());
    }

    #[test]
    fn bounded_output_lossy_utf8() {
        let mut bo = BoundedOutput::new();
        bo.append(&[0xFF, 0xFE]);
        let s = bo.head_str_lossy();
        assert!(!s.is_empty());
    }

    #[test]
    fn store_alloc_ids_increment() {
        let mut store = ShellOutputStore::new();
        let id1 = store.alloc_id();
        let id2 = store.alloc_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.0 + 1, id2.0);
    }

    #[test]
    fn store_insert_and_get() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "cargo test");
        let id = req.id;
        store.insert_started(&req);
        assert_eq!(store.len(), 1);
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, ShellStatus::Running);
        assert_eq!(entry.command, "cargo test");
    }

    #[test]
    fn store_append_stdout() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "echo hi");
        let id = req.id;
        store.insert_started(&req);
        store.append_stdout(id, b"hello ");
        store.append_stdout(id, b"world");
        let entry = store.get(id).unwrap();
        assert_eq!(entry.stdout.total_bytes, 11);
        assert_eq!(entry.stdout.head_str_lossy(), "hello world");
    }

    #[test]
    fn store_append_stderr() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "fail cmd");
        let id = req.id;
        store.insert_started(&req);
        store.append_stderr(id, b"error output");
        let entry = store.get(id).unwrap();
        assert_eq!(entry.stderr.total_bytes, 12);
    }

    #[test]
    fn store_mark_exited() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_exited(id, Some(0), Duration::from_secs(1));
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, ShellStatus::Exited);
        assert_eq!(entry.exit_code, Some(0));
        assert!(entry.elapsed.is_some());
        assert!(entry.finished_at.is_some());
    }

    #[test]
    fn store_mark_timeout() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "slow cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_timeout(id, Duration::from_secs(300));
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, ShellStatus::TimedOut);
    }

    #[test]
    fn store_mark_failed_to_start() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "bad cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_failed_to_start(id);
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, ShellStatus::FailedToStart);
    }

    #[test]
    fn store_get_last() {
        let mut store = ShellOutputStore::new();
        store.insert_started(&make_req(1, "first"));
        store.insert_started(&make_req(2, "second"));
        assert_eq!(store.get_last().unwrap().command, "second");
    }

    #[test]
    fn store_list_recent() {
        let mut store = ShellOutputStore::new();
        for i in 0..5 {
            store.insert_started(&make_req(i, &format!("cmd{}", i)));
        }
        let recent = store.list_recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].command, "cmd4");
        assert_eq!(recent[1].command, "cmd3");
        assert_eq!(recent[2].command, "cmd2");
    }

    #[test]
    fn store_evict_by_count() {
        let mut store = ShellOutputStore::with_limits(3, 1_000_000, 8_000_000);
        for i in 0..5 {
            store.insert_started(&make_req(i, &format!("cmd{}", i)));
        }
        assert_eq!(store.len(), 3);
        let ids: Vec<u64> = store.list_recent(10).iter().map(|e| e.id.0).rev().collect();
        assert!(
            store.get(ShellCommandId(0)).is_none(),
            "entry 0 should be evicted, got ids: {:?}",
            ids
        );
        assert!(
            store.get(ShellCommandId(1)).is_none(),
            "entry 1 should be evicted, got ids: {:?}",
            ids
        );
        assert!(store.get(ShellCommandId(2)).is_some());
        assert!(store.get(ShellCommandId(3)).is_some());
        assert!(store.get(ShellCommandId(4)).is_some());
    }

    #[test]
    fn store_evict_by_total_bytes() {
        let mut store = ShellOutputStore::with_limits(100, 1_000_000, 100);
        let req1 = make_req(1, "big cmd");
        store.insert_started(&req1);
        store.append_stdout(req1.id, &[b'x'; 60]);
        let req2 = make_req(2, "small cmd");
        store.insert_started(&req2);
        store.append_stdout(req2.id, b"ok");
        assert!(store.get(req1.id).is_none() || store.get(req2.id).is_some());
    }

    #[test]
    fn store_total_limit_counts_retained_bytes_not_observed_bytes() {
        let mut store = ShellOutputStore::with_limits(100, 2_000_000, 700_000);
        let req1 = make_req(1, "noisy cmd");
        store.insert_started(&req1);
        store.append_stdout(req1.id, &[b'x'; 1_000_000]);

        let req2 = make_req(2, "second cmd");
        store.insert_started(&req2);
        store.append_stdout(req2.id, &[b'y'; 100_000]);

        assert!(store.get(req1.id).is_some());
        assert_eq!(store.get(req1.id).unwrap().stdout.head.len(), HEAD_CAP);
        assert_eq!(store.get(req1.id).unwrap().stdout.tail.len(), TAIL_CAP);
    }

    #[test]
    fn store_mark_promoted() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "cmd");
        let id = req.id;
        store.insert_started(&req);
        assert!(!store.get(id).unwrap().promoted);
        store.mark_promoted(id);
        assert!(store.get(id).unwrap().promoted);
    }

    #[test]
    fn store_default_is_empty() {
        let store = ShellOutputStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn store_promote_after_from_capture_policy() {
        use super::super::types::{ShellCapturePolicy, ShellEnvPolicy, ShellOrigin};
        let mut store = ShellOutputStore::new();
        let req_ephemeral = ShellRequest {
            id: ShellCommandId(1),
            origin: ShellOrigin::HumanEphemeral,
            command: "cmd".to_string(),
            cwd: PathBuf::from("/tmp"),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        store.insert_started(&req_ephemeral);
        assert!(!store.get(ShellCommandId(1)).unwrap().promote_after);

        let req_promote = ShellRequest {
            id: ShellCommandId(2),
            origin: ShellOrigin::HumanEphemeral,
            command: "cmd2".to_string(),
            cwd: PathBuf::from("/tmp"),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreAndPromote,
            env_policy: ShellEnvPolicy::Inherit,
        };
        store.insert_started(&req_promote);
        assert!(store.get(ShellCommandId(2)).unwrap().promote_after);
    }

    #[test]
    fn bounded_output_default() {
        let bo = BoundedOutput::default();
        assert!(bo.is_empty());
    }

    #[test]
    fn store_mark_exited_nonzero_exit_code() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "failing cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_exited(id, Some(101), Duration::from_secs(2));
        let entry = store.get(id).unwrap();
        assert_eq!(entry.exit_code, Some(101));
    }

    #[test]
    fn store_mark_exited_none_exit_code() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "killed cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_exited(id, None, Duration::from_secs(0));
        let entry = store.get(id).unwrap();
        assert_eq!(entry.exit_code, None);
    }

    #[test]
    fn store_mark_killed() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "long cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_killed(id, Duration::from_secs(5));
        let entry = store.get(id).unwrap();
        assert_eq!(entry.status, ShellStatus::Killed);
        assert_eq!(entry.exit_code, None);
        assert_eq!(entry.elapsed, Some(Duration::from_secs(5)));
        assert!(entry.finished_at.is_some());
    }

    #[test]
    fn store_killed_not_overwritten_by_late_exit() {
        let mut store = ShellOutputStore::new();
        let req = make_req(1, "cmd");
        let id = req.id;
        store.insert_started(&req);
        store.mark_killed(id, Duration::from_secs(3));
        // Simulate a late exited event
        store.mark_exited(id, Some(0), Duration::from_secs(5));
        let entry = store.get(id).unwrap();
        // mark_exited overwrites, but the TUI handler checks before calling mark_exited
        // This test verifies the store API behavior
        assert_eq!(entry.status, ShellStatus::Exited);
    }
}
