//! Ordinary HTTP client construction policy.
//!
//! Eggfetch owns transport mechanics; CodeGG owns the decision to follow
//! ordinary redirects. Many root callers repeat the same bounded redirect
//! contract with an owner-specific timeout. This seam owns only that
//! repeated policy and returns Eggfetch's public builder directly so
//! callers retain control of user agent, headers, and auth.
//!
//! Security-sensitive pinned/no-follow callers (WebFetch, direct URL
//! research, remote MCP), provider `create_http_client()`, and the EggLSP
//! downloader stay independent and must not use this helper.

/// Build an ordinary redirect-following client builder with the caller's
/// timeout.
///
/// The shared contract is `follow_redirects(true)` bounded to ten hops.
/// No retry policy, endpoint, auth, parsing, or response logic lives here.
pub(crate) fn ordinary_http_client_builder(
    timeout: eggfetch_core::Timeout,
) -> eggfetch_core::ClientBuilder {
    eggfetch_core::Client::builder()
        .timeout(timeout)
        .follow_redirects(true)
        .max_redirects(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    fn read_http_request(stream: &mut TcpStream) {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).expect("read HTTP request");
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
    }

    fn redirect_server(redirects: usize) -> (String, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        listener
            .set_nonblocking(true)
            .expect("set redirect server nonblocking");
        let address = listener.local_addr().expect("redirect server address");
        let handle = thread::spawn(move || {
            let mut requests = 0;
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while requests <= redirects && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                read_http_request(&mut stream);
                let response = if requests < redirects {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: /hop/{}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        requests + 1
                    )
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                        .to_string()
                };
                stream
                    .write_all(response.as_bytes())
                    .expect("write redirect response");
                requests += 1;
            }
            requests
        });
        (format!("http://{address}/start"), handle)
    }

    #[tokio::test]
    async fn ordinary_builder_follows_redirects_and_enforces_ten_hop_bound() {
        let (url, server) = redirect_server(2);
        let client = ordinary_http_client_builder(eggfetch_core::Timeout::from_secs(10)).build();
        let mut response = client
            .get(&url)
            .expect("build request")
            .send()
            .await
            .expect("redirect request succeeds");
        assert!(response.status().is_success());
        assert_eq!(response.text().await.expect("read response"), "ok");
        assert_eq!(server.join().expect("join redirect server"), 3);

        let (url, server) = redirect_server(11);
        let client = ordinary_http_client_builder(eggfetch_core::Timeout::from_secs(10)).build();
        let error = client
            .get(&url)
            .expect("build request")
            .send()
            .await
            .expect_err("eleven redirects exceed the ten-hop bound");
        assert!(matches!(
            error,
            eggfetch_core::Error::TooManyRedirects { max: 10, .. }
        ));
        assert_eq!(server.join().expect("join bounded redirect server"), 11);
    }

    #[test]
    fn ordinary_builder_accepts_distinct_owner_timeouts() {
        // The timeout remains caller-supplied: distinct owners pass distinct
        // values and each builds a usable client. Redirect policy is proven
        // by `ordinary_builder_follows_redirects_and_enforces_ten_hop_bound`.
        let ten = ordinary_http_client_builder(eggfetch_core::Timeout::from_secs(10)).build();
        let long = ordinary_http_client_builder(eggfetch_core::Timeout::from_secs(120)).build();
        // Clients build without sharing hidden state; a request can be
        // constructed from each.
        assert!(ten.get("http://127.0.0.1:1/").is_ok());
        assert!(long.get("http://127.0.0.1:1/").is_ok());
    }
}
