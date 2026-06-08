use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    full_name: String,
    description: Option<String>,
    html_url: String,
    stargazers_count: Option<u64>,
    forks_count: Option<u64>,
    open_issues_count: Option<u64>,
    language: Option<String>,
    license: Option<GitHubLicense>,
    updated_at: Option<String>,
    pushed_at: Option<String>,
    topics: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct GitHubLicense {
    spdx_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubFile {
    name: String,
    path: String,
    size: Option<u64>,
    content: Option<String>,
    encoding: Option<String>,
    download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubIssue {
    number: u64,
    title: String,
    state: Option<String>,
    body: Option<String>,
    user: Option<GitHubUser>,
    created_at: Option<String>,
    updated_at: Option<String>,
    labels: Option<Vec<GitHubLabel>>,
    comments: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubUser {
    login: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubLabel {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRateLimit {
    resources: Option<GitHubRateResources>,
}

#[derive(Debug, Deserialize)]
struct GitHubRateResources {
    core: Option<GitHubRate>,
}

#[derive(Debug, Deserialize)]
struct GitHubRate {
    remaining: Option<u64>,
}

struct GitHubParsedUrl {
    owner: String,
    repo: String,
    path: Option<String>,
    issue_number: Option<u64>,
}

pub struct GitHubSource {
    client: reqwest::Client,
}

impl GitHubSource {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            .user_agent("codegg-research")
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    fn parse_github_url(url: &str) -> Option<GitHubParsedUrl> {
        let url = url.trim_end_matches('/');
        let prefix = "https://github.com/";
        let rest = url.strip_prefix(prefix)?;

        let segments: Vec<&str> = rest.split('/').collect();
        if segments.len() < 2 {
            return None;
        }

        let owner = segments[0].to_string();
        let repo = segments[1].to_string();

        if segments.len() >= 4 && segments[2] == "blob" {
            let path = segments[3..].join("/");
            Some(GitHubParsedUrl {
                owner,
                repo,
                path: Some(path),
                issue_number: None,
            })
        } else if segments.len() >= 4 && segments[2] == "issues" {
            let issue_number = segments[3].parse::<u64>().ok();
            Some(GitHubParsedUrl {
                owner,
                repo,
                path: None,
                issue_number,
            })
        } else if segments.len() == 2 {
            Some(GitHubParsedUrl {
                owner,
                repo,
                path: None,
                issue_number: None,
            })
        } else {
            Some(GitHubParsedUrl {
                owner,
                repo,
                path: None,
                issue_number: None,
            })
        }
    }

    fn extract_github_refs(question: &str, plan: &ResearchPlan) -> Vec<String> {
        let mut urls = Vec::new();

        // Check plan source_classes for github references
        for cls in &plan.source_classes {
            if cls.contains("github.com") {
                urls.push(cls.clone());
            }
        }

        // Extract github URLs from the question
        for word in question.split_whitespace() {
            let w = word
                .trim_matches(|c: char| c == '<' || c == '>' || c == '\'' || c == '"' || c == '`');
            if w.starts_with("https://github.com/") || w.starts_with("github.com/") {
                let url = if w.starts_with("https://") {
                    w.to_string()
                } else {
                    format!("https://{}", w)
                };
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }

        urls
    }

    fn extract_repo_name(question: &str) -> Option<(String, String)> {
        // Try to find "owner/repo" pattern
        for word in question.split_whitespace() {
            let w = word.trim_matches(|c: char| c.is_ascii_punctuation());
            let parts: Vec<&str> = w.split('/').collect();
            if parts.len() == 2
                && !parts[0].is_empty()
                && !parts[1].is_empty()
                && parts[0]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && parts[1]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Some((parts[0].to_string(), parts[1].to_string()));
            }
        }
        None
    }

    async fn check_rate_limit(&self) -> Result<u64> {
        let response = self
            .client
            .get("https://api.github.com/rate_limit")
            .header("User-Agent", "codegg-research")
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("rate limit check failed: {e}")))?;

        if response.status().is_success() {
            let resp: GitHubRateLimit = response
                .json()
                .await
                .map_err(|e| ResearchError::UrlFetch(format!("rate limit parse failed: {e}")))?;
            return Ok(resp
                .resources
                .and_then(|r| r.core)
                .and_then(|c| c.remaining)
                .unwrap_or(0));
        }

        // If rate limit check fails, return 0 to trigger skip
        Ok(0)
    }

    async fn fetch_repo_metadata(&self, owner: &str, repo: &str) -> Result<SourceRecord> {
        let url = format!("https://api.github.com/repos/{}/{}", owner, repo);

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "codegg-research")
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("request failed: {e}")))?;

        // Check rate limit remaining from headers
        if let Some(remaining) = response.headers().get("x-ratelimit-remaining") {
            if let Ok(val) = remaining.to_str() {
                if val == "0" {
                    return Err(ResearchError::UrlFetch(
                        "GitHub API rate limit exceeded".to_string(),
                    ));
                }
            }
        }

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("HTTP {status} for {url}")));
        }

        let gh_repo: GitHubRepo = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("failed to parse JSON: {e}")))?;

        let mut notes = Vec::new();
        if let Some(ref desc) = gh_repo.description {
            notes.push(format!("description: {}", desc));
        }
        if let Some(stars) = gh_repo.stargazers_count {
            notes.push(format!("stars: {}", stars));
        }
        if let Some(forks) = gh_repo.forks_count {
            notes.push(format!("forks: {}", forks));
        }
        if let Some(issues) = gh_repo.open_issues_count {
            notes.push(format!("open_issues: {}", issues));
        }
        if let Some(ref lang) = gh_repo.language {
            notes.push(format!("language: {}", lang));
        }
        if let Some(ref lic) = gh_repo.license {
            if let Some(ref spdx) = lic.spdx_id {
                notes.push(format!("license: {}", spdx));
            }
        }
        if let Some(ref topics) = gh_repo.topics {
            if !topics.is_empty() {
                notes.push(format!("topics: {}", topics.join(", ")));
            }
        }
        if let Some(ref updated) = gh_repo.updated_at {
            notes.push(format!("updated_at: {}", updated));
        }
        if let Some(ref pushed) = gh_repo.pushed_at {
            notes.push(format!("pushed_at: {}", pushed));
        }

        let published_at = gh_repo
            .pushed_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: gh_repo.html_url.clone(),
            title: Some(format!(
                "{} - {}",
                gh_repo.full_name,
                gh_repo.description.as_deref().unwrap_or("")
            )),
            source_type: SourceType::GitHubFile,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at,
            content_hash: None,
            locator: SourceLocator::Url {
                url: gh_repo.html_url,
                heading: None,
            },
            notes,
        })
    }

    async fn fetch_file_content(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> Result<SourceRecord> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/{}",
            owner, repo, path
        );

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "codegg-research")
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("request failed: {e}")))?;

        if let Some(remaining) = response.headers().get("x-ratelimit-remaining") {
            if let Ok(val) = remaining.to_str() {
                if val == "0" {
                    return Err(ResearchError::UrlFetch(
                        "GitHub API rate limit exceeded".to_string(),
                    ));
                }
            }
        }

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("HTTP {status} for {url}")));
        }

        let file: GitHubFile = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("failed to parse JSON: {e}")))?;

        let content = file
            .content
            .and_then(|c| {
                if file.encoding.as_deref() == Some("base64") {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(c.replace('\n', ""))
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                } else {
                    Some(c)
                }
            })
            .unwrap_or_default();

        let content_hash = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(content.as_bytes()))
        };

        let line_count = content.lines().count();

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: format!("https://github.com/{}/{}/blob/{}", owner, repo, path),
            title: Some(format!("{}/{}/{}", owner, repo, path)),
            source_type: SourceType::GitHubFile,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: Some(content_hash),
            locator: SourceLocator::FileRange {
                path: std::path::PathBuf::from(path),
                start_line: 1,
                end_line: line_count,
            },
            notes: vec![
                format!("file_size: {}", file.size.unwrap_or(0)),
                format!("lines: {}", line_count),
            ],
        })
    }

    async fn fetch_issue(&self, owner: &str, repo: &str, number: u64) -> Result<SourceRecord> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/issues/{}",
            owner, repo, number
        );

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "codegg-research")
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("request failed: {e}")))?;

        if let Some(remaining) = response.headers().get("x-ratelimit-remaining") {
            if let Ok(val) = remaining.to_str() {
                if val == "0" {
                    return Err(ResearchError::UrlFetch(
                        "GitHub API rate limit exceeded".to_string(),
                    ));
                }
            }
        }

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("HTTP {status} for {url}")));
        }

        let issue: GitHubIssue = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("failed to parse JSON: {e}")))?;

        let mut notes = Vec::new();
        if let Some(ref state) = issue.state {
            notes.push(format!("state: {}", state));
        }
        if let Some(ref user) = issue.user {
            if let Some(ref login) = user.login {
                notes.push(format!("author: {}", login));
            }
        }
        if let Some(ref labels) = issue.labels {
            let label_names: Vec<String> = labels.iter().filter_map(|l| l.name.clone()).collect();
            if !label_names.is_empty() {
                notes.push(format!("labels: {}", label_names.join(", ")));
            }
        }
        if let Some(comments) = issue.comments {
            notes.push(format!("comments: {}", comments));
        }
        if let Some(ref body) = issue.body {
            let truncated = if body.len() > 500 {
                format!("{}...", &body[..500])
            } else {
                body.clone()
            };
            notes.push(format!("body: {}", truncated));
        }

        let published_at = issue
            .created_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: format!("https://github.com/{}/{}/issues/{}", owner, repo, number),
            title: Some(format!("#{} - {}", issue.number, issue.title)),
            source_type: SourceType::GitHubIssue,
            source_quality: SourceQuality::MaintainerComment,
            retrieved_at: Utc::now(),
            published_at,
            content_hash: None,
            locator: SourceLocator::Url {
                url: format!("https://github.com/{}/{}/issues/{}", owner, repo, number),
                heading: None,
            },
            notes,
        })
    }
}

