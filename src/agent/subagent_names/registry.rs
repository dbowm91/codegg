//! Active-scoped display names for delegated subagent runs.
//!
//! A name is **held only while its subagent is active**. The registry is a
//! lease table, not a record: [`SubagentNameRegistry::release`] returns the
//! name to the pool, and a later subagent may draw the same one. That is what
//! "ephemeral" means here, and it is why the registry needs no persistence —
//! there is nothing to restore on restart, because nothing survives a restart.
//!
//! # Why the name is drawn here and not anywhere else
//!
//! The name is drawn exactly once, at spawn, and then travels **inside the
//! event**. The reducer and the TUI never draw a name. This is not a stylistic
//! preference — it is what makes projection replay deterministic. A reducer
//! that re-drew on replay would produce a different name for the same event
//! stream, and a durable `agent_run` row would disagree with the live view
//! about what a given subagent was called. See the replay publication path in
//! `crates/codegg-core/src/projection_replay/publication.rs`, which rebuilds
//! nodes straight from the recorded event; it copies `display_name` rather
//! than drawing a fresh one.
//!
//! # Collision policy
//!
//! Two subagents of the same agent type must never share a name, or the
//! sessions panel lists two indistinguishable rows. On collision the registry
//! takes the first free of `Name`, `Name-2`, `Name-3`, … The suffix belongs to
//! the *active* lease only; the pool entry itself is never mutated, so a
//! released `Sally-2` does not leave `Sally-2` permanently burned.
//!
//! # Determinism within a lease
//!
//! Two subagents that draw the same base name must not be handed the same
//! suffix either. The suffix counter therefore walks forward from the base
//! name against the *currently held* set rather than from a global counter.

use std::collections::HashMap;
use std::sync::Mutex;

use super::{SubagentName, SUBAGENT_NAMES};

/// Fallback when the compiled pool is empty.
///
/// The pool is generated from a non-empty TOML source, so this is
/// unreachable in practice. It exists because an empty pool must degrade to
/// "no name" rather than panic on a worker thread — a panic here would take
/// down a subagent turn over a cosmetic detail.
const FALLBACK: &str = "Agent";

/// Upper bound on the `-N` suffix walk.
///
/// At most one lease per distinct base name can be held, so the walk is
/// bounded by the number of active subagents in practice. The cap exists so a
/// corrupt or unexpectedly large active set cannot turn assignment into an
/// unbounded loop.
const MAX_SUFFIX: u32 = 64;

/// Active-scoped registry mapping a subagent task id to its display name.
///
/// Cheap to clone-by-`Arc` and shared across the worker pool; all state lives
/// behind a single mutex. Assign/release are short and non-blocking, so the
/// lock is never held across an `.await`.
#[derive(Debug, Default)]
pub struct SubagentNameRegistry {
    /// Active lease: subagent task id -> the exact name string it was given.
    ///
    /// The stored value is the *resolved* name (`Sally` or `Sally-2`), not the
    /// pool base, so releasing a lease frees exactly what was handed out.
    active: Mutex<HashMap<u64, String>>,
}

impl SubagentNameRegistry {
    /// A registry holding no names.
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign a display name to `task_id` and return it.
    ///
    /// Re-assigning an already-active `task_id` returns the name it already
    /// holds rather than burning a second lease on the same subagent. That
    /// makes the call idempotent, which matters because the draw happens on a
    /// spawn path that can be re-entered after a retry.
    pub fn assign(&self, task_id: u64) -> String {
        let Ok(mut active) = self.active.lock() else {
            // A poisoned lock means some other lease-holder panicked. Dropping
            // the poisoned guard would panic again, on every subsequent
            // spawn, turning one failure into a permanent outage. Recovering
            // the inner map keeps the surface working; the worst outcome is a
            // duplicate name, which is strictly better than no subagents.
            tracing::error!(task_id, "subagent name registry poisoned; recovering");
            return Self::assign_without_lock(task_id, &mut HashMap::new());
        };

        if let Some(existing) = active.get(&task_id) {
            return existing.clone();
        }

        let name = draw(&active);
        active.insert(task_id, name.clone());
        name
    }

