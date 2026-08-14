use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum BoundedBodyError {
    #[error("response body limit must be greater than zero")]
    InvalidLimit,
    #[error("response body exceeds {limit} byte limit ({observed} bytes)")]
    TooLarge { limit: usize, observed: usize },
    #[error("failed to read response body: {0}")]
    Stream(#[source] reqwest::Error),
}

/// Collect a response without allowing the retained body to exceed the hard
/// limit. The content length is only an early rejection; streamed chunks are
/// checked as well because the header may be absent or untrustworthy.
pub(crate) async fn read_body_bounded(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, BoundedBodyError> {
    if max_bytes == 0 {
        return Err(BoundedBodyError::InvalidLimit);
    }

    if let Some(content_length) = response.content_length() {
        let content_length = usize::try_from(content_length).unwrap_or(usize::MAX);
        if content_length > max_bytes {
            return Err(BoundedBodyError::TooLarge {
                limit: max_bytes,
                observed: content_length,
            });
        }
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .filter(|&length| length <= max_bytes)
            .unwrap_or(0),
    );
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BoundedBodyError::Stream)?;
        let remaining = max_bytes.saturating_sub(body.len());
        if chunk.len() > remaining {
            return Err(BoundedBodyError::TooLarge {
                limit: max_bytes,
                observed: body.len().saturating_add(chunk.len()),
            });
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn fixture(body: &[u8], headers: &str) -> reqwest::Response {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = Arc::new(body.to_vec());
        let headers = headers.to_string();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            let response = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{headers}\r\n");
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        });

        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{addr}/fixture"))
            .send()
            .await
            .unwrap()
    }

    async fn chunked_fixture(chunks: &[&[u8]]) -> reqwest::Response {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let chunks: Vec<Vec<u8>> = chunks.iter().map(|chunk| chunk.to_vec()).collect();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n",
                )
                .await
                .unwrap();
            for chunk in chunks {
                socket
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await
                    .unwrap();
                socket.write_all(&chunk).await.unwrap();
                socket.write_all(b"\r\n").await.unwrap();
            }
            socket.write_all(b"0\r\n\r\n").await.unwrap();
        });

        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{addr}/fixture"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_body_exactly_at_limit() {
        let response = fixture(b"12345", "Content-Length: 5\r\n").await;
        assert_eq!(read_body_bounded(response, 5).await.unwrap(), b"12345");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_declared_body_before_reading_it() {
        let response = fixture(b"123456", "Content-Length: 6\r\n").await;
        assert!(matches!(
            read_body_bounded(response, 5).await,
            Err(BoundedBodyError::TooLarge {
                limit: 5,
                observed: 6
            })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_chunked_body_when_next_chunk_crosses_limit() {
        let response = chunked_fixture(&[b"123", b"456"]).await;
        assert!(matches!(
            read_body_bounded(response, 5).await,
            Err(BoundedBodyError::TooLarge {
                limit: 5,
                observed: 6
            })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_chunked_body_under_limit() {
        let response = chunked_fixture(&[b"12", b"345"]).await;
        assert_eq!(read_body_bounded(response, 5).await.unwrap(), b"12345");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_zero_limit() {
        let response = fixture(b"1", "Content-Length: 1\r\n").await;
        assert!(matches!(
            read_body_bounded(response, 0).await,
            Err(BoundedBodyError::InvalidLimit)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pinned_address_ignores_later_dns_and_preserves_host_header() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = "post-validation-change.invalid";
        let (request_tx, request_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 256];
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") || read == 0 {
                    break;
                }
            }
            let _ = request_tx.send(request);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            // This is the same mechanism used with the production validated
            // address set. The .invalid name has no fallback DNS answer, so
            // a second resolver pass would fail instead of reaching the fixture.
            .resolve_to_addrs(host, &[addr])
            .build()
            .unwrap();
        let response = client
            .get(format!("http://{host}:{}/fixture", addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        let request = String::from_utf8(request_rx.await.unwrap())
            .unwrap()
            .to_ascii_lowercase();
        assert!(request.contains(&format!("host: {host}:{}", addr.port())));
    }
}
