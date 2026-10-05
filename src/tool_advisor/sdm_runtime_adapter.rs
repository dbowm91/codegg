//! Compatibility adapter from CodeGG's tool-advisor input to the pinned SDM runtime.
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use codegg_core::decision::{
    DecisionCandidate, DecisionEngine, DecisionRequest, DecisionSpec, DecisionStatus, StateField,
    DECISION_SCHEMA_VERSION,
};

use super::{
    RankedCandidate, ToolAdvisor, ToolAdvisorInput, ToolAdvisorPrediction,
    PREDICTION_SCHEMA_VERSION,
};

pub struct SdmToolAdvisor {
    engine: Arc<dyn DecisionEngine>,
    timeout: Duration,
}

impl SdmToolAdvisor {
    pub fn new(engine: Arc<dyn DecisionEngine>, timeout: Duration) -> Self {
        Self { engine, timeout }
    }
}

impl ToolAdvisor for SdmToolAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        let request = DecisionRequest {
            request_id: input.case_id.clone(),
            schema_version: DECISION_SCHEMA_VERSION,
            state: vec![StateField {
                key: "context".into(),
                value: input.context.clone(),
            }],
            spec: DecisionSpec::Rank {
                instruction:
                    "Estimate relevance independently for every caller-supplied candidate.".into(),
                multi_relevance: true,
                candidates: input
                    .candidates
                    .iter()
                    .map(|candidate| DecisionCandidate {
                        id: candidate.name.clone(),
                        label: candidate.name.clone(),
                        description: candidate.description.clone(),
                    })
                    .collect(),
            },
        };
        request
            .validate()
            .map_err(|error| anyhow!("invalid local decision input: {error}"))?;
        let response = futures_executor::block_on(
            self.engine
                .decide(request.clone(), Instant::now() + self.timeout),
        )
        .map_err(|error| anyhow!("local decision backend failed: {error}"))?;
        response
            .validate_for(&request)
            .map_err(|error| anyhow!("local decision response failed validation: {error}"))?;
        let (ranked, abstain_probability) = match (&response.status, &response.answer) {
            (
                DecisionStatus::Answered,
                Some(codegg_core::decision::DecisionAnswer::Rank { candidates }),
            ) => {
                let known: BTreeSet<_> = input
                    .candidates
                    .iter()
                    .map(|candidate| candidate.name.as_str())
                    .collect();
                if candidates
                    .iter()
                    .any(|candidate| !known.contains(candidate.candidate_id.as_str()))
                {
                    return Err(anyhow!(
                        "local decision returned an unknown candidate identity"
                    ));
                }
                let confidence = candidates
                    .iter()
                    .map(|candidate| candidate.relevance)
                    .fold(0.0_f64, f64::max);
                (
                    candidates
                        .iter()
                        .map(|candidate| RankedCandidate {
                            name: candidate.candidate_id.clone(),
                            score: candidate.relevance,
                        })
                        .collect(),
                    Some(1.0 - confidence),
                )
            }
            (DecisionStatus::Abstained { .. }, None) => (Vec::new(), Some(1.0)),
            (
                DecisionStatus::Unsupported { reason } | DecisionStatus::Unavailable { reason },
                _,
            ) => return Err(anyhow!("local decision unavailable: {reason}")),
            _ => {
                return Err(anyhow!(
                    "local decision response has incompatible semantics"
                ))
            }
        };
        Ok(ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            ranked,
            abstain_probability,
            mode: format!(
                "sdm-local:{}",
                response.provenance.model.unwrap_or_default()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_advisor::{ToolAdvisorCandidate, ToolAdvisorInput};

    struct FixedEngine;
    #[async_trait::async_trait]
    impl DecisionEngine for FixedEngine {
        fn state(&self) -> codegg_core::decision::BackendState {
            codegg_core::decision::BackendState::Ready
        }
        fn capabilities(&self) -> codegg_core::decision::BackendCapabilities {
            codegg_core::decision::BackendCapabilities {
                binary: false,
                choice: false,
                score: false,
                rank: true,
                max_options: 0,
                max_candidates: 8,
            }
        }
        async fn decide(
            &self,
            request: DecisionRequest,
            _: Instant,
        ) -> std::result::Result<
            codegg_core::decision::DecisionResponse,
            codegg_core::decision::DecisionError,
        > {
            let response = codegg_core::decision::DecisionResponse {
                request_id: request.request_id.clone(),
                schema_version: request.schema_version,
                status: codegg_core::decision::DecisionStatus::Answered,
                answer: Some(codegg_core::decision::DecisionAnswer::Rank {
                    candidates: vec![codegg_core::decision::RankedDecisionCandidate {
                        candidate_id: "read".into(),
                        relevance: 0.9,
                        ranking_score: 2.0,
                    }],
                }),
                provenance: codegg_core::decision::DecisionProvenance {
                    backend: "test".into(),
                    model: None,
                    runtime_version: None,
                    latency_micros: 0,
                },
            };
            response.validate_for(&request)?;
            Ok(response)
        }
    }

    #[test]
    fn adapter_maps_valid_rank_and_rejects_backend_authority_expansion() {
        let advisor = SdmToolAdvisor::new(Arc::new(FixedEngine), Duration::from_millis(25));
        let input = ToolAdvisorInput {
            case_id: "case".into(),
            context: "read file".into(),
            surface_fingerprint: "surface".into(),
            candidates: vec![ToolAdvisorCandidate {
                name: "read".into(),
                description: "read".into(),
                category: String::new(),
                disclosure: String::new(),
                synthetic_identity: false,
            }],
        };
        let prediction = advisor.score(&input).unwrap();
        assert_eq!(prediction.ranked[0].name, "read");
        let mut expanded = input;
        expanded.candidates[0].name = "denied".into();
        assert!(advisor.score(&expanded).is_err());
    }

    #[test]
    fn empty_candidate_request_is_rejected_before_backend_call() {
        let advisor = SdmToolAdvisor::new(Arc::new(FixedEngine), Duration::from_millis(25));
        let input = ToolAdvisorInput {
            case_id: "empty".into(),
            context: String::new(),
            surface_fingerprint: String::new(),
            candidates: Vec::new(),
        };
        assert!(advisor.score(&input).is_err());
    }
}
