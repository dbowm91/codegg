use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::error::{ResearchError, Result};
use super::types::*;

pub struct ResearchStore {
    root: PathBuf,
}

impl ResearchStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    async fn run_dir(&self, run_id: &str) -> PathBuf {
        self.root.join(run_id)
    }

    pub async fn create_run(&self, request: &ResearchRequest) -> Result<ResearchRunStatus> {
        let run_id = &request.id;
        let dir = self.run_dir(run_id).await;
        fs::create_dir_all(&dir).await?;

        let request_path = dir.join("request.json");
        let data = serde_json::to_string_pretty(request)?;
        fs::write(&request_path, data).await?;

        let status = ResearchRunStatus {
            run_id: run_id.clone(),
            status: RunStatus::Planning,
            started_at: chrono::Utc::now(),
            finished_at: None,
            artifact_dir: dir.clone(),
            error: None,
            counts: ResearchRunCounts::default(),
        };

        let status_path = dir.join("run.json");
        let data = serde_json::to_string_pretty(&status)?;
        fs::write(&status_path, data).await?;

        Ok(status)
    }

    pub async fn update_run_status(&self, run_id: &str, status: ResearchRunStatus) -> Result<()> {
        let dir = self.run_dir(run_id).await;
        let status_path = dir.join("run.json");
        let data = serde_json::to_string_pretty(&status)?;
        fs::write(&status_path, data).await?;
        Ok(())
    }

    pub async fn append_source(&self, source: &SourceRecord) -> Result<()> {
        let dir = self.run_dir(&source.run_id).await;
        let path = dir.join("sources.jsonl");
        append_jsonl(&path, source).await
    }

    pub async fn append_evidence(&self, evidence: &EvidenceSpan) -> Result<()> {
        let dir = self.run_dir(&evidence.run_id).await;
        let path = dir.join("evidence.jsonl");
        append_jsonl(&path, evidence).await
    }

    pub async fn append_claim(&self, claim: &ClaimRecord) -> Result<()> {
        let dir = self.run_dir(&claim.run_id).await;
        let path = dir.join("claims.jsonl");
        append_jsonl(&path, claim).await
    }

    pub async fn append_contradiction(&self, contradiction: &ContradictionRecord) -> Result<()> {
        let dir = self.run_dir(&contradiction.run_id).await;
        let path = dir.join("contradictions.jsonl");
        append_jsonl(&path, contradiction).await
    }

    pub async fn write_plan(&self, run_id: &str, plan: &ResearchPlan) -> Result<()> {
        let dir = self.run_dir(run_id).await;
        let path = dir.join("plan.json");
        let data = serde_json::to_string_pretty(plan)?;
        fs::write(&path, data).await?;
        Ok(())
    }

    pub async fn write_report(
        &self,
        run_id: &str,
        profile: &ResearchOutputProfile,
        text: &str,
    ) -> Result<PathBuf> {
        let dir = self.run_dir(run_id).await;
        let filename = match profile {
            ResearchOutputProfile::HumanFullReport => "report.md",
            ResearchOutputProfile::HumanBrief => "brief.md",
            ResearchOutputProfile::AgentAnswer => "agent-answer.md",
            ResearchOutputProfile::AgentHandoff => "handoff.ctx.md",
            ResearchOutputProfile::EvidenceBundle => "evidence-bundle.json",
        };
        let path = dir.join(filename);
        fs::write(&path, text).await?;
        Ok(path)
    }

    pub async fn load_run_bundle(&self, run_id: &str) -> Result<ResearchBundle> {
        let dir = self.run_dir(run_id).await;

        let request_path = dir.join("request.json");
        if !request_path.exists() {
            return Err(ResearchError::RunNotFound(run_id.to_string()));
        }
        let request_data = fs::read_to_string(&request_path).await?;
        let request: ResearchRequest = serde_json::from_str(&request_data)?;

        let run_path = dir.join("run.json");
        let run_data = fs::read_to_string(&run_path).await?;
        let status: ResearchRunStatus = serde_json::from_str(&run_data)?;

        let plan_path = dir.join("plan.json");
        let plan = if plan_path.exists() {
            let plan_data = fs::read_to_string(&plan_path).await?;
            Some(serde_json::from_str(&plan_data)?)
        } else {
            None
        };

        let sources = read_jsonl(&dir.join("sources.jsonl")).await?;
        let evidence = read_jsonl(&dir.join("evidence.jsonl")).await?;
        let claims = read_jsonl(&dir.join("claims.jsonl")).await?;
        let contradictions = read_jsonl(&dir.join("contradictions.jsonl")).await?;

        Ok(ResearchBundle {
            request,
            status,
            plan,
            sources,
            evidence,
            claims,
            contradictions,
        })
    }

    pub async fn list_runs(&self) -> Result<Vec<ResearchRunStatus>> {
        if !self.root.exists() {
            return Ok(vec![]);
        }

        let mut entries = fs::read_dir(&self.root).await?;
        let mut statuses = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }

            let run_path = entry.path().join("run.json");
            if run_path.exists() {
                let data = fs::read_to_string(&run_path).await?;
                if let Ok(status) = serde_json::from_str::<ResearchRunStatus>(&data) {
                    statuses.push(status);
                }
            }
        }

        statuses.sort_by_key(|s| std::cmp::Reverse(s.started_at));
        Ok(statuses)
    }

    pub async fn load_run_status(&self, run_id: &str) -> Result<ResearchRunStatus> {
        let dir = self.run_dir(run_id).await;
        let path = dir.join("run.json");
        if !path.exists() {
            return Err(ResearchError::RunNotFound(run_id.to_string()));
        }
        let data = fs::read_to_string(&path).await?;
        let status: ResearchRunStatus = serde_json::from_str(&data)?;
        Ok(status)
    }
}

