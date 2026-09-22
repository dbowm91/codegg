//! Candidate-order diagnostics and deterministic permutation contract.
//!
//! M001 of the order-invariance experiment. This module makes the
//! historical candidate-position shortcut measurable and gives later
//! architecture/training milestones one deterministic permutation contract.
//!
//! It never trains a model, never changes historical corpora, and never
//! selects hyperparameters. Training views produced here are in-memory or
//! generated-local views: source `case_id`/semantic/leakage ownership stays
//! traceable and no augmented view moves into dev/test or gains a new
//! leakage group.

use super::{dataset_fingerprint, partition_cases, ToolAdvisor, ToolAdvisorCase};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Version of the permutation contract consumed by M002-M005.
pub const PERMUTATION_CONTRACT_VERSION: u16 = 1;
/// Default permutation budget where factorial/candidate count permits.
pub const DEFAULT_PERMUTATIONS: usize = 20;
/// Seed domain for deterministic training-view augmentation.
pub const TRAIN_VIEW_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// Seed domain for the dev-only permutation suite.
pub const DEV_SUITE_SEED: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// Index of the highest-relevance candidate in presentation order.
///
/// Ties on the maximum grade resolve to the first matching candidate in
/// presentation order. `None` for no-tool cases or cases without labels.
pub fn target_index(case: &ToolAdvisorCase) -> Option<usize> {
    if case.none || case.relevance.is_empty() {
        return None;
    }
    let max_grade = case.relevance.values().copied().max()?;
    case.candidates
        .iter()
        .position(|candidate| case.relevance.get(&candidate.name).copied() == Some(max_grade))
}

/// All candidate names sharing the maximum relevance grade, in
/// presentation order. Empty for no-tool cases.
pub fn top_relevant_names(case: &ToolAdvisorCase) -> Vec<String> {
    if case.none || case.relevance.is_empty() {
        return Vec::new();
    }
    let Some(max_grade) = case.relevance.values().copied().max() else {
        return Vec::new();
    };
    case.candidates
        .iter()
        .filter(|candidate| case.relevance.get(&candidate.name).copied() == Some(max_grade))
        .map(|candidate| candidate.name.clone())
        .collect()
}

/// Histogram of highest-relevance target positions over non-no-tool cases.
pub fn target_position_histogram(cases: &[ToolAdvisorCase]) -> BTreeMap<usize, usize> {
    let mut histogram = BTreeMap::new();
    for case in cases {
        if let Some(index) = target_index(case) {
            *histogram.entry(index).or_insert(0) += 1;
        }
    }
    histogram
}

/// Histogram of candidate counts over all cases.
pub fn candidate_count_histogram(cases: &[ToolAdvisorCase]) -> BTreeMap<usize, usize> {
    let mut histogram = BTreeMap::new();
    for case in cases {
        *histogram.entry(case.candidates.len()).or_insert(0) += 1;
    }
    histogram
}

/// Candidate-count distribution restricted to no-tool cases.
///
/// No-tool cases have no target position, so "no-tool order distribution"
/// is reported as counts by candidate-set size.
pub fn no_tool_count_histogram(cases: &[ToolAdvisorCase]) -> BTreeMap<usize, usize> {
    let mut histogram = BTreeMap::new();
    for case in cases {
        if case.none {
            *histogram.entry(case.candidates.len()).or_insert(0) += 1;
        }
    }
    histogram
}

/// Position of the first top-relevant candidate for multi-tool cases.
pub fn multi_tool_top_position_histogram(cases: &[ToolAdvisorCase]) -> BTreeMap<usize, usize> {
    let mut histogram = BTreeMap::new();
    for case in cases {
        if case.none || case.relevance.len() < 2 {
            continue;
        }
        if let Some(index) = target_index(case) {
            *histogram.entry(index).or_insert(0) += 1;
        }
    }
    histogram
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic 64-bit seed mixer over a domain seed plus opaque parts.
pub fn mix_seed(domain: u64, parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_4842_2235 ^ domain;
    for part in parts {
        for byte in part.iter() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01B3);
        }
        hash ^= 0xFF;
        hash = hash.wrapping_mul(0x0100_0000_01B3);
    }
    hash
}

fn factorial_capped(n: usize, cap: usize) -> usize {
    let mut acc = 1usize;
    for i in 2..=n {
        acc = acc.saturating_mul(i);
        if acc > cap {
            return cap + 1;
        }
    }
    acc
}

