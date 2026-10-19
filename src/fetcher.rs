//! The public facade: [`Fetcher`].
//!
//! ```no_run
//! use tetrio_fetcher::Fetcher;
//!
//! let f = Fetcher::new();
//! let user = f.user("Keusidan")?;          // capital K is fine, we lowercase
//! println!("{} has {} XP", user.username, user.xp);
//!
//! // `pb` is not a real leaderboard name; it is translated to `progression`.
//! let pbs = f.user_records("keusidan", "40l", "pb", 10)?;
//! # Ok::<(), tetrio_fetcher::Error>(())
//! ```
//!
//! Every method returns [`crate::Result`] and never panics on API data.

use serde_json::Value;

use crate::error::{Error, Result};
use crate::jval;
use crate::model::*;
use crate::parse;
use crate::routes::{self, Gamemode, PersonalBoard, RecordLeaderboard};
use crate::transport::{Transport, TransportConfig};

/// Leaderboard kinds accepted by `/users/by/:leaderboard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserBoard {
    /// TETRA LEAGUE ranking by TR.
    League,
    /// Ranking by XP.
    Xp,
    /// Ranking by Achievement Rating.
    Ar,
}

impl UserBoard {
    /// The wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            UserBoard::League => "league",
            UserBoard::Xp => "xp",
            UserBoard::Ar => "ar",
        }
    }

    /// Parse, repairing plural and case variants.
    pub fn parse(input: &str) -> Result<Self> {
        let normalized: String = input
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        match normalized.as_str() {
            "league" | "tl" | "tetraleague" => Ok(UserBoard::League),
            "xp" | "experience" => Ok(UserBoard::Xp),
            "ar" | "achievementrating" | "achievements" => Ok(UserBoard::Ar),
            _ => Err(Error::Invalid {
                field: "leaderboard",
                value: input.to_string(),
                reason: "expected one of league, xp, ar",
            }),
        }
    }
}

/// Direction of pagination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageDirection {
    /// Towards lower-ranked entries (`after`).
    Down,
    /// Towards higher-ranked entries (`before`).
    Up,
}