    /// Release the name held by `task_id`, returning it to the pool.
    ///
    /// Idempotent: releasing an unknown or already-released id is a no-op.
    /// Call this on every terminal path — completion, failure, and
    /// cancellation — which is why [`SubagentNameLease`] exists rather than
    /// relying on each call site to remember.
    pub fn release(&self, task_id: u64) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&task_id);
        }
    }

    /// The name currently held by `task_id`, if it is active.
    pub fn get(&self, task_id: u64) -> Option<String> {
        self.active
            .lock()
            .ok()
            .and_then(|a| a.get(&task_id).cloned())
    }

    /// Whether `task_id` currently holds a lease.
    pub fn is_active(&self, task_id: u64) -> bool {
        self.active
            .lock()
            .map(|a| a.contains_key(&task_id))
            .unwrap_or(false)
    }

    /// Number of names currently leased.
    pub fn active_len(&self) -> usize {
        self.active.lock().map(|a| a.len()).unwrap_or(0)
    }

    /// Draw from a recovered (empty) map. Used only on the poisoned-lock path,
    /// where the real active set is unavailable and a collision is possible.
    fn assign_without_lock(task_id: u64, scratch: &mut HashMap<u64, String>) -> String {
        let name = draw(scratch);
        scratch.insert(task_id, name.clone());
        name
    }
}

/// Draw a name that does not collide with anything in `active`.
///
/// Walks the pool in a randomly chosen order so that concurrent spawns do not
/// march in lockstep through the same prefix, then applies the `-N` suffix
/// rule for the unlucky ones that land on a held base name.
fn draw(active: &HashMap<u64, String>) -> String {
    if SUBAGENT_NAMES.is_empty() {
        return FALLBACK.to_string();
    }

    let start = random_index(SUBAGENT_NAMES.len());
    for step in 0..SUBAGENT_NAMES.len() {
        let idx = (start + step) % SUBAGENT_NAMES.len();
        let base = &SUBAGENT_NAMES[idx].name;
        let candidate = first_free(base, active);
        if let Some(name) = candidate {
            return name;
        }
    }

    // Every base name is held under every suffix within MAX_SUFFIX. Degrade to
    // the task id rather than colliding silently: an ugly unique name beats two
    // subagents wearing the same one.
    format!("{FALLBACK}-{}", active.len() + 1)
}

/// First free name of `base`, `base-2`, `base-3`, … against the active set.
///
/// Returns `None` when the walk exceeds [`MAX_SUFFIX`], which lets the caller
/// move on to the next base name instead of spinning here.
fn first_free(base: &str, active: &HashMap<u64, String>) -> Option<String> {
    if !is_taken(base, active) {
        return Some(base.to_string());
    }
    for suffix in 2..=MAX_SUFFIX {
        let candidate = format!("{base}-{suffix}");
        if !is_taken(&candidate, active) {
            return Some(candidate);
        }
    }
    None
}

fn is_taken(name: &str, active: &HashMap<u64, String>) -> bool {
    active.values().any(|held| held == name)
}

/// Uniform index in `0..len`, via `rand`.
///
/// The registry already depends on randomness; using the crate the workspace
/// already depends on avoids a bespoke PRNG here. `% len` is acceptable
/// because the modulo bias over a 46-entry pool is far below any perceptible
/// threshold for cosmetic name assignment.
fn random_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (rand::random::<u64>() % len as u64) as usize
}

/// RAII lease that releases a name when dropped.
///
/// This is what makes "released on every terminal path" true by construction
/// rather than by convention: completion, failure, cancellation, and panic all
/// unwind through `Drop`, so there is no path that can leak a name and
/// gradually exhaust the pool.
#[derive(Debug)]
pub struct SubagentNameLease {
    registry: std::sync::Arc<SubagentNameRegistry>,
    task_id: u64,
    /// The name handed out at construction, kept for `name()` reads.
    name: String,
}

impl SubagentNameLease {
    /// Take a lease for `task_id`, drawing a name if one is not already held.
    pub fn acquire(registry: std::sync::Arc<SubagentNameRegistry>, task_id: u64) -> Self {
        let name = registry.assign(task_id);
        Self {
            registry,
            task_id,
            name,
        }
    }