fn shuffle_with_state(order: &mut [usize], state: &mut u64) {
    for i in (1..order.len()).rev() {
        let j = (splitmix64(state) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
}

/// Generate deterministic candidate permutations for a set of `n`.
///
/// Returns up to `max_permutations` unique permutations of `0..n`.
/// When `n!` fits inside the bound every unique permutation is
/// enumerated; otherwise the identity plus distinct deterministic
/// Fisher-Yates shuffles are returned. The identity is always first so
/// canonical-order scoring stays comparable.
pub fn generate_permutations(
    candidate_count: usize,
    seed: u64,
    max_permutations: usize,
) -> Vec<Vec<usize>> {
    let bound = max_permutations.max(1);
    if candidate_count <= 1 {
        return vec![(0..candidate_count).collect()];
    }
    if factorial_capped(candidate_count, bound) <= bound {
        let mut current: Vec<usize> = (0..candidate_count).collect();
        let mut out = vec![current.clone()];
        while next_permutation(&mut current) {
            out.push(current.clone());
            if out.len() >= bound {
                break;
            }
        }
        return out;
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let identity: Vec<usize> = (0..candidate_count).collect();
    seen.insert(identity.clone());
    out.push(identity);
    let mut state = seed;
    let mut attempts = 0usize;
    while out.len() < bound && attempts < bound.saturating_mul(40).max(64) {
        attempts += 1;
        let mut order: Vec<usize> = (0..candidate_count).collect();
        shuffle_with_state(&mut order, &mut state);
        if seen.insert(order.clone()) {
            out.push(order);
        }
    }
    out
}

fn next_permutation(order: &mut [usize]) -> bool {
    if order.len() < 2 {
        return false;
    }
    let mut i = order.len() - 2;
    loop {
        if order[i] < order[i + 1] {
            break;
        }
        if i == 0 {
            order.reverse();
            return false;
        }
        i -= 1;
    }
    let mut j = order.len() - 1;
    while order[j] <= order[i] {
        j -= 1;
    }
    order.swap(i, j);
    order[i + 1..].reverse();
    true
}

fn check_permutation(permutation: &[usize], candidate_count: usize) -> Result<()> {
    if permutation.len() != candidate_count {
        return Err(anyhow!(
            "permutation length {} does not match candidate count {candidate_count}",
            permutation.len()
        ));
    }
    let mut seen = vec![false; candidate_count];
    for index in permutation {
        if *index >= candidate_count || seen[*index] {
            return Err(anyhow!("invalid candidate permutation"));
        }
        seen[*index] = true;
    }
    Ok(())
}

/// Reorder a case's candidates by `permutation`.
///
/// `permutation[i]` is the source index placed at position `i`.
/// Relevance labels stay keyed by candidate name, so label values,
/// none/no-tool status, semantic/leakage groups, and split assignment
/// are unchanged. The returned case keeps the source `case_id` so
/// mapped-back scoring compares by candidate identity.
pub fn permute_case(case: &ToolAdvisorCase, permutation: &[usize]) -> Result<ToolAdvisorCase> {
    check_permutation(permutation, case.candidates.len())?;
    let mut permuted = case.clone();
    permuted.candidates = permutation
        .iter()
        .map(|index| case.candidates[*index].clone())
        .collect();
    permuted.validate()?;
    Ok(permuted)
}

/// Invert [`permute_case`]: restore canonical candidate order.
pub fn unpermute_case(case: &ToolAdvisorCase, permutation: &[usize]) -> Result<ToolAdvisorCase> {
    check_permutation(permutation, case.candidates.len())?;
    let mut inverse = vec![0usize; permutation.len()];
    for (position, source) in permutation.iter().enumerate() {
        inverse[*source] = position;
    }
    permute_case(case, &inverse)
}

/// Training-view permutation placing source index `target_source` at
/// presentation position `target_position`, with remaining candidates in
/// deterministic seeded order.
pub fn placing_permutation(
    candidate_count: usize,
    target_source: usize,
    target_position: usize,
    seed: u64,
) -> Result<Vec<usize>> {
    if target_source >= candidate_count || target_position >= candidate_count {
        return Err(anyhow!("target placement is out of bounds"));
    }
    let mut rest: Vec<usize> = (0..candidate_count)
        .filter(|i| *i != target_source)
        .collect();
    let mut state = seed;
    shuffle_with_state(&mut rest, &mut state);
    let mut permutation = Vec::with_capacity(candidate_count);
    let mut rest_iter = rest.into_iter();
    for position in 0..candidate_count {
        if position == target_position {
            permutation.push(target_source);
        } else if let Some(next) = rest_iter.next() {
            permutation.push(next);
        }
    }
    check_permutation(&permutation, candidate_count)?;
    Ok(permutation)
}

fn reciprocal_rank(case: &ToolAdvisorCase, ranked_names: &[String]) -> f64 {
    let relevant: BTreeSet<&str> = case.relevance.keys().map(String::as_str).collect();
    ranked_names
        .iter()
        .position(|name| relevant.contains(name.as_str()))
        .map_or(0.0, |index| 1.0 / (index as f64 + 1.0))
}

fn recall_at_1(case: &ToolAdvisorCase, ranked_names: &[String]) -> f64 {
    let relevant: BTreeSet<&str> = case.relevance.keys().map(String::as_str).collect();
    if case.none {
        return 0.0;
    }
    ranked_names
        .first()
        .is_some_and(|name| relevant.contains(name.as_str())) as u8 as f64
}

/// Kendall tau between two full rank orders over the same names.
pub fn kendall_tau(first: &[String], second: &[String]) -> Option<f64> {
    if first.len() != second.len() || first.len() < 2 {
        return None;
    }
    let position: BTreeMap<&str, usize> = second
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    if position.len() != second.len() {
        return None;
    }
    let mut concordant = 0u64;
    let mut discordant = 0u64;
    for i in 0..first.len() {
        for j in (i + 1)..first.len() {
            let left = position.get(first[i].as_str())?;
            let right = position.get(first[j].as_str())?;
            if left < right {
                concordant += 1;
            } else if left > right {
                discordant += 1;
            }
        }
    }
    let total = concordant + discordant;
    if total == 0 {
        return None;
    }
    Some((concordant as f64 - discordant as f64) / total as f64)
}

/// Spearman rho between two full rank orders over the same names.
pub fn spearman_rho(first: &[String], second: &[String]) -> Option<f64> {
    if first.len() != second.len() || first.len() < 2 {
        return None;
    }
    let position: BTreeMap<&str, usize> = second
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    if position.len() != second.len() {
        return None;
    }
    let n = first.len() as f64;
    let mut sum_squared = 0.0;
    for (index, name) in first.iter().enumerate() {
        let other = position.get(name.as_str())?;
        let delta = index as f64 - *other as f64;
        sum_squared += delta * delta;
    }
    Some(1.0 - 6.0 * sum_squared / (n * (n * n - 1.0)))
}

/// Per-case permutation robustness evidence, always mapped back to
/// canonical candidate identity rather than presentation index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermutationMetrics {
    pub case_id: String,
    pub candidate_count: usize,
    pub permutations: usize,
    pub top1_identity_consistency: f64,
    pub modal_top1: String,
    pub top3_set_consistency: f64,
    pub mean_score_drift: f64,
    pub max_score_drift: f64,
    pub mean_kendall_tau: Option<f64>,
    pub mean_spearman_rho: Option<f64>,
    pub mrr_min: f64,
    pub mrr_max: f64,
    pub recall_at_1_min: f64,
    pub recall_at_1_max: f64,
    pub worst_case_regret: f64,
    pub true_target_position: Option<usize>,
}

/// Score one case under every deterministic permutation and compare by
/// candidate identity.
pub fn evaluate_permutation_robustness(
    case: &ToolAdvisorCase,
    advisor: &dyn ToolAdvisor,
    seed: u64,
    max_permutations: usize,
) -> Result<PermutationMetrics> {
    let permutations = generate_permutations(case.candidates.len(), seed, max_permutations.max(1));
    let canonical_names: Vec<String> = case
        .candidates
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    // Score each permuted presentation, then map scores back by name.
    let mut per_perm_scores: Vec<BTreeMap<String, f64>> = Vec::new();
    let mut per_perm_order: Vec<Vec<String>> = Vec::new();
    for permutation in &permutations {
        let presented = permute_case(case, permutation)?;
        let input = super::ToolAdvisorInput {
            case_id: presented.case_id.clone(),
            context: presented.context.clone(),
            candidates: presented.candidates.clone(),
            surface_fingerprint: String::new(),
        };
        let prediction = advisor.score(&input)?;
        let mut scores = BTreeMap::new();
        for item in &prediction.ranked {
            scores.insert(item.name.clone(), item.score);
        }
        // Unscored candidates keep a below-minimum score so rank orders
        // stay total without inventing a relevance claim.
        let floor = scores.values().copied().fold(f64::INFINITY, f64::min);
        let floor = if floor.is_finite() { floor - 1.0 } else { 0.0 };
        for name in &canonical_names {
            scores.entry(name.clone()).or_insert(floor);
        }
        let mut order = canonical_names.clone();
        order.sort_by(|left, right| {
            scores[left]
                .partial_cmp(&scores[right])
                .unwrap_or(std::cmp::Ordering::Equal)
                .reverse()
                .then_with(|| left.cmp(right))
        });
        per_perm_scores.push(scores);
        per_perm_order.push(order);
    }
    let top1: Vec<String> = per_perm_order
        .iter()
        .map(|order| order.first().cloned().unwrap_or_default())
        .collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in &top1 {
        *counts.entry(name.as_str()).or_insert(0) += 1;
    }
    let modal_top1 = counts
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| left.0.cmp(right.0)))
        .map(|(name, _)| (*name).to_string())
        .unwrap_or_default();
    let modal_count = counts.get(modal_top1.as_str()).copied().unwrap_or(0);
    let top1_identity_consistency = if top1.is_empty() {
        1.0
    } else {
        modal_count as f64 / top1.len() as f64
    };
    // Top-3 set consistency against the modal top-3 set.
    let top3_sets: Vec<BTreeSet<String>> = per_perm_order
        .iter()
        .map(|order| order.iter().take(3.min(order.len())).cloned().collect())
        .collect();
    let mut set_counts: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    for set in &top3_sets {
        let mut key: Vec<String> = set.iter().cloned().collect();
        key.sort();
        *set_counts.entry(key).or_insert(0) += 1;
    }
    let modal_set_count = set_counts.values().copied().max().unwrap_or(0);
    let top3_set_consistency = if top3_sets.is_empty() {
        1.0
    } else {
        modal_set_count as f64 / top3_sets.len() as f64
    };
    // Per-candidate score drift across permutations.
    let mut drifts = Vec::new();
    for name in &canonical_names {
        let values: Vec<f64> = per_perm_scores
            .iter()
            .map(|scores| scores[name.as_str()])
            .collect();
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        drifts.push((max - min).max(0.0));
    }
    let mean_score_drift = if drifts.is_empty() {
        0.0
    } else {
        drifts.iter().sum::<f64>() / drifts.len() as f64
    };
    let max_score_drift = drifts.iter().copied().fold(0.0, f64::max);
    // Rank agreement of each permuted mapped-back order versus the
    // canonical-presentation mapped-back order.
    let canonical_order = per_perm_order.first().cloned().unwrap_or_default();
    let mut taus = Vec::new();
    let mut rhos = Vec::new();
    for order in &per_perm_order {
        if let Some(tau) = kendall_tau(&canonical_order, order) {
            taus.push(tau);
        }
        if let Some(rho) = spearman_rho(&canonical_order, order) {
            rhos.push(rho);
        }
    }
    let mean_kendall_tau = if taus.is_empty() {
        None
    } else {
        Some(taus.iter().sum::<f64>() / taus.len() as f64)
    };
    let mean_spearman_rho = if rhos.is_empty() {
        None
    } else {
        Some(rhos.iter().sum::<f64>() / rhos.len() as f64)
    };
    let mut rrs = Vec::new();
    let mut r1s = Vec::new();
    for order in &per_perm_order {
        rrs.push(reciprocal_rank(case, order));
        r1s.push(recall_at_1(case, order));
    }
    let mrr_min = rrs.iter().copied().fold(f64::INFINITY, f64::min);
    let mrr_max = rrs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let recall_at_1_min = r1s.iter().copied().fold(f64::INFINITY, f64::min);
    let recall_at_1_max = r1s.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let worst_case_regret = (mrr_max - mrr_min).max(0.0);
    Ok(PermutationMetrics {
        case_id: case.case_id.clone(),
        candidate_count: case.candidates.len(),
        permutations: permutations.len(),
        top1_identity_consistency,
        modal_top1,
        top3_set_consistency,
        mean_score_drift,
        max_score_drift,
        mean_kendall_tau,
        mean_spearman_rho,
        mrr_min: if mrr_min.is_finite() { mrr_min } else { 0.0 },
        mrr_max: if mrr_max.is_finite() { mrr_max } else { 0.0 },
        recall_at_1_min: if recall_at_1_min.is_finite() {
            recall_at_1_min
        } else {
            0.0
        },
        recall_at_1_max: if recall_at_1_max.is_finite() {
            recall_at_1_max
        } else {
            0.0
        },
        worst_case_regret,
        true_target_position: target_index(case),
    })
}

