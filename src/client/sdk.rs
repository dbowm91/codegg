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
            .build()
            .map_err(|e| ClientError::Connection(format!("failed to build client: {}", e)))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    pub async fn health(&self) -> Result<bool, ClientError> {
        let url = format!("{}/api/providers", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;
        Ok(resp.status().is_success())
    }
}
