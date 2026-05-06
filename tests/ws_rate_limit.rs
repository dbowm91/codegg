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
        pub(crate) fn new(max_requests: usize, window_secs: u64) -> Self {
            Self {
                cache: Arc::new(RwLock::new(HashMap::new())),
                max_requests,
                window: Duration::from_secs(window_secs),
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
        let limiter = RateLimiter::new(10, 60);

        for i in 0..5 {
            let result = limiter.check_rate_limit(&format!("key_{}", i)).await;
            assert!(result, "request {} should be allowed", i);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_at_limit() {
        let limiter = RateLimiter::new(3, 60);
        let key = "test_key";

        assert!(limiter.check_rate_limit(key).await);
        assert!(limiter.check_rate_limit(key).await);
        assert!(limiter.check_rate_limit(key).await);
        assert!(!limiter.check_rate_limit(key).await);
    }

    #[tokio::test]
    async fn test_rate_limiter_different_keys() {
        let limiter = RateLimiter::new(2, 60);

        assert!(limiter.check_rate_limit("key_a").await);
        assert!(limiter.check_rate_limit("key_a").await);
        assert!(!limiter.check_rate_limit("key_a").await);

        assert!(limiter.check_rate_limit("key_b").await);
        assert!(limiter.check_rate_limit("key_b").await);
        assert!(!limiter.check_rate_limit("key_b").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_window_expiry() {
        let limiter = RateLimiter::new(2, 1);

        assert!(limiter.check_rate_limit("key").await);
        assert!(limiter.check_rate_limit("key").await);
        assert!(!limiter.check_rate_limit("key").await);

        tokio::time::sleep(Duration::from_secs(2)).await;

        assert!(limiter.check_rate_limit("key").await);
    }
}
