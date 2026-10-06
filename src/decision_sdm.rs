//! Adapter from CodeGG's backend-neutral decision interface to the pinned SDM
//! local artifact runtime. Artifacts are immutable after load for this process.
use std::{fs::File, io::Read, path::Path, sync::Arc, time::Instant};

use async_trait::async_trait;
use codegg_core::decision::{
    BackendCapabilities, BackendState, DecisionEngine, DecisionError, DecisionRequest,
    DecisionResponse,
};

const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024;

pub struct SdmDecisionEngine {
    runtime: Arc<sdm_runtime::Runtime>,
}

impl SdmDecisionEngine {
    pub fn load(path: &Path, expected_digest: Option<&str>) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|_| "local decision artifact is unavailable".to_string())?;
        let size = file
            .metadata()
            .map_err(|_| "local decision artifact metadata is unavailable".to_string())?
            .len();
        if size == 0 || size > MAX_ARTIFACT_BYTES {
            return Err("local decision artifact size is outside the supported bound".into());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        file.take(MAX_ARTIFACT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "local decision artifact cannot be read".to_string())?;
        if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err("local decision artifact exceeds the supported bound".into());
        }
        let engine = Self::from_bytes(&bytes)?;
        if expected_digest.is_some_and(|expected| expected != engine.digest()) {
            return Err("local decision artifact digest does not match configured identity".into());
        }
        Ok(engine)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err("local decision artifact size is outside the supported bound".into());
        }
        let runtime = sdm_runtime::Runtime::from_bytes(bytes).map_err(|_| {
            "local decision artifact failed manifest or integrity validation".to_string()
        })?;
        Ok(Self {
            runtime: Arc::new(runtime),
        })
    }

    pub fn digest(&self) -> &str {
        self.runtime.digest()
    }
}

#[async_trait]
impl DecisionEngine for SdmDecisionEngine {
    fn state(&self) -> BackendState {
        BackendState::Ready
    }