/// Aggregate permutation evidence grouped by true target position.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetPositionBucket {
    pub target_position: usize,
    pub cases: usize,
    pub mean_mrr_worst: f64,
    pub mean_recall_at_1_worst: f64,
    pub mean_top1_consistency: f64,
}

/// Suite-level permutation summary over many cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuitePermutationSummary {
    pub cases: usize,
    pub mean_top1_consistency: f64,
    pub mean_max_score_drift: f64,
    pub worst_mrr: f64,
    pub best_mrr: f64,
    pub worst_recall_at_1: f64,
    pub best_recall_at_1: f64,
    pub max_regret: f64,
    pub by_target_position: Vec<TargetPositionBucket>,
}

/// Aggregate per-case metrics into a suite summary.
pub fn summarize_suite(metrics: &[PermutationMetrics]) -> SuitePermutationSummary {
    if metrics.is_empty() {
        return SuitePermutationSummary {
            cases: 0,
            mean_top1_consistency: 1.0,
            mean_max_score_drift: 0.0,
            worst_mrr: 0.0,
            best_mrr: 0.0,
            worst_recall_at_1: 0.0,
            best_recall_at_1: 0.0,
            max_regret: 0.0,
            by_target_position: Vec::new(),
        };
    }
    let mean_top1_consistency = metrics
        .iter()
        .map(|m| m.top1_identity_consistency)
        .sum::<f64>()
        / metrics.len() as f64;
    let mean_max_score_drift =
        metrics.iter().map(|m| m.max_score_drift).sum::<f64>() / metrics.len() as f64;
    let worst_mrr = metrics
        .iter()
        .map(|m| m.mrr_min)
        .fold(f64::INFINITY, f64::min);
    let best_mrr = metrics
        .iter()
        .map(|m| m.mrr_max)
        .fold(f64::NEG_INFINITY, f64::max);
    let worst_recall_at_1 = metrics
        .iter()
        .map(|m| m.recall_at_1_min)
        .fold(f64::INFINITY, f64::min);
    let best_recall_at_1 = metrics
        .iter()
        .map(|m| m.recall_at_1_max)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_regret = metrics
        .iter()
        .map(|m| m.worst_case_regret)
        .fold(0.0, f64::max);
    let mut buckets: BTreeMap<usize, Vec<&PermutationMetrics>> = BTreeMap::new();
    for metric in metrics {
        if let Some(position) = metric.true_target_position {
            buckets.entry(position).or_default().push(metric);
        }
    }
    let by_target_position = buckets
        .into_iter()
        .map(|(target_position, members)| {
            let cases = members.len();
            TargetPositionBucket {
                target_position,
                cases,
                mean_mrr_worst: members.iter().map(|m| m.mrr_min).sum::<f64>() / cases as f64,
                mean_recall_at_1_worst: members.iter().map(|m| m.recall_at_1_min).sum::<f64>()
                    / cases as f64,
                mean_top1_consistency: members
                    .iter()
                    .map(|m| m.top1_identity_consistency)
                    .sum::<f64>()
                    / cases as f64,
            }
        })
        .collect();
    SuitePermutationSummary {
        cases: metrics.len(),
        mean_top1_consistency,
        mean_max_score_drift,
        worst_mrr,
        best_mrr,
        worst_recall_at_1,
        best_recall_at_1,
        max_regret,
        by_target_position,
    }
}

