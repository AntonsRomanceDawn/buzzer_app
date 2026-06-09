//! Production rate limiting.
//!
//! Keys are the *real* client IP. Behind a reverse proxy the TCP peer is always
//! the proxy, so naive peer-IP keying would put every client into a single bucket.
//! [`ClientIpKeyExtractor`] instead reads `X-Forwarded-For` and trusts only the
//! right-most entries appended by our own proxy chain (`TRUSTED_PROXY_HOPS`),
//! which is the only way to extract the client IP without letting callers spoof it.

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use axum::extract::ConnectInfo;
use axum::http::Request;
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Tunable rate-limit parameters, sourced from the environment with production
/// defaults. All limits are **per real client IP**.
#[derive(Clone, Copy, Debug)]
pub struct RateLimitSettings {
    /// Number of trusted proxies in front of the app that append to
    /// `X-Forwarded-For`. `1` for a single reverse proxy (the default), `2` for
    /// e.g. CDN -> nginx -> app, `0` for a directly-exposed server (peer IP only).
    pub trusted_hops: usize,
    /// Burst capacity for general interactive endpoints (join / refresh / ws).
    pub api_burst: u32,
    /// Replenish interval (ms) for one general-endpoint token. 100ms => 10 req/s.
    pub api_period_ms: u64,
    /// Burst capacity for room creation.
    pub create_burst: u32,
    /// Replenish interval (ms) for one room-creation token. 2000ms => 0.5 req/s.
    pub create_period_ms: u64,
}

impl Default for RateLimitSettings {
    /// Production defaults (per real client IP):
    /// - general endpoints: burst 60, +1 token / 100ms => 10 req/s sustained
    /// - room creation:     burst 10, +1 token / 2000ms => 0.5 req/s sustained
    fn default() -> Self {
        Self {
            trusted_hops: 1,
            api_burst: 60,
            api_period_ms: 100,
            create_burst: 10,
            create_period_ms: 2000,
        }
    }
}

impl RateLimitSettings {
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            trusted_hops: env_or("TRUSTED_PROXY_HOPS", d.trusted_hops),
            api_burst: env_or("RL_API_BURST", d.api_burst),
            api_period_ms: env_or("RL_API_PERIOD_MS", d.api_period_ms),
            create_burst: env_or("RL_CREATE_BURST", d.create_burst),
            create_period_ms: env_or("RL_CREATE_PERIOD_MS", d.create_period_ms),
        }
    }

    pub fn extractor(&self) -> ClientIpKeyExtractor {
        ClientIpKeyExtractor::new(self.trusted_hops)
    }
}

fn env_or<T: FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// A [`KeyExtractor`] that resolves the real client IP behind trusted proxies.
///
/// The right-most `X-Forwarded-For` entry was appended by the proxy closest to
/// us (hop 1), the next one by the proxy before it, and so on. The genuine client
/// therefore sits exactly `trusted_hops` from the right; everything further left
/// was supplied by the client and is untrusted (spoofable). With `trusted_hops`
/// set correctly, a forged `X-Forwarded-For` cannot change the extracted key.
#[derive(Clone, Copy, Debug)]
pub struct ClientIpKeyExtractor {
    trusted_hops: usize,
}

impl ClientIpKeyExtractor {
    pub fn new(trusted_hops: usize) -> Self {
        Self { trusted_hops }
    }
}

impl KeyExtractor for ClientIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        // No trusted proxy: trust nothing but the actual peer, ignore the header.
        if self.trusted_hops == 0 {
            return peer_ip(req).ok_or(GovernorError::UnableToExtractKey);
        }

        if let Some(ip) = forwarded_client_ip(req, self.trusted_hops) {
            return Ok(ip);
        }

        // Header absent or shorter than expected (misconfigured proxy, or a client
        // trying to shrink the chain). Fall back to the peer IP: that collapses to a
        // shared bucket rather than honoring a spoofable value.
        peer_ip(req).ok_or(GovernorError::UnableToExtractKey)
    }
}

fn peer_ip<T>(req: &Request<T>) -> Option<IpAddr> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
}

