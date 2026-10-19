//! Normalized output types.
//!
//! Everything here is `Serialize` only: these are what the fetcher *produces*.
//! They intentionally differ from the upstream shapes:
//!
//! * Field names are `snake_case` and consistent — upstream uses `_id` for
//!   record ids and user ids inside entries, `id` inside `user`, and mixes
//!   `vsscore` with `vs`.
//! * Sentinels are resolved to `null`: hidden stats (`-1`), unranked ratings
//!   (`-1`), unranked ranks (`"z"`) and the JS `MAX_SAFE_INTEGER` timestamp.
//! * Numbers are the type they semantically are (`i64` counters, `f64` ratings).
//! * Purely computed conveniences (PPS from piece count, formatted duration)
//!   are added so consumers do not each reinvent them wrongly.

use serde::{Deserialize, Serialize};

/// A user account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct User {
    /// Internal MongoID.
    pub id: String,
    /// Lowercase username.
    pub username: String,
    /// `anon`, `user`, `bot`, `halfmod`, `mod`, `admin`, `sysop`.
    pub role: String,
    /// Account creation time, ISO-8601, when the server recorded one.
    pub registered_at: Option<String>,
    /// Experience points. Integer-valued but shipped as a float by upstream.
    pub xp: f64,
    /// Lifetime games played; `None` when the user hides it.
    pub games_played: Option<i64>,
    /// Lifetime games won; `None` when the user hides it.
    pub games_won: Option<i64>,
    /// Seconds of play; `None` when the user hides it.
    pub gametime_seconds: Option<f64>,
    /// ISO country code, or `None` when hidden or unknown.
    pub country: Option<String>,
    /// Whether the account is in bad standing (recent bans).
    pub bad_standing: Option<bool>,
    /// Whether the account currently supports TETR.IO.
    pub supporter: bool,
    /// Supporter tier (0 when not a supporter).
    pub supporter_tier: i64,
    /// Friend count.
    pub friend_count: i64,
    /// Achievement Rating.
    pub achievement_rating: i64,
    /// Breakdown of AR by achievement tier.
    pub achievement_counts: AchievementCounts,
    /// Badges, if any.
    pub badges: Vec<Badge>,
    /// Connected third-party accounts (`discord`, `twitch`, ...).
    pub connections: std::collections::BTreeMap<String, Connection>,
}

/// A third-party account connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Connection {
    /// The provider's numeric id.
    pub id: String,
    /// Account username.
    pub username: String,
    /// Friendly display name, when provided.
    pub display_name: Option<String>,
}

/// A profile badge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Badge {
    /// Badge id.
    pub id: String,
    /// Human label.
    pub label: String,
    /// Optional grouping (e.g. `supporter`).
    pub group: Option<String>,
    /// When it was granted, if the server records it.
    pub granted_at: Option<String>,
}

/// Achievement Rating split by tier.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AchievementCounts {
    /// Bronze achievements.
    pub bronze: i64,
    /// Silver achievements.
    pub silver: i64,
    /// Gold achievements.
    pub gold: i64,
    /// Platinum achievements.
    pub platinum: i64,
    /// Diamond achievements.
    pub diamond: i64,
    /// Issued achievements.
    pub issued: i64,
    /// Achievements in the global top 100.
    pub top100: i64,
    /// Achievements in the global top 50.
    pub top50: i64,
    /// Achievements in the global top 25.
    pub top25: i64,
    /// Achievements in the global top 10.
    pub top10: i64,
    /// Achievements in the global top 5.
    pub top5: i64,
    /// Achievements in the global top 3.
    pub top3: i64,
}

/// Average APM / PPS / VS over the last ten games.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AggregateStats {
    /// Attack per minute.
    pub apm: f64,
    /// Pieces per second.
    pub pps: f64,
    /// Versus score.
    pub vs: f64,
}

impl AggregateStats {
    /// Attack per piece, a stat TETR.IO players care about but the API omits.
    ///
    /// `None` when PPS is zero (a valid but degenerate record).
    pub fn app(&self) -> Option<f64> {
        if self.pps > 0.0 {
            Some(self.apm / self.pps / 60.0)
        } else {
            None
        }
    }
}