/// One deterministic augmented training view with provenance.
#[derive(Debug, Clone)]
pub struct TrainingView {
    /// Augmented case. `case_id` is `{source}::oinv-{position:02}-{slot:02}`;
    /// semantic/leakage groups are unchanged from the source.
    pub case: ToolAdvisorCase,
    pub source_case_id: String,
    pub placed_position: Option<usize>,
}

/// Build the deterministic balanced training view for clean train cases.
///
/// For every non-no-tool source case one view is produced per valid
/// candidate position, with a top-relevant candidate placed at that
/// position (round-robin over equally top-relevant candidates). The
/// aggregate therefore places the highest-relevance candidate uniformly
/// across positions. No-tool cases receive two deterministic shuffles so
/// order is never a no-tool cue. Context, descriptors, labels,
/// none/no-tool status, semantic/leakage groups, and split assignment
/// are unchanged; only candidate presentation order and the derived
/// `case_id` suffix differ.
pub fn balanced_training_views(
    cases: &[ToolAdvisorCase],
    train_indices: &[usize],
    seed: u64,
) -> Result<Vec<TrainingView>> {
    let mut views = Vec::new();
    for index in train_indices {
        let case = cases
            .get(*index)
            .ok_or_else(|| anyhow!("train index out of bounds"))?;
        let n = case.candidates.len();
        if n == 0 {
            return Err(anyhow!("case {} has no candidates", case.case_id));
        }
        if case.none {
            for slot in 0..2.min(n * 2).max(1) {
                let state_seed = mix_seed(
                    seed,
                    &[case.case_id.as_bytes(), b"no-tool", &slot.to_le_bytes()],
                );
                let mut state = state_seed;
                let mut order: Vec<usize> = (0..n).collect();
                shuffle_with_state(&mut order, &mut state);
                let mut view = permute_case(case, &order)?;
                view.case_id = format!("{}::oinv-nt-{slot:02}", case.case_id);
                view.validate()?;
                views.push(TrainingView {
                    case: view,
                    source_case_id: case.case_id.clone(),
                    placed_position: None,
                });
            }
            continue;
        }
        let top_names = top_relevant_names(case);
        if top_names.is_empty() {
            return Err(anyhow!(
                "labeled case {} has no top candidate",
                case.case_id
            ));
        }
        let name_to_source: BTreeMap<&str, usize> = case
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.name.as_str(), index))
            .collect();
        for position in 0..n {
            let chosen = &top_names[position % top_names.len()];
            let Some(source) = name_to_source.get(chosen.as_str()).copied() else {
                return Err(anyhow!(
                    "top candidate is missing from case {}",
                    case.case_id
                ));
            };
            let place_seed = mix_seed(
                seed,
                &[
                    case.case_id.as_bytes(),
                    b"place",
                    &position.to_le_bytes(),
                    chosen.as_bytes(),
                ],
            );
            let permutation = placing_permutation(n, source, position, place_seed)?;
            let mut view = permute_case(case, &permutation)?;
            view.case_id = format!("{}::oinv-{position:02}", case.case_id);
            view.validate()?;
            debug_assert_eq!(view.candidates[position].name, *chosen);
            views.push(TrainingView {
                case: view,
                source_case_id: case.case_id.clone(),
                placed_position: Some(position),
            });
        }
    }
    Ok(views)
}

/// Maximum absolute deviation (percentage points, 0-100) of the
/// augmented target-position distribution from uniform.
///
/// Uniformity is measured within each candidate-count stratum: every
/// source case contributes exactly one placed view per valid position
/// `0..n-1`, so each stratum is exactly uniform by construction. The
/// reported value is the maximum stratum deviation, which also bounds
/// the conditional frequency of any valid position. Cases with
/// different candidate counts have different valid position sets, so a
/// pooled histogram over `0..max_n` would conflate strata and is not
/// used. No-tool views carry no placed target and are excluded.
pub fn augmented_position_deviation_pp(views: &[TrainingView]) -> f64 {
    let mut strata: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for view in views {
        if let Some(position) = view.placed_position {
            strata
                .entry(view.case.candidates.len())
                .or_default()
                .push(position);
        }
    }
    let mut worst = 0.0f64;
    for (width, placed) in &strata {
        if placed.is_empty() || *width == 0 {
            continue;
        }
        let mut counts = vec![0usize; *width];
        for position in placed {
            if *position < *width {
                counts[*position] += 1;
            }
        }
        let total = placed.len() as f64;
        let uniform = 100.0 / *width as f64;
        for count in counts {
            worst = worst.max((count as f64 / total * 100.0 - uniform).abs());
        }
    }
    worst
}

/// Dev-only permutation suite: evaluation only, never optimizer input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevPermutationSuite {
    pub contract_version: u16,
    pub seed: u64,
    pub permutations_per_case: usize,
    pub case_ids: Vec<String>,
    pub suite_fingerprint: String,
}