/// A client for the TETRA CHANNEL API.
///
/// Cheap to construct. Holds an in-memory cache keyed by URL, so repeated
/// calls within the server's advertised TTL cost nothing.
pub struct Fetcher {
    transport: Transport,
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher {
    /// Build with default settings (1 req/s, 3 attempts, server cache honoured).
    pub fn new() -> Self {
        Self {
            transport: Transport::new(TransportConfig::default()),
        }
    }

    /// Build from an explicit configuration.
    pub fn with_config(config: TransportConfig) -> Self {
        Self {
            transport: Transport::new(config),
        }
    }

    /// Replace the transport wholesale.
    pub fn with_transport(transport: Transport) -> Self {
        Self { transport }
    }

    /// Access the underlying transport (for `stats()`, config, cache control).
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    // ---------------------------------------------------------------- general

    /// `/general/stats`
    pub fn server_stats(&self) -> Result<ServerStats> {
        let data = self.transport.get("general/stats")?;
        Ok(parse::parse_server_stats(&data))
    }

    /// `/general/activity` — the last 24 hours of player counts, oldest first.
    pub fn activity(&self) -> Result<Vec<f64>> {
        let data = self.transport.get("general/activity")?;
        Ok(jval::as_num_array(&data, "activity"))
    }

    // ------------------------------------------------------------------ users

    /// `/users/:user`
    ///
    /// The username is lowercased automatically: the API answers `404` for
    /// `Keusidan`, which is the single most common false "user not found".
    pub fn user(&self, user: &str) -> Result<User> {
        let id = routes::normalize_user(user)?;
        let data = self.transport.get(&format!("users/{id}"))?;
        Ok(parse::parse_user(&data))
    }

    /// `/users/:user/summaries/league`
    pub fn user_league(&self, user: &str) -> Result<LeagueStanding> {
        let id = routes::normalize_user(user)?;
        let data = self.transport.get(&format!("users/{id}/summaries/league"))?;
        Ok(parse::parse_league_standing(&data))
    }

    /// `/users/:user/summaries` — every summary at once, untyped.
    ///
    /// Returned as JSON because the payload mixes 40l, blitz, zenith, league,
    /// zen and an achievements array; the interesting parts have dedicated
    /// methods. Convenience accessor: [`Self::user_personal_best`].
    pub fn user_summaries(&self, user: &str) -> Result<Value> {
        let id = routes::normalize_user(user)?;
        self.transport.get(&format!("users/{id}/summaries"))
    }

    /// The personal best for one gamemode, read from `/users/:user/summaries`.
    ///
    /// Returns `None` when the player has no record in that mode — which is the
    /// difference between "you have no PB" and "the API is broken".
    pub fn user_personal_best(&self, user: &str, gamemode: &str) -> Result<Option<Record>> {
        let mode = Gamemode::parse(gamemode)?;
        let summaries = self.user_summaries(user)?;
        let container = jval::get(&summaries, mode.as_str());
        let record = container
            .and_then(|c| jval::get(c, "record"))
            .filter(|r| r.is_object());
        Ok(record.map(parse::parse_record))
    }

    /// `/users/search/:query`
    ///
    /// The API requires the query to contain **two colons** — `social:type:value`,
    /// e.g. `discord:id:1234`. A naive `users/search/keusi` returns
    /// `422 The params field 'query' must contain two colons.` This method adds
    /// the missing middle field for a `provider:value` pair.
    pub fn search_users(&self, query: &str) -> Result<Vec<User>> {
        let raw = query.trim();
        if raw.is_empty() {
            return Err(Error::Invalid {
                field: "query",
                value: String::new(),
                reason: "search query must not be empty",
            });
        }
        // Already qualified: `provider:field:value`.
        let encoded = if raw.matches(':').count() >= 2 {
            raw.to_string()
        } else if let Some((provider, value)) = raw.split_once(':') {
            // `discord:1234` -> `discord:id:1234`
            format!("{provider}:id:{value}")
        } else {
            // Bare text: not a documented route. We surface the API's own
            // answer rather than inventing one.
            raw.to_string()
        };
        let data = self
            .transport
            .get(&format!("users/search/{}", url_segment(&encoded)))?;
        let items = match &data {
            Value::Array(items) => items.as_slice(),
            _ => parse::entries_of(&data),
        };
        Ok(items.iter().map(parse::parse_user).collect())
    }

    /// `/users/:user/records/:gamemode/:leaderboard`
    ///
    /// `leaderboard` accepts the aliases other clients use (`pb`, `global`,
    /// `personal`) and translates them, because the raw values trigger
    /// `422 ... 'leaderboard' is illegal`.
    ///
    /// An empty result is returned as an empty page; use
    /// [`Self::user_records_opts`] with `allow_empty = false` to turn it into
    /// [`Error::EmptyResult`].
    pub fn user_records(
        &self,
        user: &str,
        gamemode: &str,
        leaderboard: &str,
        limit: u32,
    ) -> Result<Page<Record>> {
        self.user_records_opts(user, gamemode, leaderboard, limit, None, true)
    }

    /// Same as [`Self::user_records`] with cursor and empty-result control.
    pub fn user_records_opts(
        &self,
        user: &str,
        gamemode: &str,
        leaderboard: &str,
        limit: u32,
        cursor: Option<(&str, PageDirection)>,
        allow_empty: bool,
    ) -> Result<Page<Record>> {
        let id = routes::normalize_user(user)?;
        let mode = Gamemode::parse(gamemode)?;
        let board = PersonalBoard::parse(leaderboard)?;
        let limit = routes::clamp_limit(limit);

        let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some((value, direction)) = cursor {
            let key = match direction {
                PageDirection::Down => "after",
                PageDirection::Up => "before",
            };
            query.push((key, value.to_string()));
        }

        let path = format!(
            "users/{id}/records/{}/{board}",
            mode.as_str(),
            board = board.as_str()
        );
        let data = self.transport.get_with_query(&path, &query)?;
        let entries = parse::entries_of(&data);
        if entries.is_empty() && !allow_empty {
            return Err(Error::EmptyResult {
                url: self.transport.build_url(&path, &query),
            });
        }
        let records: Vec<Record> = entries.iter().map(parse::parse_record).collect();
        let next_cursor = entries.last().and_then(parse::parse_cursor);
        Ok(Page {
            entries: records,
            next_cursor,
            requested_limit: limit,
        })
    }

    /// `/users/by/:leaderboard`
    pub fn user_leaderboard(
        &self,
        board: &str,
        country: Option<&str>,
        limit: u32,
    ) -> Result<Page<UserLeaderboardEntry>> {
        self.user_leaderboard_opts(board, country, limit, None, true)
    }

    /// Same as [`Self::user_leaderboard`] with cursor and empty-result control.
    pub fn user_leaderboard_opts(
        &self,
        board: &str,
        country: Option<&str>,
        limit: u32,
        cursor: Option<(&str, PageDirection)>,
        allow_empty: bool,
    ) -> Result<Page<UserLeaderboardEntry>> {
        let board = UserBoard::parse(board)?;
        let limit = routes::clamp_limit(limit);
        let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(code) = country.filter(|c| !c.trim().is_empty()) {
            query.push(("country", code.trim().to_ascii_uppercase()));
        }
        if let Some((value, direction)) = cursor {
            let key = match direction {
                PageDirection::Down => "after",
                PageDirection::Up => "before",
            };
            query.push((key, value.to_string()));
        }

        let path = format!("users/by/{}", board.as_str());
        let data = self.transport.get_with_query(&path, &query)?;
        let entries = parse::entries_of(&data);
        if entries.is_empty() && !allow_empty {
            return Err(Error::EmptyResult {
                url: self.transport.build_url(&path, &query),
            });
        }
        let parsed = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| parse::parse_user_leaderboard_entry(entry, index as u64 + 1))
            .collect();
        let next_cursor = entries.last().and_then(parse::parse_cursor);
        Ok(Page {
            entries: parsed,
            next_cursor,
            requested_limit: limit,
        })
    }