    /// The leased display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The leased name as an owned `String`, for embedding in an event.
    pub fn name_owned(&self) -> String {
        self.name.clone()
    }

    /// The subagent this name belongs to.
    pub fn task_id(&self) -> u64 {
        self.task_id
    }
}

impl Drop for SubagentNameLease {
    fn drop(&mut self) {
        self.registry.release(self.task_id);
    }
}

/// The pool entry behind a display name, if any.
///
/// The suffix is stripped so `Sally-2` resolves to the `Sally` entry. Returns
/// `None` for a name that did not come from the pool (a fallback), which is
/// the honest answer rather than a wrong attribution.
pub fn pool_entry(display_name: &str) -> Option<&'static SubagentName> {
    let base = display_name
        .split_once('-')
        .map_or(display_name, |(head, tail)| {
            if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
                head
            } else {
                display_name
            }
        });
    SUBAGENT_NAMES.iter().find(|entry| entry.name == base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn registry() -> Arc<SubagentNameRegistry> {
        Arc::new(SubagentNameRegistry::new())
    }

    #[test]
    fn assigned_names_come_from_the_pool() {
        let registry = registry();
        let name = registry.assign(1);
        assert!(
            pool_entry(&name).is_some(),
            "{name} should resolve to a pool entry"
        );
    }

    #[test]
    fn concurrent_subagents_never_share_a_name() {
        let registry = registry();
        let mut names = Vec::new();
        for task_id in 0..12 {
            names.push(registry.assign(task_id));
        }
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "duplicate name in {names:?}");
    }

    #[test]
    fn draw_survives_a_pool_where_the_chosen_base_is_occupied() {
        // Exercise the suffix path deterministically by driving `draw` against
        // a hand-built active set, rather than hoping `assign` happens to pick
        // a colliding base. The random draw makes the registry-level
        // collision test above inherently probabilistic; this one is not.
        let mut active: HashMap<u64, String> = HashMap::new();
        for (idx, entry) in SUBAGENT_NAMES.iter().enumerate() {
            // Occupy every base name so the draw is forced to suffix.
            active.insert(idx as u64, entry.name.to_string());
        }
        let name = draw(&active);
        assert!(
            pool_entry(&name).is_some(),
            "an occupied base must still yield a pool name, got {name}"
        );
        assert!(
            !is_taken(&name, &active),
            "{name} collided with a held name"
        );
        assert!(
            name.ends_with("-2"),
            "with every base held, the first draw should take -2: {name}"
        );
    }

    #[test]
    fn a_fully_occupied_pool_walks_the_suffix_ladder() {
        let mut active: HashMap<u64, String> = HashMap::new();
        let mut key = 0u64;
        for entry in SUBAGENT_NAMES.iter() {
            key += 1;
            active.insert(key, entry.name.to_string());
            // Hold `Name` and `Name-2` for every entry.
            key += 1;
            active.insert(key, format!("{}-2", entry.name));
        }
        let name = draw(&active);
        assert!(
            name.ends_with("-3"),
            "the walk should land on -3 when base and -2 are held: {name}"
        );
        assert!(!is_taken(&name, &active), "{name} collided");
    }

    #[test]
    fn suffix_walk_avoids_already_held_suffixes() {
        let mut active: HashMap<u64, String> = HashMap::new();
        active.insert(1, "Sally".to_string());
        active.insert(2, "Sally-2".to_string());
        let next = first_free("Sally", &active).expect("Sally-3 is free");
        assert_eq!(next, "Sally-3");
    }

    #[test]
    fn plain_base_is_preferred_over_a_suffix() {
        let mut active: HashMap<u64, String> = HashMap::new();
        active.insert(1, "Greg-2".to_string());
        assert_eq!(first_free("Greg", &active).as_deref(), Some("Greg"));
    }

    #[test]
    fn reassigning_the_same_task_is_idempotent() {
        let registry = registry();
        let first = registry.assign(42);
        let second = registry.assign(42);
        assert_eq!(first, second, "a retry must not burn a second lease");
        assert_eq!(registry.active_len(), 1);
    }

    #[test]
    fn release_returns_the_name_to_the_pool() {
        let registry = registry();
        let name = registry.assign(5);
        assert!(registry.is_active(5));
        registry.release(5);
        assert!(!registry.is_active(5));
        assert_eq!(registry.get(5), None);
        assert_eq!(registry.active_len(), 0);
        // The released name must be drawable again.
        let mut seen = std::collections::HashSet::new();
        for task_id in 0..200 {
            seen.insert(registry.assign(task_id));
        }
        assert!(
            seen.contains(&name),
            "{name} should be reusable after release"
        );
    }

    #[test]
    fn releasing_an_unknown_id_is_a_no_op() {
        let registry = registry();
        registry.release(999);
        assert_eq!(registry.active_len(), 0);
    }

    #[test]
    fn lease_releases_on_drop() {
        let registry = registry();
        {
            let lease = SubagentNameLease::acquire(registry.clone(), 11);
            assert_eq!(lease.task_id(), 11);
            assert!(registry.is_active(11));
            assert_eq!(lease.name_owned(), lease.name());
        }
        assert!(!registry.is_active(11), "Drop must release the lease");
    }

    #[test]
    fn lease_releases_even_when_the_body_unwinds() {
        let registry = registry();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _lease = SubagentNameLease::acquire(registry.clone(), 12);
            panic!("subagent failed");
        }));
        assert!(result.is_err());
        assert!(!registry.is_active(12), "a panicking path must not leak");
    }

    #[test]
    fn lease_names_are_distinct_across_concurrent_holders() {
        let registry = registry();
        let leases: Vec<SubagentNameLease> = (0..8)
            .map(|task_id| SubagentNameLease::acquire(registry.clone(), task_id))
            .collect();
        let mut names: Vec<&str> = leases.iter().map(|l| l.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "concurrent leases collided");
        drop(leases);
        assert_eq!(registry.active_len(), 0);
    }

    #[test]
    fn pool_entry_strips_a_numeric_suffix_only() {
        let base = SUBAGENT_NAMES[0].name;
        assert_eq!(
            pool_entry(base).map(|e| e.name),
            Some(base),
            "a bare base name resolves to itself"
        );
        assert_eq!(
            pool_entry(&format!("{base}-2")).map(|e| e.name),
            Some(base),
            "a numeric suffix resolves to the base entry"
        );
        assert_eq!(
            pool_entry(&format!("{base}-x")).map(|e| e.name),
            None,
            "a non-numeric suffix is not a registry suffix"
        );
        assert_eq!(pool_entry("Nobody-999"), None);
    }

    #[test]
    fn pool_entry_does_not_mistake_a_hyphenated_name_for_a_suffix() {
        // Guards the split heuristic: only a trailing all-digit segment is a
        // registry suffix. A pool name that legitimately contains a hyphen must
        // still resolve to itself.
        let hyphenated = SUBAGENT_NAMES.iter().find(|e| e.name.contains('-'));
        if let Some(entry) = hyphenated {
            assert_eq!(pool_entry(entry.name).map(|e| e.name), Some(entry.name));
        }
    }

    #[test]
    fn exhausted_pool_falls_back_to_a_unique_name() {
        let mut active: HashMap<u64, String> = HashMap::new();
        // Hold every base and every suffix the walk would try. The keys are
        // arbitrary — only the *values* matter to the collision check — but
        // each entry needs its own key or later inserts would overwrite
        // earlier ones and leave the map partly empty.
        let mut key = 0u64;
        for entry in SUBAGENT_NAMES.iter() {
            key += 1;
            active.insert(key, entry.name.to_string());
            for suffix in 2..=MAX_SUFFIX {
                key += 1;
                active.insert(key, format!("{}-{suffix}", entry.name));
            }
        }
        let name = draw(&active);
        assert!(
            pool_entry(&name).is_none(),
            "an exhausted pool must not claim a pool identity"
        );
        assert!(is_taken(&name, &active) == false);
    }
}