/// Freeze the dev permutation suite over clean dev cases.
pub fn dev_permutation_suite(
    cases: &[ToolAdvisorCase],
    dev_indices: &[usize],
    seed: u64,
    permutations_per_case: usize,
) -> Result<DevPermutationSuite> {
    let dev_cases: Vec<ToolAdvisorCase> = dev_indices
        .iter()
        .map(|index| {
            cases
                .get(*index)
                .cloned()
                .ok_or_else(|| anyhow!("dev index out of bounds"))
        })
        .collect::<Result<_>>()?;
    let dataset = dataset_fingerprint(&dev_cases)?;
    let suite_fingerprint = hex::encode(
        sha2::Sha256::digest(
            format!(
                "order-invariance-dev-suite-v{PERMUTATION_CONTRACT_VERSION}:{seed}:{permutations_per_case}:{dataset}"
            )
            .as_bytes(),
        ),
    );
    Ok(DevPermutationSuite {
        contract_version: PERMUTATION_CONTRACT_VERSION,
        seed,
        permutations_per_case,
        case_ids: dev_cases.iter().map(|case| case.case_id.clone()).collect(),
        suite_fingerprint,
    })
}

/// Machine-readable M001 order-bias report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDiagnosticsReport {
    pub schema_version: u16,
    pub contract_version: u16,
    pub source_dataset: String,
    pub source_fingerprint: String,
    pub total_cases: usize,
    pub non_no_tool_cases: usize,
    pub target_index_histogram: BTreeMap<usize, usize>,
    pub candidate_count_histogram: BTreeMap<usize, usize>,
    pub no_tool_count_histogram: BTreeMap<usize, usize>,
    pub multi_tool_top_position_histogram: BTreeMap<usize, usize>,
    pub train_cases: usize,
    pub dev_cases: usize,
    pub test_cases: usize,
    pub train_fingerprint: String,
    pub dev_fingerprint: String,
    pub test_fingerprint: String,
    pub train_target_histogram: BTreeMap<usize, usize>,
    pub dev_target_histogram: BTreeMap<usize, usize>,
    pub test_target_histogram: BTreeMap<usize, usize>,
    pub train_view_seed: u64,
    pub augmented_train_views: usize,
    pub augmented_position_deviation_pp: f64,
    pub dev_suite_seed: u64,
    pub dev_suite_fingerprint: String,
    pub dev_suite_cases: usize,
}

