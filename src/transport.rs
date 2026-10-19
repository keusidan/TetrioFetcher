//! HTTP transport: the only place that knows about sockets, retries and caching.
//!
//! Responsibilities, in the order a request flows through this module:
//!
//! 1. **Politeness** — wait for the shared rate limiter before sending. The
//!    upstream docs ask for roughly one request per second.
//! 2. **Freshness** — if we already have this exact URL and the server told us
//!    `cache.cached_until` has not passed, serve the stored body without
//!    touching the network. The docs explicitly require honouring this.
//! 3. **Retry** — on connection errors, timeouts, `429` and `5xx`, retry with
//!    exponential backoff. `404`/`422` are *not* retried: they are answers.
//! 4. **Unwrap** — decode the `{"success":..,"data":..,"cache":..}` envelope.
//!    A non-JSON body (the HTML 404 page) becomes [`Error::NonJson`] with a
//!    snippet instead of a bare `serde_json` message.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::{Error, Result};

/// Default base URL of TETRA CHANNEL.
pub const DEFAULT_BASE_URL: &str = "https://ch.tetr.io/api";

/// Default `User-Agent`. TETR.IO asks bots to identify themselves.
pub const DEFAULT_USER_AGENT: &str = concat!(
    "TetrioFetcher/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/keusidan/TetrioFetcher)"
);

/// The `cache` block returned with successful responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheInfo {
    /// `"miss"`, `"hit"` or something new upstream invented.
    pub status: String,
    /// Milliseconds since the Unix epoch when the datum was cached.
    pub cached_at_ms: u64,
    /// Milliseconds since the Unix epoch when it stops being valid.
    pub cached_until_ms: u64,
}

impl CacheInfo {
    /// Seconds the caller should treat this datum as fresh.
    pub fn ttl(&self) -> Duration {
        let millis = self.cached_until_ms.saturating_sub(self.cached_at_ms);
        Duration::from_millis(millis)
    }

    /// Whether the datum is still fresh relative to the local clock.
    pub fn is_fresh(&self) -> bool {
        now_millis() < self.cached_until_ms
    }
}

/// Knobs for the HTTP layer.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Base URL without a trailing slash.
    pub base_url: String,
    /// Sent as `User-Agent`.
    pub user_agent: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// How many times to retry retryable failures (so 3 means up to 3 calls).
    pub max_attempts: u32,
    /// First backoff delay; doubles every attempt.
    pub backoff_base: Duration,
    /// Minimum spacing between requests, to stay a good citizen.
    pub min_interval: Duration,
    /// Honour the server's `cached_until` and reuse remembered bodies.
    pub honour_server_cache: bool,
    /// Optional `X-Session-ID`; keeps paginated reads consistent server-side.
    pub session_id: Option<String>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            timeout: Duration::from_secs(20),
            max_attempts: 3,
            backoff_base: Duration::from_millis(400),
            min_interval: Duration::from_secs(1),
            honour_server_cache: true,
            session_id: None,
        }
    }
}

struct CacheEntry {
    body: Value,
    fresh_until: Instant,
}

/// Small, synchronous HTTP client with the upstream's quirks baked in.
///
/// Not `Sync`: it holds a `RefCell` cache and a "last request" clock. Create one
/// per thread, or wrap in a `Mutex`.
pub struct Transport {
    config: TransportConfig,
    agent: ureq::Agent,
    cache: RefCell<HashMap<String, CacheEntry>>,
    last_request: RefCell<Option<Instant>>,
    stats: RefCell<TransportStats>,
}

/// Counters useful in tests and diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransportStats {
    /// Requests that actually went over the network.
    pub network_requests: u32,
    /// Requests answered from the server-cache.
    pub cache_hits: u32,
    /// Retry attempts performed.
    pub retries: u32,
    /// Bodies that were not JSON.
    pub non_json_responses: u32,
}

