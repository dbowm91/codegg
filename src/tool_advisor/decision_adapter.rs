//! Adapter from CodeGG's authority-filtered tool surface into the
//! backend-neutral decision contract used by learned tool advice.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

use codegg_core::decision::{
    BackendState, DecisionAnswer, DecisionCandidate, DecisionEngine, DecisionError,
    DecisionProvenance, DecisionRequest, DecisionResponse, DecisionSpec, DecisionStatus,
    RankedDecisionCandidate, StateField, DECISION_SCHEMA_VERSION, MAX_STATE_VALUE_BYTES,
};

use super::{RankedCandidate, ToolAdvisorInput, ToolAdvisorPrediction, PREDICTION_SCHEMA_VERSION};

/// One bounded learned ranking result plus backend provenance for diagnostics.
pub struct DecisionRankResult {
    pub prediction: ToolAdvisorPrediction,
    pub provenance: DecisionProvenance,
}

/// Submit a Rank request through the selected backend and validate the full
/// response against the immutable request before projecting it to advisor
/// policy. The caller supplies only candidates already admitted by CodeGG.
pub async fn rank_with_engine(
    engine: &Arc<dyn DecisionEngine>,
    input: &ToolAdvisorInput,
    timeout: Duration,
) -> anyhow::Result<DecisionRankResult> {
    if !matches!(engine.state(), BackendState::Ready) {
        anyhow::bail!("decision backend is not ready for inference");
    }
    let capabilities = engine.capabilities();
    if !capabilities.rank {
        anyhow::bail!("decision backend does not support Rank");
    }
    if input.candidates.len() > capabilities.max_candidates {
        anyhow::bail!("decision candidate count exceeds backend capability");
    }
    let request = rank_request_from_input(input)?;
    let response = engine
        .decide(request.clone(), Instant::now() + timeout)
        .await?;
    response.validate_for(&request)?;
    let (ranked, abstain_probability) = match (&response.status, &response.answer) {
        (DecisionStatus::Answered, Some(DecisionAnswer::Rank { candidates })) => (
            candidates
                .iter()
                .map(|candidate| RankedCandidate {
                    name: candidate.candidate_id.clone(),
                    score: candidate.relevance,
                })
                .collect(),
            None,
        ),
        (DecisionStatus::Abstained { .. }, None) => (Vec::new(), Some(1.0)),
        (DecisionStatus::Unsupported { reason } | DecisionStatus::Unavailable { reason }, _) => {
            anyhow::bail!("decision backend declined Rank: {reason}")
        }
        _ => anyhow::bail!("decision backend returned an incompatible Rank response"),
    };
    Ok(DecisionRankResult {
        prediction: ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            ranked,
            abstain_probability,
            mode: response
                .provenance
                .model
                .clone()
                .unwrap_or_else(|| response.provenance.backend.clone()),
        },
        provenance: response.provenance,
    })
}

/// Build a bounded generic Rank request from the caller's already-filtered
/// advisor projection. It has no registry, permission, or execution access.
pub fn rank_request_from_input(input: &ToolAdvisorInput) -> Result<DecisionRequest, DecisionError> {
    if input.context.len() > MAX_STATE_VALUE_BYTES {
        return Err(DecisionError::InvalidRequest(
            "decision context exceeds limit".into(),
        ));
    }
    let request = DecisionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        schema_version: DECISION_SCHEMA_VERSION,
        state: vec![
            StateField {
                key: "context".into(),
                value: input.context.clone(),
            },
            StateField {
                key: "surface_fingerprint".into(),
                value: input.surface_fingerprint.clone(),
            },
        ],
        spec: DecisionSpec::Rank {
            instruction: "Estimate each candidate's relevance to the supplied context.".into(),
            candidates: input
                .candidates
                .iter()
                .map(|candidate| DecisionCandidate {
                    id: candidate.name.clone(),
                    label: candidate.name.clone(),
                    description: candidate.description.clone(),
                })
                .collect(),
            multi_relevance: true,
        },
    };
    request.validate()?;
    Ok(request)
}