/// Build the full M001 diagnostics report from repository assets.
///
/// `source_dataset` labels the corpus (e.g. the asset path) for the
/// report; fingerprints always derive from case contents.
pub fn build_diagnostics_report(
    cases: &[ToolAdvisorCase],
    source_dataset: &str,
) -> Result<OrderDiagnosticsReport> {
    let partition = partition_cases(cases);
    let select = |indices: &[usize]| {
        indices
            .iter()
            .map(|index| cases[*index].clone())
            .collect::<Vec<_>>()
    };
    let train_cases = select(&partition.train_cases);
    let dev_cases = select(&partition.dev_cases);
    let test_cases = select(&partition.test_cases);
    let views = balanced_training_views(cases, &partition.train_cases, TRAIN_VIEW_SEED)?;
    let suite = dev_permutation_suite(
        cases,
        &partition.dev_cases,
        DEV_SUITE_SEED,
        DEFAULT_PERMUTATIONS,
    )?;
    Ok(OrderDiagnosticsReport {
        schema_version: 1,
        contract_version: PERMUTATION_CONTRACT_VERSION,
        source_dataset: source_dataset.to_string(),
        source_fingerprint: dataset_fingerprint(cases)?,
        total_cases: cases.len(),
        non_no_tool_cases: cases.iter().filter(|case| !case.none).count(),
        target_index_histogram: target_position_histogram(cases),
        candidate_count_histogram: candidate_count_histogram(cases),
        no_tool_count_histogram: no_tool_count_histogram(cases),
        multi_tool_top_position_histogram: multi_tool_top_position_histogram(cases),
        train_cases: train_cases.len(),
        dev_cases: dev_cases.len(),
        test_cases: test_cases.len(),
        train_fingerprint: dataset_fingerprint(&train_cases)?,
        dev_fingerprint: dataset_fingerprint(&dev_cases)?,
        test_fingerprint: dataset_fingerprint(&test_cases)?,
        train_target_histogram: target_position_histogram(&train_cases),
        dev_target_histogram: target_position_histogram(&dev_cases),
        test_target_histogram: target_position_histogram(&test_cases),
        train_view_seed: TRAIN_VIEW_SEED,
        augmented_train_views: views.len(),
        augmented_position_deviation_pp: augmented_position_deviation_pp(&views),
        dev_suite_seed: suite.seed,
        dev_suite_fingerprint: suite.suite_fingerprint,
        dev_suite_cases: suite.case_ids.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{load_cases, partition_cases, ToolAdvisorCandidate};
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn sample_case() -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: super::super::CASE_SCHEMA_VERSION,
            case_id: "sample".into(),
            context: "read a file from disk".into(),
            candidates: vec![
                ToolAdvisorCandidate {
                    name: "read".into(),
                    description: "Read files".into(),
                    category: "fs".into(),
                    disclosure: "core".into(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "write".into(),
                    description: "Write files".into(),
                    category: "fs".into(),
                    disclosure: "core".into(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "shell".into(),
                    description: "Run shell".into(),
                    category: "exec".into(),
                    disclosure: "deferred".into(),
                    synthetic_identity: false,
                },
            ],
            relevance: BTreeMap::from([("read".to_string(), 3)]),
            preferred_order: vec!["read".into()],
            none: false,
            tags: Vec::new(),
            group_id: "group-sample".into(),
            provenance: "test".into(),
            semantic_group: "semantic-sample".into(),
            leakage_group: String::new(),
            task_family: "filesystem".into(),
            tool_family: "filesystem".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    fn no_tool_case() -> ToolAdvisorCase {
        let mut case = sample_case();
        case.case_id = "no-tool-sample".into();
        case.relevance = BTreeMap::new();
        case.preferred_order = Vec::new();
        case.none = true;
        case.group_id = "group-no-tool".into();
        case.semantic_group = "semantic-no-tool".into();
        case
    }

    #[test]
    fn permutation_preserves_candidate_name_descriptor_pairs() {
        let case = sample_case();
        let permutation = vec![2, 0, 1];
        let permuted = permute_case(&case, &permutation).expect("permute");
        assert_eq!(permuted.candidates.len(), 3);
        for (position, source) in permutation.iter().enumerate() {
            assert_eq!(
                permuted.candidates[position].name, case.candidates[*source].name,
                "name must travel with its descriptor"
            );
            assert_eq!(
                permuted.candidates[position].description, case.candidates[*source].description,
                "descriptor must travel with its name"
            );
        }
    }

    #[test]
    fn labels_survive_round_trip_permutation() {
        let case = sample_case();
        let permutation = vec![2, 0, 1];
        let permuted = permute_case(&case, &permutation).expect("permute");
        assert_eq!(permuted.relevance, case.relevance);
        assert_eq!(permuted.preferred_order, case.preferred_order);
        let restored = unpermute_case(&permuted, &permutation).expect("unpermute");
        assert_eq!(restored.candidates, case.candidates);
        assert_eq!(restored.relevance, case.relevance);
        assert_eq!(restored.case_id, case.case_id);
    }

    #[test]
    fn no_tool_cases_remain_no_tool_under_all_permutations() {
        let case = no_tool_case();
        for permutation in generate_permutations(3, 42, DEFAULT_PERMUTATIONS) {
            let permuted = permute_case(&case, &permutation).expect("permute");
            assert!(permuted.none, "no-tool flag must survive permutation");
            assert!(permuted.relevance.is_empty());
            assert!(target_index(&permuted).is_none());
        }
    }

    #[test]
    fn train_dev_test_ownership_never_changes_across_views() {
        let cases = load_cases(None).expect("builtin corpus");
        let partition = partition_cases(&cases);
        let views = balanced_training_views(&cases, &partition.train_cases, TRAIN_VIEW_SEED)
            .expect("train views");
        assert!(!views.is_empty());
        // Ownership is inherited from the canonical corpus partition, which
        // is computed once on unaugmented cases. Every view keeps its
        // source leakage keys (identical normalized input, same
        // variant/semantic/manual groups), so it belongs to its source
        // leakage component and source split by construction. Views are
        // training input only and are never re-partitioned independently:
        // content-derived group ids hash member signatures, so duplicating
        // an identical input inside the partitioned set would rename the
        // component. The contract therefore binds each view to its
        // source split instead of re-running the partitioner.
        let source_by_id: BTreeMap<&str, &ToolAdvisorCase> = cases
            .iter()
            .map(|case| (case.case_id.as_str(), case))
            .collect();
        let split_of: BTreeMap<&str, &str> = partition
            .train_cases
            .iter()
            .map(|i| (cases[*i].case_id.as_str(), "train"))
            .chain(
                partition
                    .dev_cases
                    .iter()
                    .map(|i| (cases[*i].case_id.as_str(), "dev")),
            )
            .chain(
                partition
                    .test_cases
                    .iter()
                    .map(|i| (cases[*i].case_id.as_str(), "test")),
            )
            .collect();
        for view in &views {
            let source = source_by_id
                .get(view.source_case_id.as_str())
                .expect("view traces to a source case");
            assert_eq!(
                split_of.get(view.source_case_id.as_str()),
                Some(&"train"),
                "training views derive from train sources only"
            );
            assert_eq!(
                super::super::input_signature(&view.case),
                super::super::input_signature(source),
                "view must share its source normalized input"
            );
            assert_eq!(view.case.semantic_group, source.semantic_group);
            assert_eq!(view.case.leakage_group, source.leakage_group);
            assert_eq!(
                view.case.generated_variant_family, source.generated_variant_family,
                "template lineage must not fork per permutation"
            );
            assert_eq!(view.case.context, source.context);
            assert_eq!(view.case.relevance, source.relevance);
            assert_eq!(view.case.none, source.none);
        }
        // Dev suite contains no test cases and records seed + fingerprint.
        let suite = dev_permutation_suite(
            &cases,
            &partition.dev_cases,
            DEV_SUITE_SEED,
            DEFAULT_PERMUTATIONS,
        )
        .expect("dev suite");
        assert_eq!(suite.case_ids.len(), partition.dev_cases.len());
        assert_eq!(suite.seed, DEV_SUITE_SEED);
        assert!(!suite.suite_fingerprint.is_empty());
        let dev_ids: BTreeSet<&str> = partition
            .dev_cases
            .iter()
            .map(|i| cases[*i].case_id.as_str())
            .collect();
        for id in &suite.case_ids {
            assert!(dev_ids.contains(id.as_str()));
        }
    }

    #[test]
    fn target_position_balancing_meets_declared_tolerance() {
        let cases = load_cases(None).expect("builtin corpus");
        let partition = partition_cases(&cases);
        let views = balanced_training_views(&cases, &partition.train_cases, TRAIN_VIEW_SEED)
            .expect("train views");
        let deviation = augmented_position_deviation_pp(&views);
        // Exact construction places each source target uniformly, so the
        // aggregate deviation is effectively zero; the 5pp gate holds with
        // wide margin. No-tool views are excluded from the aggregate.
        assert!(
            deviation <= 5.0,
            "augmented position deviation {deviation:.2}pp exceeds 5pp"
        );
    }

    #[test]
    fn permutation_seed_is_deterministic() {
        let first = generate_permutations(5, 1234, DEFAULT_PERMUTATIONS);
        let second = generate_permutations(5, 1234, DEFAULT_PERMUTATIONS);
        assert_eq!(first, second);
        let other = generate_permutations(5, 9999, DEFAULT_PERMUTATIONS);
        assert_ne!(first, other, "different seeds must diverge");
        // Small sets enumerate exhaustively up to the bound.
        let tiny = generate_permutations(2, 7, DEFAULT_PERMUTATIONS);
        assert_eq!(tiny.len(), 2);
    }

    struct PositionBiasedAdvisor;

    impl ToolAdvisor for PositionBiasedAdvisor {
        fn score(
            &self,
            input: &super::super::ToolAdvisorInput,
        ) -> anyhow::Result<super::super::ToolAdvisorPrediction> {
            // Scores purely by presentation position: exposes the shortcut.
            let ranked = input
                .candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| super::super::RankedCandidate {
                    name: candidate.name.clone(),
                    score: 100.0 - index as f64,
                })
                .collect();
            Ok(super::super::ToolAdvisorPrediction {
                schema_version: super::super::PREDICTION_SCHEMA_VERSION,
                case_id: input.case_id.clone(),
                ranked,
                abstain_probability: Some(0.0),
                mode: "position-biased".into(),
            })
        }
    }

    #[test]
    fn mapped_back_predictions_compare_by_identity_not_index() {
        // A position-biased scorer always ranks presentation index 0 first.
        // Mapped back by identity, top-1 must therefore vary with the
        // permutation instead of collapsing to one index.
        let case = sample_case();
        let metrics = evaluate_permutation_robustness(
            &case,
            &PositionBiasedAdvisor,
            11,
            DEFAULT_PERMUTATIONS,
        )
        .expect("metrics");
        assert_eq!(metrics.permutations, 6, "3 candidates enumerate 6 orders");
        assert!(
            metrics.top1_identity_consistency < 1.0,
            "position bias must show identity inconsistency"
        );
        assert!(
            metrics.worst_case_regret > 0.0,
            "position bias must show permutation regret"
        );
        assert_eq!(
            metrics.true_target_position,
            Some(0),
            "sample target starts at index 0"
        );
    }

    #[test]
    fn historical_position_histogram_reproduces_known_values() {
        let cases = load_cases(None).expect("builtin corpus");
        assert_eq!(cases.len(), 256, "historical corpus total");
        let non_no_tool = cases.iter().filter(|case| !case.none).count();
        assert_eq!(non_no_tool, 207, "historical non-no-tool count");
        let histogram = target_position_histogram(&cases);
        assert_eq!(histogram.get(&0).copied().unwrap_or(0), 201);
        assert_eq!(histogram.get(&1).copied().unwrap_or(0), 6);
        assert!(
            histogram.keys().all(|index| *index <= 1),
            "no historical target beyond index 1: {histogram:?}"
        );
        // Partition detail is derived, never hard-coded.
        let partition = partition_cases(&cases);
        let report =
            build_diagnostics_report(&cases, "assets/tool-advisor/corpus.jsonl").expect("report");
        assert_eq!(
            report.train_cases + report.dev_cases + report.test_cases,
            256
        );
        assert!(!report.train_fingerprint.is_empty());
        assert!(!report.dev_fingerprint.is_empty());
        assert!(!report.test_fingerprint.is_empty());
        assert_eq!(partition.train_cases.len(), report.train_cases);
    }

    #[test]
    fn v3_diagnostic_histogram_reproduces_known_values() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/tool-advisor/qualification-v3-holdout.jsonl");
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return;
        };
        let cases = super::super::parse_jsonl(&contents).expect("v3 holdout");
        let non_no_tool = cases.iter().filter(|case| !case.none).count();
        assert_eq!(non_no_tool, 146, "v3 non-no-tool count");
        let histogram = target_position_histogram(&cases);
        assert_eq!(histogram.get(&2).copied().unwrap_or(0), 133);
        assert_eq!(histogram.get(&3).copied().unwrap_or(0), 13);
        assert_eq!(histogram.get(&0).copied().unwrap_or(0), 0);
        assert_eq!(histogram.get(&1).copied().unwrap_or(0), 0);
    }

    #[test]
    fn order_diagnostics_report_is_machine_readable() {
        let cases = load_cases(None).expect("builtin corpus");
        let report =
            build_diagnostics_report(&cases, "assets/tool-advisor/corpus.jsonl").expect("report");
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.contract_version, PERMUTATION_CONTRACT_VERSION);
        assert_eq!(report.total_cases, 256);
        assert_eq!(report.non_no_tool_cases, 207);
        assert!(!report.source_fingerprint.is_empty());
        assert!(!report.dev_suite_fingerprint.is_empty());
        assert_eq!(report.dev_suite_seed, DEV_SUITE_SEED);
        assert_eq!(report.train_view_seed, TRAIN_VIEW_SEED);
        assert!(report.augmented_train_views > report.train_cases);
        let encoded = serde_json::to_string(&report).expect("report serializes");
        let decoded: OrderDiagnosticsReport =
            serde_json::from_str(&encoded).expect("report round-trips");
        assert_eq!(decoded.source_fingerprint, report.source_fingerprint);
    }
}

