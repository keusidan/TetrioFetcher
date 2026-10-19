//! The single error type every fetcher call returns.
//!
//! The upstream API signals failure in at least five different ways, and only
//! some of them come with an HTTP status that matches:
//!
//! | what you actually get back                                   | HTTP | this crate |
//! |--------------------------------------------------------------|------|------------|
//! | `{"success":false,"error":{"msg":..,"key":..,"context":..}}` | 422  | [`Error::Api`] |
//! | `{"success":false,"error":{"msg":"No such user! .."}}`       | 404  | [`Error::Api`] |
//! | a full TETRA CHANNEL **HTML** 404 page                       | 404  | [`Error::NonJson`] |
//! | `{"success":true,"data":{"entries":[]}}` for a bogus route   | 200  | [`Error::EmptyResult`] (or an empty `Ok` if `allow_empty`) |
//! | Cloudflare 5xx / connection reset / TLS failure              | 5xx  | [`Error::Transport`] |
//!
//! Everything is funneled into one enum so callers can `match` instead of
//! writing `try/catch` around a JSON parser.

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong between "user asked for X" and "user got X".
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The API accepted the request and answered with a structured error.
    ///
    /// This covers `success:false` bodies on 200, 404 and 422 responses alike.
    #[error("TETRA CHANNEL API error ({status}): {msg}{}", format_hint(.key, .context))]
    Api {
        /// HTTP status that accompanied the error body.
        status: u16,
        /// The server's human readable message (`error.msg`).
        msg: String,
        /// Which field the server objected to (`error.key`), if it said.
        key: Option<String>,
        /// Where the offending field lives (`error.context`), usually `params` or `query`.
        context: Option<String>,
    },

    /// The upstream answered with something that is not JSON at all.
    ///
    /// In practice this is the TETRA CHANNEL HTML 404 page:
    /// `/users/:user/records/:gamemode` without a leaderboard segment. Naive
    /// clients feed that `<!DOCTYPE html>` into `serde_json` and surface
    /// `expected value at line 1 column 1`, which tells the user nothing.
    #[error("expected JSON from {url} but got {content_type} (first bytes: {snippet:?})")]
    NonJson {
        /// The URL that misbehaved.
        url: String,
        /// The `content-type` header, or `unknown`.
        content_type: String,
        /// First bytes of the body, for diagnosis.
        snippet: String,
    },

    /// The response was well-formed JSON but not shaped like the documented envelope.
    #[error("unexpected response shape from {url}: {detail}")]
    Malformed {
        /// The URL that misbehaved.
        url: String,
        /// What about the shape was wrong.
        detail: String,
    },

    /// The route exists, the envelope is valid, but there is nothing in it.
    ///
    /// The API returns `200` + `entries: []` for *typo'd* leaderboard ids
    /// (`records/99l_global`) and unknown gamemodes (`records/foo/top`), so an
    /// empty result is often indistinguishable from a mistake. This variant lets
    /// callers treat "empty" as a failure explicitly.
    #[error("empty result from {url} (the API returns 200 + [] for unknown ids)")]
    EmptyResult {
        /// The URL that returned nothing.
        url: String,
    },

    /// Network-layer problem: DNS, TLS, connection reset, timeout, 5xx exhaustion.
    #[error("transport error after {attempts} attempt(s): {source_msg}")]
    Transport {
        /// How many times we tried before giving up.
        attempts: u32,
        /// Rendered source error.
        source_msg: String,
    },

    /// We gave up because the response was a 5xx or 429 that survived every retry.
    #[error("upstream unavailable: HTTP {status} after {attempts} attempt(s)")]
    Unavailable {
        /// Last status seen.
        status: u16,
        /// How many attempts were made.
        attempts: u32,
    },

    /// The caller handed us something we refuse to send upstream.
    #[error("invalid {field}: {value:?} ({reason})")]
    Invalid {
        /// Which argument was rejected.
        field: &'static str,
        /// The rejected value, already rendered for display.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },
}

fn format_hint(key: &Option<String>, context: &Option<String>) -> String {
    match (key, context) {
        (Some(k), Some(c)) => format!(" [{c}.{k}]"),
        (Some(k), None) => format!(" [{k}]"),
        (None, Some(c)) => format!(" [{c}]"),
        (None, None) => String::new(),
    }
}

impl Error {
    /// The offending parameter name, when the server named one.
    ///
    /// Useful for logging or for fixing the caller's argument.
    pub fn offending_key(&self) -> Option<&str> {
        match self {
            Error::Api { key, .. } => key.as_deref(),
            _ => None,
        }
    }

    /// True when retrying the identical request could plausibly succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Error::Transport { .. } | Error::Unavailable { .. })
    }

    /// True when the user most likely typo'd a username or a leaderboard id.
    pub fn is_likely_user_mistake(&self) -> bool {
        matches!(self, Error::EmptyResult { .. } | Error::Invalid { .. })
            || matches!(
                self,
                Error::Api {
                    status: 404 | 422,
                    ..
                }
            )
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Transport {
            attempts: 1,
            source_msg: e.to_string(),
        }
    }
}

// Assertions want to compare errors without a snapshot library; the rendered
// `Debug` form is stable and complete.
impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        format!("{self:?}") == format!("{other:?}")
    }
}