impl Default for GitHubSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchSourceAdapter for GitHubSource {
    fn name(&self) -> &'static str {
        "github"
    }

    fn collect<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceRecord>>> + Send + 'a>> {
        Box::pin(async move {
            if !request.budget.allow_network {
                return Err(ResearchError::NetworkNotAllowed);
            }

            // Check rate limit first
            let remaining = self.check_rate_limit().await.unwrap_or(0);
            if remaining == 0 {
                eprintln!("Warning: GitHub API rate limit exhausted, skipping GitHub sources");
                return Ok(Vec::new());
            }

            let mut sources = Vec::new();
            let mut seen_urls = std::collections::HashSet::new();

            // Collect from explicit URLs in the request
            let mut urls = Self::extract_github_refs(&request.question, plan);

            // Also check request.sources for github URLs
            for source_spec in &request.sources {
                if source_spec.spec_type == SourceSpecType::Url
                    && source_spec.value.contains("github.com")
                {
                    urls.push(source_spec.value.clone());
                }
            }

            for url in &urls {
                if seen_urls.contains(url) {
                    continue;
                }
                seen_urls.insert(url.clone());

                if let Some(parsed) = Self::parse_github_url(url) {
                    if parsed.issue_number.is_some() {
                        let issue_num = parsed.issue_number.unwrap();
                        match self
                            .fetch_issue(&parsed.owner, &parsed.repo, issue_num)
                            .await
                        {
                            Ok(source) => sources.push(source),
                            Err(e) => {
                                eprintln!("Warning: failed to fetch issue: {}", e);
                            }
                        }
                    } else if let Some(ref path) = parsed.path {
                        match self
                            .fetch_file_content(&parsed.owner, &parsed.repo, path)
                            .await
                        {
                            Ok(source) => sources.push(source),
                            Err(e) => {
                                eprintln!("Warning: failed to fetch file: {}", e);
                            }
                        }
                    } else {
                        match self.fetch_repo_metadata(&parsed.owner, &parsed.repo).await {
                            Ok(source) => sources.push(source),
                            Err(e) => {
                                eprintln!("Warning: failed to fetch repo metadata: {}", e);
                            }
                        }
                    }
                }
            }

            // If no explicit URLs, try to find owner/repo in the question
            if sources.is_empty() {
                if let Some((owner, repo)) = Self::extract_repo_name(&request.question) {
                    let url = format!("https://github.com/{}/{}", owner, repo);
                    if !seen_urls.contains(&url) {
                        match self.fetch_repo_metadata(&owner, &repo).await {
                            Ok(source) => sources.push(source),
                            Err(e) => {
                                eprintln!("Warning: failed to fetch repo metadata: {}", e);
                            }
                        }
                    }
                }
            }

            Ok(sources)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        let source = GitHubSource::new();
        assert_eq!(source.name(), "github");
    }

    #[test]
    fn test_parse_github_url_repo() {
        let parsed = GitHubSource::parse_github_url("https://github.com/rust-lang/rust");
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert_eq!(p.owner, "rust-lang");
        assert_eq!(p.repo, "rust");
        assert!(p.path.is_none());
        assert!(p.issue_number.is_none());
    }

    #[test]
    fn test_parse_github_url_file() {
        let parsed = GitHubSource::parse_github_url(
            "https://github.com/rust-lang/rust/blob/main/src/lib.rs",
        );
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert_eq!(p.owner, "rust-lang");
        assert_eq!(p.repo, "rust");
        assert_eq!(p.path.as_deref(), Some("main/src/lib.rs"));
    }

    #[test]
    fn test_parse_github_url_issue() {
        let parsed =
            GitHubSource::parse_github_url("https://github.com/rust-lang/rust/issues/12345");
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert_eq!(p.owner, "rust-lang");
        assert_eq!(p.repo, "rust");
        assert_eq!(p.issue_number, Some(12345));
    }

    #[test]
    fn test_parse_github_url_trailing_slash() {
        let parsed = GitHubSource::parse_github_url("https://github.com/rust-lang/rust/");
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert_eq!(p.owner, "rust-lang");
        assert_eq!(p.repo, "rust");
    }

    #[test]
    fn test_parse_github_url_not_github() {
        let parsed = GitHubSource::parse_github_url("https://gitlab.com/foo/bar");
        assert!(parsed.is_none());
    }

    #[test]
    fn test_extract_repo_name() {
        let name = GitHubSource::extract_repo_name("Tell me about tokio/tokio");
        let (owner, repo) = name.unwrap();
        assert_eq!(owner, "tokio");
        assert_eq!(repo, "tokio");
    }

    #[test]
    fn test_extract_github_refs_from_question() {
        let plan = ResearchPlan {
            scope: String::new(),
            comparison_axes: vec![],
            source_classes: vec![],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        };
        let refs = GitHubSource::extract_github_refs(
            "Check https://github.com/tokio/tokio for details",
            &plan,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], "https://github.com/tokio/tokio");
    }
}