fn forwarded_client_ip<T>(req: &Request<T>, trusted_hops: usize) -> Option<IpAddr> {
    let value = req.headers().get(X_FORWARDED_FOR)?.to_str().ok()?;
    let hops: Vec<&str> = value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();

    // Client IP = `trusted_hops` from the right. If the header is shorter than the
    // configured trust depth we refuse to guess and let the caller fall back.
    let index = hops.len().checked_sub(trusted_hops)?;
    hops.get(index)?.parse::<IpAddr>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    fn req_with_xff(value: &str) -> Request<()> {
        Request::builder()
            .header(X_FORWARDED_FOR, value)
            .body(())
            .unwrap()
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn single_proxy_takes_only_entry() {
        let ext = ClientIpKeyExtractor::new(1);
        assert_eq!(ext.extract(&req_with_xff("203.0.113.7")).unwrap(), ip("203.0.113.7"));
    }

    #[test]
    fn single_proxy_ignores_spoofed_left_entries() {
        // Client prepends a fake IP; our proxy appends the real one on the right.
        let ext = ClientIpKeyExtractor::new(1);
        assert_eq!(
            ext.extract(&req_with_xff("1.1.1.1, 203.0.113.7")).unwrap(),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn two_proxies_skip_one_trusted_hop() {
        // chain: client, edge-proxy ; app peer is the second proxy.
        let ext = ClientIpKeyExtractor::new(2);
        assert_eq!(
            ext.extract(&req_with_xff("203.0.113.7, 70.0.0.1")).unwrap(),
            ip("203.0.113.7")
        );
        // even with a spoofed left entry the real client survives
        assert_eq!(
            ext.extract(&req_with_xff("9.9.9.9, 203.0.113.7, 70.0.0.1")).unwrap(),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn too_few_hops_fall_back_to_peer() {
        // trusted_hops exceeds the header length -> None from the forwarded parser,
        // so extraction falls back to the peer IP (here: error, no ConnectInfo).
        assert!(forwarded_client_ip(&req_with_xff("203.0.113.7"), 2).is_none());
    }
}

/// End-to-end throughput tests: build a router with the *real* `GovernorLayer`,
/// fire requests through it, and count how many are allowed before a `429`. This
/// is how we verify the actual budget a client gets, not just the config values.
///
/// Run `cargo test -p server --  --nocapture` to print the measured numbers.
#[cfg(test)]
mod throughput {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt; // for `oneshot`
    use tower_governor::governor::GovernorConfigBuilder;
    use tower_governor::GovernorLayer;

    /// Build a one-route app guarded by a per-client-IP limiter with the given
    /// replenish period and burst -- the exact builder used in `main`.
    fn limited_app(extractor: ClientIpKeyExtractor, period_ms: u64, burst: u32) -> Router {
        let conf = Arc::new(
            GovernorConfigBuilder::default()
                .key_extractor(extractor)
                .per_millisecond(period_ms)
                .burst_size(burst)
                .finish()
                .expect("valid test rate limit config"),
        );
        Router::new()
            .route("/", get(|| async { StatusCode::OK }))
            .layer(GovernorLayer::new(conf))
    }

    /// Send one request carrying the given `X-Forwarded-For` and return the status.
    async fn send(app: &Router, xff: &str) -> StatusCode {
        let req = Request::builder()
            .uri("/")
            .header("x-forwarded-for", xff)
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap().status()
    }

    /// Fire `attempts` requests from one client and count how many were allowed.
    async fn count_allowed(app: &Router, xff: &str, attempts: usize) -> usize {
        let mut allowed = 0;
        for _ in 0..attempts {
            if send(app, xff).await == StatusCode::OK {
                allowed += 1;
            }
        }
        allowed
    }

    /// Drive an async test body to completion. We avoid `#[tokio::test]` because
    /// this workspace has a dependency literally named `core`, which shadows the
    /// sysroot `core` that the macro's expansion references.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
            .block_on(fut)
    }

    #[test]
    fn api_default_budget_is_60_requests() {
        block_on(async {
            let s = RateLimitSettings::default();
            let app = limited_app(s.extractor(), s.api_period_ms, s.api_burst);
            let attempts = (s.api_burst + 40) as usize;
            let allowed = count_allowed(&app, "203.0.113.10", attempts).await;
            println!(
                "[api] allowed {allowed} of {attempts} requests before 429 (burst {})",
                s.api_burst
            );
            // A fast burst replenishes ~0 tokens; allow +1 for timing slack.
            assert!(
                allowed >= s.api_burst as usize && allowed <= s.api_burst as usize + 1,
                "expected ~{} allowed, got {allowed}",
                s.api_burst
            );
        });
    }

    #[test]
    fn create_default_budget_is_10_requests() {
        block_on(async {
            let s = RateLimitSettings::default();
            let app = limited_app(s.extractor(), s.create_period_ms, s.create_burst);
            let attempts = (s.create_burst + 20) as usize;
            let allowed = count_allowed(&app, "203.0.113.11", attempts).await;
            println!(
                "[create] allowed {allowed} of {attempts} requests before 429 (burst {})",
                s.create_burst
            );
            assert!(
                allowed >= s.create_burst as usize && allowed <= s.create_burst as usize + 1,
                "expected ~{} allowed, got {allowed}",
                s.create_burst
            );
        });
    }

    #[test]
    fn request_past_the_burst_is_429() {
        block_on(async {
            // 1 token / minute => effectively no replenish during the test.
            let app = limited_app(ClientIpKeyExtractor::new(1), 60_000, 3);
            for i in 0..3 {
                assert_eq!(send(&app, "203.0.113.20").await, StatusCode::OK, "request {i}");
            }
            assert_eq!(
                send(&app, "203.0.113.20").await,
                StatusCode::TOO_MANY_REQUESTS,
                "4th request should be throttled"
            );
        });
    }

    #[test]
    fn separate_client_ips_get_independent_budgets() {
        block_on(async {
            let app = limited_app(ClientIpKeyExtractor::new(1), 60_000, 3);

            // Client A burns its whole budget...
            assert_eq!(count_allowed(&app, "203.0.113.1", 3).await, 3);
            assert_eq!(send(&app, "203.0.113.1").await, StatusCode::TOO_MANY_REQUESTS);

            // ...client B is unaffected and still gets the full burst.
            assert_eq!(count_allowed(&app, "203.0.113.2", 5).await, 3);
        });
    }

    #[test]
    fn spoofed_forwarded_prefixes_share_one_budget() {
        block_on(async {
            // Same real client (right-most entry); attacker varies the left entries.
            // With trusted_hops = 1 they all map to one bucket, so the burst is shared.
            let app = limited_app(ClientIpKeyExtractor::new(1), 60_000, 3);
            let spoofed = [
                "9.9.9.9, 203.0.113.5",
                "8.8.8.8, 203.0.113.5",
                "7.7.7.7, 203.0.113.5",
                "6.6.6.6, 203.0.113.5",
                "5.5.5.5, 203.0.113.5",
            ];
            let mut allowed = 0;
            for xff in spoofed {
                if send(&app, xff).await == StatusCode::OK {
                    allowed += 1;
                }
            }
            assert_eq!(allowed, 3, "spoofing the left XFF entries must not buy extra budget");
        });
    }

    #[test]
    fn tokens_replenish_over_time() {
        block_on(async {
            // burst 2, +1 token every 50ms.
            let app = limited_app(ClientIpKeyExtractor::new(1), 50, 2);
            assert_eq!(send(&app, "203.0.113.9").await, StatusCode::OK);
            assert_eq!(send(&app, "203.0.113.9").await, StatusCode::OK);
            assert_eq!(send(&app, "203.0.113.9").await, StatusCode::TOO_MANY_REQUESTS);

            // Wait for a couple of tokens to replenish, then we can send again.
            tokio::time::sleep(Duration::from_millis(120)).await;
            assert_eq!(send(&app, "203.0.113.9").await, StatusCode::OK);
        });
    }
}