    fn capabilities(&self) -> BackendCapabilities {
        let rank = self
            .runtime
            .artifact()
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == "rank");
        BackendCapabilities {
            binary: false,
            choice: false,
            score: false,
            rank,
            max_options: 0,
            max_candidates: if rank {
                self.runtime.artifact().manifest.max_candidates
            } else {
                0
            },
        }
    }

    async fn decide(
        &self,
        request: DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        request.validate()?;
        if Instant::now() >= deadline {
            return Err(DecisionError::InvalidRequest(
                "local decision deadline expired".into(),
            ));
        }
        // Both crates implement the same versioned M001 JSON contract. Convert at
        // this explicit repository boundary and validate on both sides.
        let external_request: sdm_core::DecisionRequest =
            serde_json::from_value(serde_json::to_value(&request).map_err(|_| {
                DecisionError::InvalidRequest("decision request cannot be encoded".into())
            })?)
            .map_err(|_| {
                DecisionError::InvalidRequest("decision request is incompatible with SDM".into())
            })?;
        let external_response = self.runtime.decide(&external_request).map_err(|_| {
            DecisionError::InvalidResponse("SDM runtime rejected the bounded request".into())
        })?;
        serde_json::from_value(
            serde_json::to_value(external_response).map_err(|_| {
                DecisionError::InvalidResponse("SDM response cannot be encoded".into())
            })?,
        )
        .map_err(|_| {
            DecisionError::InvalidResponse("SDM response is incompatible with M001".into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::decision::{
        DecisionAnswer, DecisionCandidate, DecisionSpec, DecisionStatus, StateField,
        DECISION_SCHEMA_VERSION,
    };
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn artifact(bias: f64) -> Vec<u8> {
        let weights = BTreeMap::from([("overlap:read".to_string(), bias)]);
        let threshold: f64 = 0.5;
        let artifact = sdm_runtime::LinearArtifact {
            manifest: sdm_runtime::ArtifactManifest {
                schema_version: 1,
                architecture: "sdm-binary-linear-v1".into(),
                input_schema_version: 1,
                capabilities: vec!["rank".into()],
                max_state_bytes: 65_536,
                max_candidates: 256,
                model_sha256: sdm_runtime::parameter_digest(0.0, &weights),
                training_fingerprint: "test-dataset".into(),
                calibration_version: 1,
                calibration_sha256: format!(
                    "sha256:{:x}",
                    Sha256::digest(threshold.to_bits().to_le_bytes())
                ),
                license: "MIT".into(),
                source: "unit-test".into(),
            },
            bias: 0.0,
            weights,
            threshold,
        };
        serde_json::to_vec(&artifact).unwrap()
    }

    fn request(kind: DecisionSpec) -> DecisionRequest {
        DecisionRequest {
            request_id: "local-test".into(),
            schema_version: DECISION_SCHEMA_VERSION,
            state: vec![StateField {
                key: "context".into(),
                value: "read file".into(),
            }],
            spec: kind,
        }
    }

    #[tokio::test]
    async fn local_rank_backend_returns_only_caller_candidates_and_exact_provenance() {
        let engine = SdmDecisionEngine::from_bytes(&artifact(1.0)).unwrap();
        let input = request(DecisionSpec::Rank {
            instruction: "rank".into(),
            multi_relevance: true,
            candidates: vec![
                DecisionCandidate {
                    id: "read".into(),
                    label: "read".into(),
                    description: "read file".into(),
                },
                DecisionCandidate {
                    id: "git".into(),
                    label: "git".into(),
                    description: "history".into(),
                },
            ],
        });
        let response = engine
            .decide(input.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        response.validate_for(&input).unwrap();
        assert!(matches!(
            response.status,
            codegg_core::decision::DecisionStatus::Answered
        ));
        let Some(DecisionAnswer::Rank { candidates }) = response.answer else {
            panic!("rank answer")
        };
        assert_eq!(
            candidates
                .iter()
                .map(|c| c.candidate_id.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            ["git", "read"].into_iter().collect()
        );
        assert_eq!(response.provenance.model.as_deref(), Some(engine.digest()));
        assert!(engine.capabilities().rank);
    }

    #[tokio::test]
    async fn unsupported_semantics_and_expired_deadlines_fail_explicitly() {
        let engine = SdmDecisionEngine::from_bytes(&artifact(1.0)).unwrap();
        let input = request(DecisionSpec::Binary {
            question: "yes?".into(),
        });
        let response = engine
            .decide(input.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(
            response.status,
            DecisionStatus::Unsupported { .. }
        ));
        let expired = request(DecisionSpec::Rank {
            instruction: "rank".into(),
            multi_relevance: true,
            candidates: vec![DecisionCandidate {
                id: "read".into(),
                label: "read".into(),
                description: "read".into(),
            }],
        });
        assert!(engine.decide(expired, Instant::now()).await.is_err());
    }

    #[tokio::test]
    async fn loaded_engine_is_an_immutable_snapshot_across_file_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.json");
        let first = artifact(1.0);
        std::fs::write(&path, &first).unwrap();
        let old = SdmDecisionEngine::load(&path, None).unwrap();
        let old_digest = old.digest().to_string();
        let second = artifact(2.0);
        std::fs::write(&path, &second).unwrap();
        let new = SdmDecisionEngine::load(&path, None).unwrap();
        assert_ne!(old.digest(), new.digest());
        assert_eq!(old.digest(), old_digest);
        let input = request(DecisionSpec::Rank {
            instruction: "rank".into(),
            multi_relevance: true,
            candidates: vec![DecisionCandidate {
                id: "read".into(),
                label: "read".into(),
                description: "read".into(),
            }],
        });
        let response = old
            .decide(input, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            response.provenance.model.as_deref(),
            Some(old_digest.as_str())
        );
    }

    #[test]
    fn tamper_and_expected_digest_mismatch_reject_before_ready() {
        let mut bytes = artifact(1.0);
        bytes[10] ^= 1;
        assert!(SdmDecisionEngine::from_bytes(&bytes).is_err());
        let bytes = artifact(1.0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.json");
        std::fs::write(&path, &bytes).unwrap();
        assert!(SdmDecisionEngine::load(&path, Some("sha256:wrong")).is_err());
    }
}