impl Transport {
    /// Build a client with the given configuration.
    pub fn new(config: TransportConfig) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(config.timeout))
            // We want to inspect 4xx/5xx bodies ourselves: the API puts useful
            // information (`error.msg`) in them.
            .http_status_as_error(false)
            .user_agent(config.user_agent.clone())
            .build()
            .into();
        Self {
            config,
            agent,
            cache: RefCell::new(HashMap::new()),
            last_request: RefCell::new(None),
            stats: RefCell::new(TransportStats::default()),
        }
    }

    /// The effective configuration.
    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    /// A snapshot of the request counters.
    pub fn stats(&self) -> TransportStats {
        self.stats.borrow().clone()
    }

    /// Drop every remembered body.
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }

    /// GET `path` (e.g. `/users/keusidan`) and return the unwrapped `data`.
    pub fn get(&self, path: &str) -> Result<Value> {
        self.get_with_query(path, &[])
    }

    /// GET `path` with query pairs, returning the unwrapped `data`.
    ///
    /// Query keys and values are percent-encoded, so `Country=JP` and
    /// `Name=A B` are safe to pass.
    pub fn get_with_query(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let url = self.build_url(path, query);
        let body = self.fetch_raw(&url)?;
        unwrap_envelope(&url, body)
    }

    /// GET and return the whole envelope (`success`, `data`, `cache`).
    ///
    /// Most callers want [`Transport::get`]; this exists so the CLI can show the
    /// cache block and so tests can assert on envelope handling.
    pub fn get_envelope(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let url = self.build_url(path, query);
        self.fetch_raw(&url)
    }

    /// Build a fully-encoded URL.
    pub fn build_url(&self, path: &str, query: &[(&str, String)]) -> String {
        let mut url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        if !query.is_empty() {
            url.push('?');
            let encoded: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
                .collect();
            url.push_str(&encoded.join("&"));
        }
        url
    }

    fn fetch_raw(&self, url: &str) -> Result<Value> {
        if self.config.honour_server_cache {
            if let Some(body) = self.cache_lookup(url) {
                self.stats.borrow_mut().cache_hits += 1;
                return Ok(body);
            }
        }

        let mut attempt = 0;
        loop {
            attempt += 1;
            self.throttle();
            self.stats.borrow_mut().network_requests += 1;

            match self.attempt(url) {
                Ok((status, body)) => {
                    if status == 429 || (500..600).contains(&status) {
                        if attempt < self.config.max_attempts {
                            self.stats.borrow_mut().retries += 1;
                            self.sleep_backoff(attempt);
                            continue;
                        }
                        return Err(Error::Unavailable {
                            status,
                            attempts: attempt,
                        });
                    }
                    // 2xx, 404, 422 ... anything else: the body is the answer.
                    if let Some(cache) = body.get("cache").and_then(parse_cache) {
                        self.remember(url, &body, cache);
                    }
                    return Ok(body);
                }
                Err(err) if err.is_retryable() && attempt < self.config.max_attempts => {
                    self.stats.borrow_mut().retries += 1;
                    self.sleep_backoff(attempt);
                }
                Err(err) => {
                    return Err(match err {
                        Error::Transport { source_msg, .. } => Error::Transport {
                            attempts: attempt,
                            source_msg,
                        },
                        other => other,
                    })
                }
            }
        }
    }

    /// One network attempt: returns `(status, parsed_body)`.
    fn attempt(&self, url: &str) -> Result<(u16, Value)> {
        let mut request = self.agent.get(url).header("Accept", "application/json");
        if let Some(session) = &self.config.session_id {
            request = request.header("X-Session-ID", session);
        }

        let mut response = match request.call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(code)) => {
                // Only reachable when http_status_as_error is on; kept for safety.
                return Err(Error::Unavailable {
                    status: code,
                    attempts: 1,
                });
            }
            Err(e) => {
                return Err(Error::Transport {
                    attempts: 1,
                    source_msg: describe_ureq_error(&e),
                })
            }
        };

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        let body = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| Error::Transport {
                attempts: 1,
                source_msg: e.to_string(),
            })?;

        match serde_json::from_str::<Value>(&body) {
            Ok(v) => Ok((status, v)),
            Err(_) => {
                // The TETRA CHANNEL 404 page, a Cloudflare interstitial, or an
                // HTML error page. Report it as such instead of "expected value
                // at line 1 column 1".
                self.stats.borrow_mut().non_json_responses += 1;
                Err(Error::NonJson {
                    url: url.to_string(),
                    content_type,
                    snippet: snippet(&body),
                })
            }
        }
    }

    fn cache_lookup(&self, url: &str) -> Option<Value> {
        let cache = self.cache.borrow();
        let entry = cache.get(url)?;
        if Instant::now() < entry.fresh_until {
            Some(entry.body.clone())
        } else {
            None
        }
    }

    fn remember(&self, url: &str, body: &Value, cache: CacheInfo) {
        if !cache.is_fresh() || cache.cached_until_ms == 0 {
            return;
        }
        let remaining_ms = cache.cached_until_ms.saturating_sub(now_millis());
        let fresh_until = Instant::now() + Duration::from_millis(remaining_ms);
        self.cache.borrow_mut().insert(
            url.to_string(),
            CacheEntry {
                body: body.clone(),
                fresh_until,
            },
        );
    }

    fn throttle(&self) {
        let mut last = self.last_request.borrow_mut();
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < self.config.min_interval {
                std::thread::sleep(self.config.min_interval - elapsed);
            }
        }
        *last = Some(Instant::now());
    }

    fn sleep_backoff(&self, attempt: u32) {
        let factor = 1u32 << attempt.saturating_sub(1).min(5);
        let delay = self.config.backoff_base.saturating_mul(factor);
        std::thread::sleep(delay.min(Duration::from_secs(30)));
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new(TransportConfig::default())
    }
}