/// Construct a Rank request from names selected by the caller and descriptors
/// already admitted by `ResolvedToolSurface`. This function never consults a
/// registry, permission service, or tool backend.
pub fn rank_request_from_surface(
    request_id: impl Into<String>,
    context: &str,
    surface: &crate::agent::tool_surface::ResolvedToolSurface,
    candidate_names: &BTreeSet<String>,
) -> Result<DecisionRequest, DecisionError> {
    if context.len() > MAX_STATE_VALUE_BYTES {
        return Err(DecisionError::InvalidRequest(
            "decision context exceeds limit".into(),
        ));
    }
    let candidates = surface
        .tools
        .iter()
        .filter(|tool| !tool.required && !tool.never_reduce)
        .filter(|tool| candidate_names.contains(&tool.canonical_name))
        .map(|tool| DecisionCandidate {
            id: tool.canonical_name.clone(),
            label: tool.definition.name.clone(),
            description: tool.definition.description.clone(),
        })
        .collect();
    let request = DecisionRequest {
        request_id: request_id.into(),
        schema_version: DECISION_SCHEMA_VERSION,
        state: vec![StateField {
            key: "context".into(),
            value: context.into(),
        }],
        spec: DecisionSpec::Rank {
            instruction: "Estimate each candidate's relevance to the supplied context.".into(),
            candidates,
            multi_relevance: true,
        },
    };
    request.validate()?;
    Ok(request)
}