/// A player's TETRA LEAGUE standing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LeagueStanding {
    /// Games played this season.
    pub games_played: i64,
    /// Games won this season.
    pub games_won: i64,
    /// Tetra Rating; `None` when unranked (`-1` upstream).
    pub tr: Option<f64>,
    /// GLIXARE, a win chance estimate against an average player.
    pub gxe: Option<f64>,
    /// Letter rank; `None` when unranked (`"z"` upstream).
    pub rank: Option<String>,
    /// Highest letter rank this season.
    pub best_rank: Option<String>,
    /// Glicko-2 rating.
    pub glicko: Option<f64>,
    /// Glicko-2 rating deviation.
    pub rd: Option<f64>,
    /// Recent average APM.
    pub apm: Option<f64>,
    /// Recent average PPS.
    pub pps: Option<f64>,
    /// Recent average VS.
    pub vs: Option<f64>,
    /// Whether the rating is decaying for inactivity.
    pub decaying: bool,
}

impl LeagueStanding {
    /// The recent averages as one block, when the server gave all three.
    pub fn recent_stats(&self) -> Option<AggregateStats> {
        match (self.apm, self.pps, self.vs) {
            (Some(apm), Some(pps), Some(vs)) => Some(AggregateStats { apm, pps, vs }),
            _ => None,
        }
    }
}

/// One entry of a user leaderboard (`/users/by/:leaderboard`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserLeaderboardEntry {
    /// Placement on this leaderboard, 1-based.
    pub placement: u64,
    /// Internal user id.
    pub user_id: String,
    /// Username.
    pub username: String,
    /// Role.
    pub role: String,
    /// XP (leaderboard `xp`).
    pub xp: Option<f64>,
    /// Achievement Rating (leaderboard `ar`).
    pub achievement_rating: Option<i64>,
    /// League standing (leaderboard `league`).
    pub league: Option<LeagueStanding>,
    /// Country code.
    pub country: Option<String>,
    /// Whether the user supports TETR.IO.
    pub supporter: bool,
    /// Lifetime games played; `None` when hidden.
    pub games_played: Option<i64>,
    /// Lifetime games won; `None` when hidden.
    pub games_won: Option<i64>,
    /// Seconds of play; `None` when hidden.
    pub gametime_seconds: Option<f64>,
    /// Pagination cursor. Pass the last entry's value back as `after`/`before`.
    pub cursor: Option<String>,
}

/// Final statistics of a single-player game.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GameStats {
    /// Lines cleared.
    pub lines: Option<i64>,
    /// Score.
    pub score: Option<i64>,
    /// Pieces placed.
    pub pieces_placed: Option<i64>,
    /// Keys pressed.
    pub inputs: Option<i64>,
    /// Holds used.
    pub holds: Option<i64>,
    /// Level reached.
    pub level: Option<i64>,
    /// Highest combo.
    pub top_combo: Option<i64>,
    /// Highest back-to-back chain.
    pub top_btb: Option<i64>,
    /// Total T-spins.
    pub tspins: Option<i64>,
    /// Quad clears.
    pub quads: Option<i64>,
    /// PPS as sent by the server, when present.
    pub pps: Option<f64>,
    /// The untouched upstream object, so no field is ever truly lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// A record: one completed game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    /// Record id.
    pub id: String,
    /// Replay id, for fetching the replay.
    pub replay_id: Option<String>,
    /// Gamemode token (`40l`, `blitz`, ...).
    pub gamemode: String,
    /// Whether this was a personal best on that leaderboard.
    pub personal_best: bool,
    /// Whether it was a PB at the time it was set.
    pub was_personal_best: bool,
    /// When it was set.
    pub played_at: Option<String>,
    /// Revolution id the record belongs to, if any.
    pub revolution: Option<String>,
    /// The player who set it.
    pub user: Option<RecordUser>,
    /// Other players, for versus records (TETRA LEAGUE).
    pub other_users: Vec<RecordUser>,
    /// Leaderboards this record appears on.
    pub leaderboards: Vec<String>,
    /// Final stats.
    pub stats: GameStats,
    /// Aggregate stats for versus records.
    pub aggregate: Option<AggregateStats>,
    /// The raw `results` object, for anything not normalized above.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_results: Option<serde_json::Value>,
}

