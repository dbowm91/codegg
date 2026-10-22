use codegg::agent_convergence::{
    assemble_verifier_evidence, ConvergenceSpec, SemanticVerificationVerdict,
};
use codegg::identity::AgentRunId;
use codegg::run_result::{
    AgentRunArtifact, AgentRunFinding, AgentRunResult, AgentRunResultStatus, RepositoryState,
    Retryability,
};

#[test]
fn verifier_vertical_slice_contract_is_bounded_and_typed() {
    let spec = ConvergenceSpec::new(
        "deliver the requested change",
        vec!["host validation remains authoritative".into()],
    )
    .unwrap();
    let result = AgentRunResult {
        run_id: AgentRunId::new(),
        status: AgentRunResultStatus::Succeeded,
        summary: "producer result".into(),
        worktree_id: None,
        base_commit: Some("base".into()),
        result_commit: Some("result".into()),
        changed_paths: vec!["src/lib.rs".into()],
        validation: Vec::new(),
        findings: vec![AgentRunFinding {
            severity: "info".into(),
            title: "bounded finding".into(),
            rationale: "evidence-backed".into(),
            file: Some("src/lib.rs".into()),
            line: Some(1),
        }],
        artifacts: vec![AgentRunArtifact {
            kind: "result".into(),
            label: "structured result".into(),
            reference: None,
        }],
        repository_state: RepositoryState::Clean,
        retryability: Retryability::NotRetryable,
        recovery_hint: None,
    };
    let packet = assemble_verifier_evidence(&spec, &[result]).unwrap();
    assert!(packet.encode_bounded().unwrap().len() < 32 * 1024);
    assert!(matches!(
        SemanticVerificationVerdict::parse_marked(
            "<convergence_verdict>{\"kind\":\"inconclusive\",\"reason\":\"missing evidence\",\"missing_evidence\":[\"diff\"]}</convergence_verdict>"
        )
        .unwrap(),
        SemanticVerificationVerdict::Inconclusive { .. }
    ));
}
