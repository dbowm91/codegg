use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[derive(Clone)]
    pub(crate) struct RateLimiter {
        cache: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
        max_requests: usize,
        window: Duration,
    }

    impl RateLimiter {
        pub(crate) fn new(max_requests: usize, window: Duration) -> Self {
            Self {
                cache: Arc::new(RwLock::new(HashMap::new())),
                max_requests,
                window,
            }
        }

        pub(crate) async fn check_rate_limit(&self, key: &str) -> bool {
            let now = Instant::now();
            let mut cache = self.cache.write().await;
            let requests = cache.entry(key.to_string()).or_insert_with(Vec::new);

            requests.retain(|&t| now.duration_since(t) < self.window);

            if requests.len() >= self.max_requests {
                return false;
            }

            requests.push(now);
            true
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(10, Duration::from_secs(60));

        for i in 0..5 {
            let result = limiter.check_rate_limit(&format!("key_{}", i)).await;
            assert!(result, "request {} should be allowed", i);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_at_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        let key = "test_key";

        assert!(limiter.check_rate_limit(key).await);
        assert!(limiter.check_rate_limit(key).await);
        assert!(limiter.check_rate_limit(key).await);
        assert!(!limiter.check_rate_limit(key).await);
    }

    #[tokio::test]
    async fn test_rate_limiter_different_keys() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));

        assert!(limiter.check_rate_limit("key_a").await);
        assert!(limiter.check_rate_limit("key_a").await);
        assert!(!limiter.check_rate_limit("key_a").await);

        assert!(limiter.check_rate_limit("key_b").await);
        assert!(limiter.check_rate_limit("key_b").await);
        assert!(!limiter.check_rate_limit("key_b").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_window_expiry() {
        // Use a small window so the wait is short (a few hundred ms
        // rather than the previous 2s).
        let limiter = RateLimiter::new(2, Duration::from_millis(100));
        let key = "expiry_key";

        assert!(limiter.check_rate_limit(key).await);
        assert!(limiter.check_rate_limit(key).await);
        assert!(!limiter.check_rate_limit(key).await);

        // Wait just past the window. The retain() inside
        // check_rate_limit drops the expired requests, freeing
        // capacity.
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!(limiter.check_rate_limit(key).await);
    }
}