/// The player embedded in a record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordUser {
    /// Internal user id.
    pub id: String,
    /// Username.
    pub username: String,
    /// Country at the time of the record.
    pub country: Option<String>,
    /// Whether they supported TETR.IO at the time.
    pub supporter: bool,
}

/// One point of a score-flow graph.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScoreFlowPoint {
    /// Unix timestamp in seconds.
    pub timestamp: i64,
    /// Whether the score was a personal best.
    pub is_pb: bool,
    /// The score. Negative for 40 LINES because lower is better.
    pub score: i64,
}

/// Condensed progression graph (`/labs/scoreflow`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreFlow {
    /// Start of the graph, or `None` when the feed is empty.
    pub start_time: Option<i64>,
    /// The points.
    pub points: Vec<ScoreFlowPoint>,
}

/// A TETRA LEAGUE rating band from `/labs/league_ranks`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankBand {
    /// The letter rank (`x+`, `x`, `u`, `ss`, ...).
    pub rank: String,
    /// Global position of the first player in the band.
    pub position: i64,
    /// Percentile cutoff, e.g. `0.002` for the top 0.2%.
    pub percentile: Option<f64>,
    /// TR needed for the band.
    pub tr: Option<f64>,
    /// Target TR for the band.
    pub target_tr: Option<f64>,
    /// Players in the band.
    pub count: i64,
    /// Average APM of the band.
    pub apm: Option<f64>,
    /// Average PPS of the band.
    pub pps: Option<f64>,
    /// Average VS of the band.
    pub vs: Option<f64>,
}

/// The whole `/labs/league_ranks` snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LeagueRanks {
    /// Snapshot id.
    pub id: String,
    /// Total ranked players.
    pub total: i64,
    /// One entry per band, highest first.
    pub bands: Vec<RankBand>,
}

/// Server-wide statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerStats {
    /// Players on the server, including anonymous accounts.
    pub user_count: i64,
    /// New users per second over the last minute.
    pub user_count_delta: f64,
    /// Anonymous accounts.
    pub anon_count: i64,
    /// Accounts ever created.
    pub total_accounts: i64,
    /// Ranked accounts.
    pub ranked_count: i64,
    /// Stored game records.
    pub record_count: i64,
    /// Games played.
    pub games_played: i64,
    /// Games played per second over the last minute.
    pub games_played_delta: f64,
    /// Games finished.
    pub games_finished: i64,
    /// Total seconds played.
    pub gametime_seconds: f64,
    /// Total keys pressed.
    pub inputs: i64,
    /// Total pieces placed.
    pub pieces_placed: i64,
    /// Derived: pieces per second, server-wide.
    pub average_pps: Option<f64>,
    /// Derived: keys per second, server-wide.
    pub average_kps: Option<f64>,
}

impl ServerStats {
    /// Build the derived rates. The upstream site computes the same two numbers
    /// client-side; doing it here keeps every consumer consistent.
    pub fn with_derived(mut self) -> Self {
        if self.gametime_seconds > 0.0 {
            self.average_pps = Some(self.pieces_placed as f64 / self.gametime_seconds);
            self.average_kps = Some(self.inputs as f64 / self.gametime_seconds);
        }
        self
    }
}

/// A page of results, with everything needed to fetch the next one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Page<T> {
    /// The items on this page.
    pub entries: Vec<T>,
    /// Cursor to pass as `after` for the next (lower) page.
    pub next_cursor: Option<String>,
    /// How many items were requested after clamping.
    pub requested_limit: u32,
}

impl<T> Page<T> {
    /// True when the API returned no entries at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
