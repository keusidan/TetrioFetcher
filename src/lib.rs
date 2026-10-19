//! # TetrioFetcher
//!
//! A Rust client for the [TETR.IO](https://tetr.io) TETRA CHANNEL API that
//! **masks the API's problems** instead of forwarding them to your callers, and
//! hands back clean, normalized JSON.
//!
//! The upstream API is actively developed and, as its own documentation warns,
//! may change without notice. In practice that means a client written directly
//! against it inherits a pile of sharp edges. This crate exists to absorb them:
//!
//! | upstream behaviour | what you would normally get | what this crate does |
//! |---|---|---|
//! | `/users/:user/records/40l/pb` — `pb` is not a real leaderboard | `422 leaderboard is illegal` | translates `pb`/`personal`/`pbs` to `progression` |
//! | `/records/40l` — bare gamemode | `200` + `entries: []` | expands to `40l_global` |
//! | `/users/KEUSIDAN` | `404` | lowercases usernames for you |
//! | `/users/nobody/records/foo/top` — typo'd gamemode | `200` + `entries: []` | rejected locally with a message naming the option |
//! | `limit=0` / `limit=999` / `limit=abc` | three different `422`s | clamps into `1..=100` |
//! | `200 OK` with `{"success":false,...}` | truthy status, broken parse | [`Error::Api`](crate::Error::Api) with `key`/`context` |
//! | HTML error page served as `text/html` | `serde_json` syntax error | [`Error::NonJson`](crate::Error::NonJson) with a snippet |
//! | `xp: 22336` vs `xp: 68966508.611` | type mismatch at deserialize time | [`jval`] accessors accept both |
//! | `gamesplayed: -1` (hidden) | `-1` looks like a real value; `0` looks like "never played" | `None` for hidden, `0` stays `0` |
//! | `tr: -1`, `rank: "z"` (unranked) | `"z"` reported as a letter rank | `None` |
//! | `startTime: 9007199254740991` (JS sentinel) | a date far in the future | `None` |
//! | `aggregatestats.vsscore` vs `vs` | two names for one field | always `vs` |
//! | `cache.cached_until` in ms | callers re-request data unnecessarily | honoured; repeated calls are free |
//! | `429` / `5xx` / resets | one failed request | exponential backoff, 3 attempts |
//! | request floods | rate limited or blocked | shared 1 req/s throttle |
//!
//! ## Example
//!
//! ```no_run
//! use tetrio_fetcher::Fetcher;
//!
//! let fetcher = Fetcher::new();
//!
//! // Capital K works, and hidden totals come back as None.
//! let user = fetcher.user("Keusidan")?;
//! assert_eq!(user.username, "keusidan");
//!
//! // `pb` is not a valid leaderboard upstream; it is rewritten for you.
//! let pbs = fetcher.user_records("keusidan", "40l", "pb", 25)?;
//! for record in &pbs.entries {
//!     println!("{:?} {:?}", record.stats.lines, record.stats.score);
//! }
//! # Ok::<(), tetrio_fetcher::Error>(())
//! ```
//!
//! ## Design notes
//!
//! * **Synchronous.** TETRA CHANNEL is a plain JSON-over-HTTPS API with no
//!   streaming; a blocking client drops in anywhere and needs no runtime. It
//!   also makes the throttle and the backoff honest, which matters for a
//!   service that asks for roughly one request per second.
//! * **No panics on API data.** Parsing is total: an unknown field degrades to
//!   `None`, never to a panic or a failed whole-response parse.
//! * **Nothing is discarded silently.** [`Record::raw_results`] and
//!   [`Fetcher::raw_get`] make it possible to use data this crate has not
//!   modeled yet.
//! * **Offline-testable.** Fixtures in `tests/fixtures` drive the parser tests,
//!   so the suite does not depend on the API being up or on its current data.
//!
//! ## Etiquette
//!
//! This crate is not affiliated with TETR.IO. Use the TETRA CHANNEL API only,
//! identify yourself (the default `User-Agent` does), keep to about one request
//! per second, and honour the cache headers — all of which is the default here.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![warn(clippy::all)]

mod error;
mod fetcher;
mod model;
pub mod parse;
mod routes;
mod transport;

pub mod jval;

pub use crate::error::{Error, Result};
pub use crate::fetcher::{Fetcher, PageDirection, UserBoard};
pub use crate::model::{
    AchievementCounts, AggregateStats, Badge, Connection, GameStats, LeagueRanks, LeagueStanding,
    Page, RankBand, Record, RecordUser, ScoreFlow, ScoreFlowPoint, ServerStats, User,
    UserLeaderboardEntry,
};
pub use crate::routes::{clamp_limit, normalize_user, Gamemode, PersonalBoard, RecordLeaderboard};
pub use crate::transport::{
    unwrap_envelope, CacheInfo, Transport, TransportConfig, TransportStats, DEFAULT_BASE_URL,
    DEFAULT_USER_AGENT,
};

/// The crate version, for `User-Agent` strings and diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