    // ---------------------------------------------------------------- records

    /// `/records/:leaderboard`
    ///
    /// Accepts `40l`, `40l_global`, `blitz_country_JP`, `zenith_global@2024w31`
    /// and repairs the bare-gamemode form that upstream silently answers with an
    /// empty list.
    pub fn records(&self, leaderboard: &str, limit: u32) -> Result<Page<Record>> {
        self.records_opts(leaderboard, limit, None, true)
    }

    /// Same as [`Self::records`] with cursor and empty-result control.
    pub fn records_opts(
        &self,
        leaderboard: &str,
        limit: u32,
        cursor: Option<(&str, PageDirection)>,
        allow_empty: bool,
    ) -> Result<Page<Record>> {
        let lb = RecordLeaderboard::parse(leaderboard)?;
        let limit = routes::clamp_limit(limit);
        let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some((value, direction)) = cursor {
            let key = match direction {
                PageDirection::Down => "after",
                PageDirection::Up => "before",
            };
            query.push((key, value.to_string()));
        }
        let path = format!("records/{}", lb.to_id());
        let data = self.transport.get_with_query(&path, &query)?;
        let entries = parse::entries_of(&data);
        if entries.is_empty() && !allow_empty {
            return Err(Error::EmptyResult {
                url: self.transport.build_url(&path, &query),
            });
        }
        Ok(Page {
            entries: entries.iter().map(parse::parse_record).collect(),
            next_cursor: entries.last().and_then(parse::parse_cursor),
            requested_limit: limit,
        })
    }

    // ------------------------------------------------------------------- labs

    /// `/labs/scoreflow/:user/:gamemode`
    pub fn score_flow(&self, user: &str, gamemode: &str) -> Result<ScoreFlow> {
        let id = routes::normalize_user(user)?;
        let mode = Gamemode::parse(gamemode)?;
        let data = self
            .transport
            .get(&format!("labs/scoreflow/{id}/{}", mode.as_str()))?;
        Ok(parse::parse_score_flow(&data))
    }

    /// `/labs/leagueflow/:user`
    pub fn league_flow(&self, user: &str) -> Result<ScoreFlow> {
        let id = routes::normalize_user(user)?;
        let data = self.transport.get(&format!("labs/leagueflow/{id}"))?;
        Ok(parse::parse_score_flow(&data))
    }

    /// `/labs/league_ranks`
    pub fn league_ranks(&self) -> Result<LeagueRanks> {
        let data = self.transport.get("labs/league_ranks")?;
        Ok(parse::parse_league_ranks(&data))
    }

    // --------------------------------------------------------------- raw pass

    /// Escape hatch: any path, fully encoded, envelope unwrapped.
    ///
    /// Use this for endpoints that gain fields before this crate learns about
    /// them. The result is already normalized in the sense that errors are
    /// typed and HTML pages are rejected.
    pub fn raw_get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let owned: Vec<(&str, String)> = query
            .iter()
            .map(|(k, v)| (*k, (*v).to_string()))
            .collect();
        self.transport.get_with_query(path, &owned)
    }
}

/// Percent-encode characters that would otherwise change the URL path shape.
///
/// Segment separators are the only thing we must encode here; `:` is legal in a
/// path segment and the search route depends on it.
fn url_segment(input: &str) -> String {
    input
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('?', "%3F")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_boards_are_parsed_forgivingly() {
        assert_eq!(UserBoard::parse("League").unwrap(), UserBoard::League);
        assert_eq!(UserBoard::parse("XP").unwrap(), UserBoard::Xp);
        assert_eq!(
            UserBoard::parse("Achievement Rating").unwrap(),
            UserBoard::Ar
        );
        assert!(UserBoard::parse("region").is_err());
    }

    #[test]
    fn search_segment_encoding_protects_path_shape() {
        assert_eq!(url_segment("keusi dan"), "keusi%20dan");
        assert_eq!(url_segment("discord:id:123"), "discord:id:123");
        assert_eq!(url_segment("a?b"), "a%3Fb");
    }
}