/// Frozen-artifact permutation diagnostic (M001 §4).
///
/// Gated on the encoder-training feature because it loads the real
/// packed MiniLM artifact. Skips cleanly when local reference assets
/// are absent; the M001 closure record carries the machine evidence
/// from an environment where they are present.
#[cfg(all(test, feature = "tool-advisor-encoder-training"))]
mod artifact_diagnostic_tests {
    use super::super::sequence_ranking;
    use super::*;
    use std::path::Path;

    struct RankerAdvisor {
        ranker: sequence_ranking::SequenceRanker,
    }

    impl ToolAdvisor for RankerAdvisor {
        fn score(
            &self,
            input: &super::super::ToolAdvisorInput,
        ) -> anyhow::Result<super::super::ToolAdvisorPrediction> {
            let case = super::super::ToolAdvisorCase {
                schema_version: super::super::CASE_SCHEMA_VERSION,
                case_id: input.case_id.clone(),
                context: input.context.clone(),
                candidates: input.candidates.clone(),
                relevance: std::collections::BTreeMap::new(),
                preferred_order: Vec::new(),
                none: true,
                tags: Vec::new(),
                group_id: "diagnostic".into(),
                provenance: "order-invariance-diagnostic".into(),
                semantic_group: String::new(),
                leakage_group: String::new(),
                task_family: String::new(),
                tool_family: String::new(),
                generated_variant_family: String::new(),
                teacher_probabilities: std::collections::BTreeMap::new(),
            };
            let (prediction, _, _) = self.ranker.predict_case(&case)?;
            Ok(prediction)
        }
    }