/// Split the envelope and return `data`, or a precise [`Error`].
pub fn unwrap_envelope(url: &str, body: Value) -> Result<Value> {
    let success = match body.get("success") {
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return Err(Error::Malformed {
                url: url.to_string(),
                detail: format!("`success` is not a bool but {other}"),
            })
        }
        None => {
            return Err(Error::Malformed {
                url: url.to_string(),
                detail: "response has no `success` field".to_string(),
            })
        }
    };

    if !success {
        let error = body.get("error").cloned().unwrap_or(Value::Null);
        let msg = error
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("the API reported failure without a message")
            .to_string();
        return Err(Error::Api {
            // The envelope itself does not carry the status; the caller-visible
            // HTTP status is 404 for "no such user" and 422 for bad params. Both
            // are recoverable by the caller, which is what matters here.
            status: infer_status(&msg),
            msg,
            key: error
                .get("key")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            context: error
                .get("context")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }

    match body.get("data") {
        Some(data) => Ok(data.clone()),
        None => Err(Error::Malformed {
            url: url.to_string(),
            detail: "`success` is true but there is no `data`".to_string(),
        }),
    }
}

/// Best-effort status inference for envelope-level errors.
fn infer_status(msg: &str) -> u16 {
    if msg.contains("No such user") || msg.contains("no longer exists") {
        404
    } else if msg.contains("illegal")
        || msg.contains("must be")
        || msg.contains("must contain")
        || msg.contains("may not be combined")
    {
        422
    } else {
        400
    }
}

fn parse_cache(value: &Value) -> Option<CacheInfo> {
    let status = value.get("status")?.as_str()?.to_string();
    let cached_at_ms = value.get("cached_at")?.as_u64().or_else(|| {
        value
            .get("cached_at")?
            .as_f64()
            .filter(|f| *f >= 0.0)
            .map(|f| f as u64)
    })?;
    let cached_until_ms = value.get("cached_until")?.as_u64().or_else(|| {
        value
            .get("cached_until")?
            .as_f64()
            .filter(|f| *f >= 0.0)
            .map(|f| f as u64)
    })?;
    Some(CacheInfo {
        status,
        cached_at_ms,
        cached_until_ms,
    })
}

fn snippet(body: &str) -> String {
    let cleaned: String = body.chars().take(120).collect();
    cleaned.replace(['\n', '\r', '\t'], " ")
}

fn describe_ureq_error(err: &ureq::Error) -> String {
    match err {
        ureq::Error::Timeout(_) => "request timed out".to_string(),
        ureq::Error::Io(io) => format!("io: {io}"),
        other => other.to_string(),
    }
}

fn encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' | b'@' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_success_returns_data() {
        let body = json!({"success": true, "data": {"username": "keusidan"}});
        let data = unwrap_envelope("u", body).unwrap();
        assert_eq!(data["username"], "keusidan");
    }

    #[test]
    fn envelope_failure_becomes_typed_api_error() {
        let body = json!({"success": false, "error": {"msg": "The params field 'leaderboard' is illegal", "key": "leaderboard", "context": "params"}});
        let err = unwrap_envelope("u", body).unwrap_err();
        match &err {
            Error::Api {
                msg, key, context, ..
            } => {
                assert!(msg.contains("illegal"));
                assert_eq!(key.as_deref(), Some("leaderboard"));
                assert_eq!(context.as_deref(), Some("params"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
        assert_eq!(err.offending_key(), Some("leaderboard"));
        assert!(err.is_likely_user_mistake());
    }

    #[test]
    fn no_such_user_is_flagged_as_404() {
        let body = json!({"success": false, "error": {"msg": "No such user! | Either you mistyped something, or the account no longer exists."}});
        match unwrap_envelope("u", body).unwrap_err() {
            Error::Api { status, .. } => assert_eq!(status, 404),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_data_but_success_is_malformed() {
        let body = json!({"success": true});
        assert!(matches!(
            unwrap_envelope("u", body).unwrap_err(),
            Error::Malformed { .. }
        ));
    }

    #[test]
    fn missing_success_is_malformed() {
        let body = json!({"data": {}});
        assert!(matches!(
            unwrap_envelope("u", body).unwrap_err(),
            Error::Malformed { .. }
        ));
    }

    #[test]
    fn cache_block_parses_and_ttl_is_sane() {
        let cache = parse_cache(
            &json!({"status": "hit", "cached_at": 1_000u64, "cached_until": 61_000u64}),
        )
        .unwrap();
        assert_eq!(cache.status, "hit");
        assert_eq!(cache.ttl(), Duration::from_secs(60));
    }

    #[test]
    fn url_encoding_is_applied_to_query_values() {
        let t = Transport::new(TransportConfig {
            base_url: "https://ch.tetr.io/api/".into(),
            ..Default::default()
        });
        let url = t.build_url("/users/search/a b:c", &[("limit", "25".into())]);
        assert_eq!(url, "https://ch.tetr.io/api/users/search/a b:c?limit=25");
        let url2 = t.build_url("records/40l_global", &[("country", "JP".into())]);
        assert_eq!(url2, "https://ch.tetr.io/api/records/40l_global?country=JP");
    }

    #[test]
    fn base_url_trailing_slash_does_not_duplicate() {
        let t = Transport::new(TransportConfig {
            base_url: "https://example.test/api///".into(),
            ..Default::default()
        });
        assert_eq!(
            t.build_url("/users/x", &[]),
            "https://example.test/api/users/x"
        );
    }
}
