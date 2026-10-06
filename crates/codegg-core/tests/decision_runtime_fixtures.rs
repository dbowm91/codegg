use std::{collections::BTreeMap, fs};

use codegg_core::decision::{DecisionRequest, DecisionResponse};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Fixture {
    fixture_schema_version: u16,
    fixture_id: String,
    source_case_id: Option<String>,
    source_sha256: Option<String>,
    request: DecisionRequest,
    response: Option<DecisionResponse>,
    #[serde(default = "default_valid")]
    expect_request_valid: bool,
}

fn default_valid() -> bool {
    true
}

#[test]
fn frozen_decision_fixtures_match_source_cases_and_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture_path = root.join("assets/decision-runtime/compatibility-v1.jsonl");
    let source_path = root.join("assets/tool-advisor/corpus.jsonl");
    let source_lines: BTreeMap<String, String> = fs::read_to_string(source_path)
        .expect("frozen advisor corpus")
        .lines()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            (
                value["case_id"].as_str().unwrap().to_owned(),
                line.to_owned(),
            )
        })
        .collect();

    let fixtures: Vec<Fixture> = fs::read_to_string(fixture_path)
        .expect("decision fixtures")
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid fixture JSON"))
        .collect();
    assert!(
        fixtures.len() >= 10,
        "fixtures cover semantic families, status outcomes, source cases, and bounds"
    );

    let mut fixture_ids = std::collections::BTreeSet::new();
    let mut fingerprints = BTreeMap::new();
    let mut source_count = 0;
    for fixture in fixtures {
        assert_eq!(fixture.fixture_schema_version, 1);
        assert!(
            fixture_ids.insert(fixture.fixture_id.clone()),
            "duplicate fixture id"
        );
        if let Some(source_case_id) = fixture.source_case_id {
            source_count += 1;
            assert_eq!(fixture.fixture_id, source_case_id);
            let source = source_lines
                .get(&source_case_id)
                .expect("fixture source case exists");
            assert_eq!(
                fixture.source_sha256.as_deref().unwrap(),
                format!("{:x}", Sha256::digest(source.as_bytes())),
                "source fingerprint changed for {}",
                fixture.fixture_id
            );
        } else {
            assert!(fixture.source_sha256.is_none());
        }
        let validation = fixture.request.validate();
        assert_eq!(
            validation.is_ok(),
            fixture.expect_request_valid,
            "{}",
            fixture.fixture_id
        );
        if fixture.expect_request_valid {
            let response = fixture
                .response
                .expect("valid request fixture has response");
            response
                .validate_for(&fixture.request)
                .expect("response matches request");
            fingerprints.insert(
                fixture.fixture_id.clone(),
                fixture.request.canonical_fingerprint().unwrap(),
            );
        } else {
            assert!(
                fixture.response.is_none(),
                "invalid fixture must not have a response"
            );
        }
    }
    assert_eq!(source_count, 4, "four frozen advisor cases are represented");
    assert_eq!(
        fingerprints["contract-rank-synthetic"],
        fingerprints["contract-rank-order-permutation"]
    );

    let fixture_path = root.join("assets/decision-runtime/compatibility-v1.jsonl");
    let ids: std::collections::BTreeSet<String> = fs::read_to_string(fixture_path)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["fixture_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    for required in [
        "contract-binary",
        "contract-choice",
        "contract-score",
        "contract-rank-synthetic",
        "contract-unsupported",
        "contract-unavailable",
        "contract-oversized-rejection",
    ] {
        assert!(ids.contains(required), "missing fixture {required}");
    }
}
