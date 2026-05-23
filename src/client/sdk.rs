use std::time::Duration;

use reqwest::Client;

use crate::error::ClientError;

pub struct RemoteClient {
    base_url: String,
    http: Client,
}

impl RemoteClient {
    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self, ClientError> {
        let mut builder = Client::builder();
        if let Some(t) = token {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", t).parse().map_err(|e| {
                    ClientError::Connection(format!("invalid authorization header: {}", e))
                })?,
            );
            builder = builder.default_headers(headers);
        }
        let http = builder
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| ClientError::Connection(format!("failed to build client: {}", e)))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    pub async fn health(&self) -> Result<bool, ClientError> {
        let url = format!("{}/health", self.base_url);
        let resp = self
            .http
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        if resp.status().is_success() {
            Ok(true)
        } else {
            Err(ClientError::Unreachable(format!(
                "health check failed: {}",
                resp.status()
            )))
        }
    }
}