async fn append_jsonl(path: &Path, record: &impl serde::Serialize) -> Result<()> {
    let mut line = serde_json::to_string(record)?;
    line.push('\n');
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .truncate(false)
        .open(path)
        .await?
        .write_all(line.as_bytes())
        .await?;
    Ok(())
}

async fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(path).await?;
    let mut records = Vec::new();
    for line in content.lines() {
        if !line.trim().is_empty() {
            records.push(serde_json::from_str(line)?);
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_request(id: &str) -> ResearchRequest {
        ResearchRequest {
            id: id.to_string(),
            question: "test question".to_string(),
            mode: ResearchMode::NarrowAnswer,
            audience: ResearchAudience::Human,
            depth: ResearchDepth::Low,
            output_profiles: vec![ResearchOutputProfile::HumanFullReport],
            constraints: vec![],
            sources: vec![],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources: 8,
                max_chunks_per_source: 5,
                max_evidence_spans: 10,
                max_model_calls: 0,
                max_output_tokens: None,
                allow_network: false,
            },
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn store_creates_run_directory() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let request = make_request("test-run-1");
        let status = store.create_run(&request).await.unwrap();
        assert_eq!(status.run_id, "test-run-1");
        assert_eq!(status.status, RunStatus::Planning);
        assert!(tmp.path().join("test-run-1").exists());
        assert!(tmp.path().join("test-run-1/request.json").exists());
        assert!(tmp.path().join("test-run-1/run.json").exists());
    }

    #[tokio::test]
    async fn store_appends_and_reads_sources() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let request = make_request("run-src");
        store.create_run(&request).await.unwrap();

        let source = SourceRecord {
            id: "src-1".to_string(),
            run_id: "run-src".to_string(),
            uri: "test.rs".to_string(),
            title: Some("test.rs".to_string()),
            source_type: SourceType::LocalFile,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::FileRange {
                path: PathBuf::from("test.rs"),
                start_line: 1,
                end_line: 10,
            },
            notes: vec![],
        };
        store.append_source(&source).await.unwrap();

        let sources: Vec<SourceRecord> =
            read_jsonl(&tmp.path().join("run-src/sources.jsonl")).await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, "src-1");
    }

    #[tokio::test]
    async fn store_appends_and_reads_evidence() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let request = make_request("run-ev");
        store.create_run(&request).await.unwrap();

        let evidence = EvidenceSpan {
            id: "ev-1".to_string(),
            run_id: "run-ev".to_string(),
            source_id: "src-1".to_string(),
            locator: SourceLocator::TextSpan {
                label: "test".to_string(),
            },
            text: "some evidence".to_string(),
            summary: None,
            extracted_at: Utc::now(),
        };
        store.append_evidence(&evidence).await.unwrap();

        let evs: Vec<EvidenceSpan> =
            read_jsonl(&tmp.path().join("run-ev/evidence.jsonl")).await.unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].text, "some evidence");
    }

    #[tokio::test]
    async fn store_appends_and_reads_claims() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let request = make_request("run-cl");
        store.create_run(&request).await.unwrap();

        let claim = ClaimRecord {
            id: "cl-1".to_string(),
            run_id: "run-cl".to_string(),
            text: "a claim".to_string(),
            claim_type: ClaimType::Fact,
            confidence: Confidence::High,
            evidence_ids: vec!["ev-1".to_string()],
            caveats: vec![],
            applies_to: vec![],
        };
        store.append_claim(&claim).await.unwrap();

        let claims: Vec<ClaimRecord> =
            read_jsonl(&tmp.path().join("run-cl/claims.jsonl")).await.unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].text, "a claim");
    }

    #[tokio::test]
    async fn store_writes_report() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let request = make_request("run-rpt");
        store.create_run(&request).await.unwrap();

        let path = store
            .write_report("run-rpt", &ResearchOutputProfile::HumanFullReport, "# Report\nHello")
            .await
            .unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "report.md");

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Hello"));
    }

    #[tokio::test]
    async fn store_list_runs() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        store.create_run(&make_request("run-a")).await.unwrap();
        store.create_run(&make_request("run-b")).await.unwrap();

        let runs = store.list_runs().await.unwrap();
        assert_eq!(runs.len(), 2);
    }

    #[tokio::test]
    async fn store_load_run_bundle() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let request = make_request("run-bundle");
        store.create_run(&request).await.unwrap();

        let source = SourceRecord {
            id: "src-1".to_string(),
            run_id: "run-bundle".to_string(),
            uri: "test.rs".to_string(),
            title: None,
            source_type: SourceType::LocalFile,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::TextSpan {
                label: "root".to_string(),
            },
            notes: vec![],
        };
        store.append_source(&source).await.unwrap();

        let bundle = store.load_run_bundle("run-bundle").await.unwrap();
        assert_eq!(bundle.request.id, "run-bundle");
        assert_eq!(bundle.sources.len(), 1);
    }

    #[tokio::test]
    async fn store_load_nonexistent_run() {
        let tmp = TempDir::new().unwrap();
        let store = ResearchStore::new(tmp.path().to_path_buf());
        let result = store.load_run_status("nonexistent").await;
        assert!(result.is_err());
    }
}
