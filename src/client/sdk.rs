use eggfetch_core::{Client, Timeout};

use crate::error::ClientError;

pub struct RemoteClient {
    base_url: String,
    http: Client,
}

impl RemoteClient {
    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self, ClientError> {
        let mut builder = crate::http_client::ordinary_http_client_builder(Timeout::from_secs(10));
        if let Some(t) = token {
            builder = builder
                .default_header("authorization", &format!("Bearer {t}"))
                .map_err(|e| {
                    ClientError::Connection(format!("invalid authorization header: {e}"))
                })?;
        }
        let http = builder.build();
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
            .map_err(|e| ClientError::Unreachable(e.to_string()))?
            .timeout(Timeout::from_secs(10))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_authorization_header_at_construction() {
        let result = RemoteClient::new("http://127.0.0.1:1", Some("bad\r\ntoken"));
        assert!(
            matches!(result, Err(ClientError::Connection(message)) if message.contains("authorization"))
        );
    }
}