/// Convert historical advisor output to the generic shape for compatibility
/// fixtures. It does not make the prediction authoritative or change ordering.
pub fn prediction_to_response(
    request: &DecisionRequest,
    prediction: &ToolAdvisorPrediction,
) -> Result<DecisionResponse, DecisionError> {
    request.validate()?;
    if prediction.case_id != request.request_id {
        return Err(DecisionError::InvalidResponse(
            "legacy prediction request identity mismatch".into(),
        ));
    }
    let DecisionSpec::Rank { candidates, .. } = &request.spec else {
        return Err(DecisionError::InvalidResponse(
            "compatibility adapter requires Rank".into(),
        ));
    };
    let mut by_name = BTreeMap::new();
    for item in &prediction.ranked {
        if !item.score.is_finite() || by_name.insert(item.name.as_str(), item.score).is_some() {
            return Err(DecisionError::InvalidResponse(
                "duplicate or non-finite legacy score".into(),
            ));
        }
        if !candidates.iter().any(|candidate| candidate.id == item.name) {
            return Err(DecisionError::InvalidResponse(
                "legacy prediction names an unknown candidate".into(),
            ));
        }
    }
    let abstained = prediction
        .abstain_probability
        .is_some_and(|value| value >= 0.5);
    let answer = if abstained {
        None
    } else {
        let ranked = candidates
            .iter()
            .map(|candidate| {
                let score = by_name.get(candidate.id.as_str()).copied().unwrap_or(0.0);
                RankedDecisionCandidate {
                    candidate_id: candidate.id.clone(),
                    relevance: 1.0 / (1.0 + (-score).exp()),
                    ranking_score: score,
                }
            })
            .collect();
        Some(DecisionAnswer::Rank { candidates: ranked })
    };
    Ok(DecisionResponse {
        request_id: request.request_id.clone(),
        schema_version: request.schema_version,
        status: if abstained {
            DecisionStatus::Abstained {
                reason: Some("legacy advisor abstained".into()),
            }
        } else {
            DecisionStatus::Answered
        },
        answer,
        provenance: DecisionProvenance {
            backend: "legacy-tool-advisor-fixture".into(),
            model: Some(prediction.mode.clone()),
            runtime_version: Some(format!("prediction-schema-{}", prediction.schema_version)),
            latency_micros: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent::tool_surface::ResolvedToolSurface, provider::ToolDefinition};
    use codegg_core::decision::{BackendCapabilities, DecisionCandidate, RankedDecisionCandidate};
    use std::collections::BTreeSet;

    struct RankEngine {
        rank: bool,
        forge: bool,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl DecisionEngine for RankEngine {
        fn state(&self) -> BackendState {
            BackendState::Ready
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                binary: false,
                choice: false,
                score: false,
                rank: self.rank,
                max_options: 0,
                max_candidates: 16,
            }
        }

        async fn decide(
            &self,
            request: DecisionRequest,
            _deadline: Instant,
        ) -> Result<DecisionResponse, DecisionError> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let DecisionSpec::Rank { candidates, .. } = request.spec else {
                panic!("expected Rank request")
            };
            let ranked = candidates
                .into_iter()
                .enumerate()
                .map(|(index, candidate)| RankedDecisionCandidate {
                    candidate_id: if self.forge {
                        "forged".into()
                    } else {
                        candidate.id
                    },
                    relevance: 0.9 - index as f64 * 0.1,
                    ranking_score: 2.0 - index as f64,
                })
                .collect();
            Ok(DecisionResponse {
                request_id: request.request_id,
                schema_version: request.schema_version,
                status: DecisionStatus::Answered,
                answer: Some(DecisionAnswer::Rank { candidates: ranked }),
                provenance: DecisionProvenance {
                    backend: "fixture-rank".into(),
                    model: Some("fixture".into()),
                    runtime_version: Some("test".into()),
                    latency_micros: 7,
                },
            })
        }
    }

    fn advisor_input() -> ToolAdvisorInput {
        ToolAdvisorInput {
            case_id: "adapter-test".into(),
            context: "read project files".into(),
            candidates: vec![
                super::super::ToolAdvisorCandidate {
                    name: "read".into(),
                    description: "read a file".into(),
                    category: "workspace".into(),
                    disclosure: "small".into(),
                    synthetic_identity: false,
                },
                super::super::ToolAdvisorCandidate {
                    name: "glob".into(),
                    description: "find files".into(),
                    category: "workspace".into(),
                    disclosure: "small".into(),
                    synthetic_identity: false,
                },
            ],
            surface_fingerprint: "surface-fingerprint".into(),
        }
    }

    #[tokio::test]
    async fn generic_rank_maps_provenance_and_only_caller_candidates() {
        let engine = Arc::new(RankEngine {
            rank: true,
            forge: false,
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let engine_trait: Arc<dyn DecisionEngine> = engine.clone();
        let result = rank_with_engine(&engine_trait, &advisor_input(), Duration::from_millis(50))
            .await
            .expect("rank decision");
        assert_eq!(
            result
                .prediction
                .ranked
                .iter()
                .map(|candidate| candidate.name.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["read", "glob"])
        );
        assert_eq!(result.provenance.backend, "fixture-rank");
        assert_eq!(engine.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn unsupported_rank_never_calls_backend_and_mismatched_ids_reject() {
        let unsupported = Arc::new(RankEngine {
            rank: false,
            forge: false,
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let unsupported_trait: Arc<dyn DecisionEngine> = unsupported.clone();
        assert!(rank_with_engine(
            &unsupported_trait,
            &advisor_input(),
            Duration::from_millis(50)
        )
        .await
        .is_err());
        assert_eq!(
            unsupported.calls.load(std::sync::atomic::Ordering::Relaxed),
            0
        );

        let forged = Arc::new(RankEngine {
            rank: true,
            forge: true,
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let forged_trait: Arc<dyn DecisionEngine> = forged;
        assert!(
            rank_with_engine(&forged_trait, &advisor_input(), Duration::from_millis(50))
                .await
                .is_err()
        );
    }

    fn request() -> DecisionRequest {
        DecisionRequest {
            request_id: "case-1".into(),
            schema_version: DECISION_SCHEMA_VERSION,
            state: vec![StateField {
                key: "context".into(),
                value: "inspect files".into(),
            }],
            spec: DecisionSpec::Rank {
                instruction: "rank".into(),
                multi_relevance: true,
                candidates: vec![
                    DecisionCandidate {
                        id: "read".into(),
                        label: "read".into(),
                        description: "read files".into(),
                    },
                    DecisionCandidate {
                        id: "glob".into(),
                        label: "glob".into(),
                        description: "find files".into(),
                    },
                ],
            },
        }
    }

    #[test]
    fn legacy_prediction_maps_to_exact_generic_candidate_set() {
        let request = request();
        let prediction = ToolAdvisorPrediction {
            schema_version: 1,
            case_id: "case-1".into(),
            ranked: vec![super::super::RankedCandidate {
                name: "read".into(),
                score: 2.0,
            }],
            abstain_probability: None,
            mode: "fixture".into(),
        };
        let response = prediction_to_response(&request, &prediction).unwrap();
        response.validate_for(&request).unwrap();
        let Some(DecisionAnswer::Rank { candidates }) = response.answer else {
            panic!("expected rank")
        };
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].ranking_score, 2.0);
        assert_eq!(candidates[1].ranking_score, 0.0);
    }

    #[test]
    fn legacy_abstention_is_explicit_and_unknown_names_reject() {
        let request = request();
        let mut prediction = ToolAdvisorPrediction {
            schema_version: 1,
            case_id: "case-1".into(),
            ranked: vec![],
            abstain_probability: Some(0.75),
            mode: "fixture".into(),
        };
        assert!(matches!(
            prediction_to_response(&request, &prediction)
                .unwrap()
                .status,
            DecisionStatus::Abstained { .. }
        ));
        prediction.ranked.push(super::super::RankedCandidate {
            name: "synthetic".into(),
            score: 1.0,
        });
        assert!(prediction_to_response(&request, &prediction).is_err());
    }

    #[test]
    fn rank_answer_retains_multiple_relevant_candidates() {
        let request = request();
        let response = DecisionResponse {
            request_id: request.request_id.clone(),
            schema_version: request.schema_version,
            status: DecisionStatus::Answered,
            answer: Some(DecisionAnswer::Rank {
                candidates: vec![
                    RankedDecisionCandidate {
                        candidate_id: "read".into(),
                        relevance: 0.8,
                        ranking_score: 2.0,
                    },
                    RankedDecisionCandidate {
                        candidate_id: "glob".into(),
                        relevance: 0.6,
                        ranking_score: 1.0,
                    },
                ],
            }),
            provenance: DecisionProvenance {
                backend: "fixture".into(),
                model: None,
                runtime_version: None,
                latency_micros: 0,
            },
        };
        response.validate_for(&request).unwrap();
    }

    #[test]
    fn adapter_cannot_project_denied_or_non_surface_tools() {
        let definition = |name: &str| ToolDefinition {
            name: name.into(),
            description: format!("description for {name}"),
            parameters: serde_json::json!({"type":"object"}),
            defer_loading: None,
        };
        let denied = BTreeSet::from(["secret".to_string()]);
        let surface = ResolvedToolSurface::resolve(
            [
                definition("glob"),
                definition("read"),
                definition("secret"),
                definition("mcp__server__synthetic"),
            ],
            &denied,
            &BTreeSet::new(),
            false,
            false,
            None,
        )
        .unwrap();
        let requested = BTreeSet::from([
            "glob".to_string(),
            "read".to_string(),
            "secret".to_string(),
            "outside_surface".to_string(),
            "mcp__server__synthetic".to_string(),
        ]);
        let request =
            rank_request_from_surface("authority", "inspect", &surface, &requested).unwrap();
        let DecisionSpec::Rank { candidates, .. } = request.spec else {
            panic!("expected Rank")
        };
        let names: BTreeSet<_> = candidates
            .into_iter()
            .map(|candidate| candidate.id)
            .collect();
        assert_eq!(
            names,
            BTreeSet::from(["glob".into(), "mcp__server__synthetic".into()])
        );
    }
}