    #[test]
    fn frozen_packed_artifact_shows_order_sensitivity_on_dev_sample() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let artifact_path =
            root.join("target/tool-advisor/sequence-ranking-minilm-packed-head.json");
        if !artifact_path.exists() {
            eprintln!("SKIP: packed artifact absent; closure carries local evidence");
            return;
        }
        let device = candle_core::Device::Cpu;
        let Ok(ranker) = sequence_ranking::load_artifact(&artifact_path, &device) else {
            eprintln!("SKIP: packed artifact failed to load in this environment");
            return;
        };
        let advisor = RankerAdvisor { ranker };
        let cases = super::super::load_cases(None).expect("builtin corpus");
        let partition = super::super::partition_cases(&cases);
        // Bounded sample keeps the suite fast; full dev/test/v3 evidence
        // is gathered once for the closure record, not on every test run.
        let sample: Vec<&ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| &cases[*index])
            .filter(|case| !case.none)
            .take(4)
            .collect();
        assert!(!sample.is_empty());
        let mut metrics = Vec::new();
        for case in sample {
            // Eight deterministic presentations bound runtime while still
            // exposing order sensitivity on 3-5 candidate cases.
            let metric = evaluate_permutation_robustness(case, &advisor, DEV_SUITE_SEED, 8)
                .expect("permutation metrics");
            assert_eq!(metric.case_id, case.case_id);
            metrics.push(metric);
        }
        let summary = summarize_suite(&metrics);
        let encoded = serde_json::to_string(&summary).expect("summary serializes");
        assert!(!encoded.is_empty());
        // The diagnostic asserts measurability, not a fixed verdict: the
        // closure record interprets the numbers. Consistency bounds guard
        // against a vacuous harness (a constant scorer would report 1.0
        // with zero drift and zero regret, which this rejects).
        let vacuous = summary.mean_top1_consistency >= 1.0
            && summary.mean_max_score_drift <= 0.0
            && summary.max_regret <= 0.0;
        assert!(!vacuous, "diagnostic suite must discriminate: {encoded}");
    }

    /// Full M001 §4 artifact diagnostic (ignored: ~10 min CPU).
    ///
    /// Run explicitly for closure evidence:
    /// `cargo test --locked --features tool-advisor-encoder-training -p
    /// codegg --lib tool_advisor::order_invariance::artifact_diagnostic_tests::
    /// full_artifact_permutation_diagnostic -- --ignored --nocapture`
    ///
    /// Scores the frozen packed artifact on deterministic samples of the
    /// historical dev partition, the historical test partition
    /// (diagnostic only), and the observed v3 holdout (diagnostic
    /// only), then writes machine receipts under
    /// `target/tool-advisor/order-invariance/`. Sampling and the
    /// 8-permutation bound are documented in the receipt: the default
    /// contract (20 permutations) is preserved for M002+ consumers,
    /// while this one-off diagnostic bounds CPU cost.
    #[test]
    #[ignore]
    fn full_artifact_permutation_diagnostic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let artifact_path =
            root.join("target/tool-advisor/sequence-ranking-minilm-packed-head.json");
        if !artifact_path.exists() {
            eprintln!("SKIP: packed artifact absent; cannot gather diagnostic");
            return;
        }
        let device = candle_core::Device::Cpu;
        let ranker = sequence_ranking::load_artifact(&artifact_path, &device)
            .expect("load frozen packed artifact");
        let advisor = RankerAdvisor { ranker };
        let cases = super::super::load_cases(None).expect("builtin corpus");
        let partition = super::super::partition_cases(&cases);
        let v3_path = root.join("assets/tool-advisor/qualification-v3-holdout.jsonl");
        let v3_contents = std::fs::read_to_string(&v3_path).expect("read v3 holdout");
        let v3_cases = super::super::parse_jsonl(&v3_contents).expect("parse v3 holdout");

        eprintln!(
            "partitions: train={} dev={} test={} (of {}); v3={}",
            partition.train_cases.len(),
            partition.dev_cases.len(),
            partition.test_cases.len(),
            cases.len(),
            v3_cases.len()
        );

        #[derive(serde::Serialize)]
        struct SplitDiagnostic {
            split: String,
            sampled_cases: usize,
            permutations_per_case: u8,
            summary: super::SuitePermutationSummary,
            metrics: Vec<super::PermutationMetrics>,
        }

        fn sample<'a>(
            pool: &'a [super::ToolAdvisorCase],
            non_no_tool: usize,
            no_tool: usize,
        ) -> Vec<&'a super::ToolAdvisorCase> {
            let mut labeled: Vec<&'a super::ToolAdvisorCase> =
                pool.iter().filter(|case| !case.none).collect();
            labeled.sort_by(|left, right| left.case_id.cmp(&right.case_id));
            let mut abstentions: Vec<&'a super::ToolAdvisorCase> =
                pool.iter().filter(|case| case.none).collect();
            abstentions.sort_by(|left, right| left.case_id.cmp(&right.case_id));
            labeled
                .into_iter()
                .take(non_no_tool)
                .chain(abstentions.into_iter().take(no_tool))
                .collect()
        }

        let dev_pool: Vec<super::ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        let test_pool: Vec<super::ToolAdvisorCase> = partition
            .test_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();

        let mut splits = Vec::new();
        for (split, pool) in [
            ("historical-dev", &dev_pool),
            ("historical-test-diagnostic-only", &test_pool),
            ("observed-v3-diagnostic-only", &v3_cases),
        ] {
            // Deterministic case-id-ordered sample: 10 labeled + 3
            // no-tool per split. Documented in the receipt.
            let sampled = sample(pool, 10, 3);
            let mut metrics = Vec::new();
            for case in sampled {
                // 8 deterministic presentations bound CPU while covering
                // every valid target position on <=8-candidate cases.
                metrics.push(
                    super::evaluate_permutation_robustness(
                        case,
                        &advisor,
                        super::DEV_SUITE_SEED,
                        8,
                    )
                    .expect("permutation metrics"),
                );
            }
            let summary = super::summarize_suite(&metrics);
            eprintln!(
                "{split}: cases={} top1_consistency={:.3} max_drift={:.3} \
                 worst_mrr={:.3} best_mrr={:.3} regret={:.3}",
                summary.cases,
                summary.mean_top1_consistency,
                summary.mean_max_score_drift,
                summary.worst_mrr,
                summary.best_mrr,
                summary.max_regret,
            );
            splits.push(SplitDiagnostic {
                split: split.to_string(),
                sampled_cases: summary.cases,
                permutations_per_case: 8,
                summary,
                metrics,
            });
        }

        #[derive(serde::Serialize)]
        struct DiagnosticReceipt {
            artifact: String,
            contract_version: u16,
            seed: u64,
            sampling: String,
            splits: Vec<SplitDiagnostic>,
            order_bias: super::OrderDiagnosticsReport,
        }

        let receipt = DiagnosticReceipt {
            artifact: "sequence-encoder-packed-marker-v1 (frozen)".to_string(),
            contract_version: super::PERMUTATION_CONTRACT_VERSION,
            seed: super::DEV_SUITE_SEED,
            sampling: "deterministic case-id-ordered sample: first 10 non-no-tool + \
                first 3 no-tool per split; 8 deterministic permutations per case"
                .to_string(),
            splits,
            order_bias: super::build_diagnostics_report(&cases, "assets/tool-advisor/corpus.jsonl")
                .expect("order-bias report"),
        };
        let out_dir = root.join("target/tool-advisor/order-invariance");
        std::fs::create_dir_all(&out_dir).expect("create diagnostic output dir");
        std::fs::write(
            out_dir.join("m001-artifact-diagnostic.json"),
            serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
        )
        .expect("write artifact diagnostic");
        std::fs::write(
            out_dir.join("m001-order-diagnostics.json"),
            serde_json::to_vec_pretty(&receipt.order_bias).expect("serialize report"),
        )
        .expect("write order diagnostics");
        eprintln!("receipts written under {}", out_dir.display());
    }
}
