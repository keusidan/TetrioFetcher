//! Raw JSON -> normalized model conversion.
//!
//! Every function here is total: malformed or missing fields degrade to `None`
//! or a default instead of failing the whole response. That is deliberate. A
//! single renamed upstream field should cost you one field, not the request.

use serde_json::Value;

use crate::jval;
use crate::model::*;

/// Parse a user blob from `/users/:user`.
pub fn parse_user(data: &Value) -> User {
    User {
        id: jval::as_str(data, "_id").unwrap_or_default(),
        username: jval::as_str(data, "username").unwrap_or_default(),
        // `role` is occasionally absent on pruned accounts; treat as anon.
        role: jval::as_str(data, "role").unwrap_or_else(|| "anon".to_string()),
        registered_at: jval::as_str(data, "ts"),
        xp: jval::as_f64(data, "xp").unwrap_or(0.0),
        games_played: jval::as_stat(data, "gamesplayed"),
        games_won: jval::as_stat(data, "gameswon"),
        gametime_seconds: jval::as_stat_f64(data, "gametime"),
        country: jval::as_str(data, "country").filter(|c| !c.is_empty()),
        bad_standing: jval::as_bool(data, "badstanding"),
        supporter: jval::as_bool(data, "supporter").unwrap_or(false),
        supporter_tier: jval::as_i64(data, "supporter_tier").unwrap_or(0),
        friend_count: jval::as_i64(data, "friend_count").unwrap_or(0),
        achievement_rating: jval::as_i64(data, "ar").unwrap_or(0),
        achievement_counts: parse_achievement_counts(data),
        badges: parse_badges(data),
        connections: parse_connections(data),
    }
}

fn parse_achievement_counts(data: &Value) -> AchievementCounts {
    let counts = jval::get(data, "ar_counts").cloned().unwrap_or(Value::Null);
    let get = |key: &str| jval::as_i64(&counts, key).unwrap_or(0);
    AchievementCounts {
        bronze: get("1"),
        silver: get("2"),
        gold: get("3"),
        platinum: get("4"),
        diamond: get("5"),
        issued: get("100"),
        top100: get("t100"),
        top50: get("t50"),
        top25: get("t25"),
        top10: get("t10"),
        top5: get("t5"),
        top3: get("t3"),
    }
}

fn parse_badges(data: &Value) -> Vec<Badge> {
    let Some(Value::Array(items)) = jval::get(data, "badges") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|badge| {
            Some(Badge {
                id: jval::as_str(badge, "id")?,
                label: jval::as_str(badge, "label").unwrap_or_default(),
                group: jval::as_str(badge, "group").filter(|g| !g.is_empty()),
                // Upstream writes `ts: false` for badges with no timestamp.
                granted_at: jval::as_str(badge, "ts").filter(|t| t != "false"),
            })
        })
        .collect()
}

fn parse_connections(data: &Value) -> std::collections::BTreeMap<String, Connection> {
    let mut out = std::collections::BTreeMap::new();
    let Some(Value::Object(map)) = jval::get(data, "connections") else {
        return out;
    };
    for (provider, value) in map {
        if value.is_null() {
            continue;
        }
        // Different providers ship different key sets; keep what is present.
        let id = jval::as_str(value, "id")
            .or_else(|| jval::as_str(value, "userid"))
            .unwrap_or_default();
        let username = jval::as_str(value, "username").unwrap_or_default();
        out.insert(
            provider.clone(),
            Connection {
                id,
                username,
                display_name: jval::as_str(value, "display_username").filter(|s| !s.is_empty()),
            },
        );
    }
    out
}

/// Parse a league standing from `league` / `summaries/league`.
pub fn parse_league_standing(data: &Value) -> LeagueStanding {
    LeagueStanding {
        games_played: jval::as_i64(data, "gamesplayed").unwrap_or(0),
        games_won: jval::as_i64(data, "gameswon").unwrap_or(0),
        tr: jval::as_rating(data, "tr"),
        gxe: jval::as_rating(data, "gxe"),
        rank: jval::as_rank(data, "rank"),
        best_rank: jval::as_rank(data, "bestrank"),
        glicko: jval::as_rating(data, "glicko"),
        rd: jval::as_rating(data, "rd"),
        apm: jval::as_f64(data, "apm"),
        pps: jval::as_f64(data, "pps"),
        vs: jval::as_f64(data, "vs"),
        decaying: jval::as_bool(data, "decaying").unwrap_or(false),
    }
}

/// Parse one entry of `/users/by/:leaderboard`.
///
/// `placement` is injected by the caller because the API does not send it.
pub fn parse_user_leaderboard_entry(data: &Value, placement: u64) -> UserLeaderboardEntry {
    let league = jval::get(data, "league").map(parse_league_standing);
    UserLeaderboardEntry {
        placement,
        user_id: jval::as_str(data, "_id").unwrap_or_default(),
        username: jval::as_str(data, "username").unwrap_or_default(),
        role: jval::as_str(data, "role").unwrap_or_else(|| "anon".to_string()),
        xp: jval::as_f64(data, "xp"),
        achievement_rating: jval::as_i64(data, "ar"),
        league,
        country: jval::as_str(data, "country").filter(|c| !c.is_empty()),
        supporter: jval::as_bool(data, "supporter").unwrap_or(false),
        games_played: jval::as_stat(data, "gamesplayed"),
        games_won: jval::as_stat(data, "gameswon"),
        gametime_seconds: jval::as_stat_f64(data, "gametime"),
        cursor: parse_cursor(data),
    }
}

/// Render the `p.{pri,sec,ter}` cursor as the `pri:sec:ter` string the API takes.
pub fn parse_cursor(data: &Value) -> Option<String> {
    let p = jval::get(data, "p")?;
    let pri = jval::as_f64(p, "pri")?;
    let sec = jval::as_f64(p, "sec").unwrap_or(0.0);
    let ter = jval::as_f64(p, "ter").unwrap_or(0.0);
    Some(format!("{pri}:{sec}:{ter}"))
}

/// Parse a record from `/records/:leaderboard` or a personal-records list.
pub fn parse_record(data: &Value) -> Record {
    let results = jval::get(data, "results").cloned().unwrap_or(Value::Null);
    Record {
        id: jval::as_str(data, "_id").unwrap_or_default(),
        replay_id: jval::as_str(data, "replayid").filter(|r| !r.is_empty()),
        gamemode: jval::as_str(data, "gamemode").unwrap_or_default(),
        personal_best: jval::as_bool(data, "pb").unwrap_or(false),
        was_personal_best: jval::as_bool(data, "oncepb").unwrap_or(false),
        played_at: jval::as_str(data, "ts"),
        revolution: jval::as_str(data, "revolution").filter(|r| !r.is_empty()),
        user: jval::get(data, "user").map(parse_record_user),
        other_users: parse_record_users(data, "otherusers"),
        leaderboards: match jval::get(data, "leaderboards") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        },
        stats: parse_game_stats(&results),
        aggregate: parse_aggregate(&results),
        raw_results: (!results.is_null()).then_some(results),
    }
}

fn parse_record_user(data: &Value) -> RecordUser {
    RecordUser {
        id: jval::as_str(data, "id")
            .or_else(|| jval::as_str(data, "_id"))
            .unwrap_or_default(),
        username: jval::as_str(data, "username").unwrap_or_default(),
        country: jval::as_str(data, "country").filter(|c| !c.is_empty()),
        supporter: jval::as_bool(data, "supporter").unwrap_or(false),
    }
}

fn parse_record_users(data: &Value, key: &str) -> Vec<RecordUser> {
    match jval::get(data, key) {
        Some(Value::Array(items)) => items.iter().map(parse_record_user).collect(),
        _ => Vec::new(),
    }
}

fn parse_aggregate(results: &Value) -> Option<AggregateStats> {
    let agg = jval::get(results, "aggregatestats")?;
    Some(AggregateStats {
        apm: jval::as_f64(agg, "apm")?,
        pps: jval::as_f64(agg, "pps")?,
        // Versus records call it `vsscore`; some payloads call it `vs`.
        vs: jval::as_f64(agg, "vsscore").or_else(|| jval::as_f64(agg, "vs"))?,
    })
}

fn parse_game_stats(results: &Value) -> GameStats {
    let Some(stats) = jval::get(results, "stats") else {
        return GameStats::default();
    };
    let clears = jval::get(stats, "clears").cloned().unwrap_or(Value::Null);
    GameStats {
        lines: jval::as_i64(stats, "lines"),
        score: jval::as_i64(stats, "score"),
        pieces_placed: jval::as_i64(stats, "piecesplaced"),
        inputs: jval::as_i64(stats, "inputs"),
        holds: jval::as_i64(stats, "holds"),
        level: jval::as_i64(stats, "level"),
        top_combo: jval::as_i64(stats, "topcombo"),
        top_btb: jval::as_i64(stats, "topbtb"),
        tspins: jval::as_i64(stats, "tspins"),
        quads: jval::as_i64(&clears, "quads"),
        // The server sometimes includes `pps` in `stats`, sometimes only in
        // `aggregatestats`; take whichever is present.
        pps: jval::as_f64(stats, "pps"),
        raw: Some(stats.clone()),
    }
}

/// Parse `/labs/scoreflow/:user/:gamemode`.
pub fn parse_score_flow(data: &Value) -> ScoreFlow {
    let start_time = jval::as_millis(data, "startTime").map(|ms| (ms / 1000) as i64);
    let points = match jval::get(data, "points") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|point| {
                // Each point is a positional tuple `[offset, is_pb, score]`.
                let arr = point.as_array()?;
                let offset = arr.first().and_then(|v| v.as_i64()).unwrap_or(0);
                let is_pb = arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0) == 1;
                let score = arr.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
                let base = start_time.unwrap_or(0);
                Some(ScoreFlowPoint {
                    timestamp: base + offset,
                    is_pb,
                    score,
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    ScoreFlow { start_time, points }
}

/// Parse `/labs/league_ranks`.
///
/// The payload is doubly nested (`data._id`, `data.data.{...}`), which differs
/// from every other endpoint. `bands` is sorted by position so callers do not
/// have to know the API's key order.
pub fn parse_league_ranks(data: &Value) -> LeagueRanks {
    let id = jval::as_str(data, "_id").unwrap_or_default();
    let inner = jval::get(data, "data").cloned().unwrap_or(Value::Null);
    let total = jval::as_i64(&inner, "total").unwrap_or(0);
    let mut bands: Vec<RankBand> = match inner.as_object() {
        Some(map) => map
            .iter()
            .filter(|(key, _)| key.as_str() != "total")
            .map(|(rank, value)| RankBand {
                rank: rank.clone(),
                position: jval::as_i64(value, "pos").unwrap_or(0),
                percentile: jval::as_f64(value, "percentile"),
                tr: jval::as_rating(value, "tr"),
                target_tr: jval::as_rating(value, "targettr"),
                count: jval::as_i64(value, "count").unwrap_or(0),
                apm: jval::as_f64(value, "apm"),
                pps: jval::as_f64(value, "pps"),
                vs: jval::as_f64(value, "vs"),
            })
            .collect(),
        None => Vec::new(),
    };
    bands.sort_by_key(|b| b.position);
    LeagueRanks { id, total, bands }
}

/// Parse `/general/stats`.
pub fn parse_server_stats(data: &Value) -> ServerStats {
    ServerStats {
        user_count: jval::as_i64(data, "usercount").unwrap_or(0),
        user_count_delta: jval::as_f64(data, "usercount_delta").unwrap_or(0.0),
        anon_count: jval::as_i64(data, "anoncount").unwrap_or(0),
        total_accounts: jval::as_i64(data, "totalaccounts").unwrap_or(0),
        ranked_count: jval::as_i64(data, "rankedcount").unwrap_or(0),
        record_count: jval::as_i64(data, "recordcount").unwrap_or(0),
        games_played: jval::as_i64(data, "gamesplayed").unwrap_or(0),
        games_played_delta: jval::as_f64(data, "gamesplayed_delta").unwrap_or(0.0),
        games_finished: jval::as_i64(data, "gamesfinished").unwrap_or(0),
        gametime_seconds: jval::as_f64(data, "gametime").unwrap_or(0.0),
        inputs: jval::as_i64(data, "inputs").unwrap_or(0),
        pieces_placed: jval::as_i64(data, "piecesplaced").unwrap_or(0),
        average_pps: None,
        average_kps: None,
    }
    .with_derived()
}

/// Extract `entries` from a list response.
pub fn entries_of(data: &Value) -> &[Value] {
    match jval::get(data, "entries") {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hidden_user() -> Value {
        json!({
            "_id": "6667cfa451ddfaaf19734af5",
            "username": "keusidan",
            "role": "anon",
            "ts": "2024-06-11T04:16:36.021Z",
            "badges": [],
            "xp": 22336,
            "gamesplayed": -1,
            "gameswon": -1,
            "gametime": -1.0,
            "country": null,
            "badstanding": false,
            "supporter": false,
            "supporter_tier": 0,
            "connections": {},
            "friend_count": 0,
            "achievements": [],
            "ar": 0,
            "ar_counts": {},
            "oldusernames": []
        })
    }

    #[test]
    fn hidden_totals_do_not_become_zero_or_minus_one() {
        let user = parse_user(&hidden_user());
        // `-1` means hidden. Reporting `-1` or `0` would both be lies.
        assert_eq!(user.games_played, None);
        assert_eq!(user.games_won, None);
        assert_eq!(user.gametime_seconds, None);
        assert_eq!(user.xp, 22336.0);
        assert_eq!(user.country, None);
    }

    #[test]
    fn float_xp_is_accepted() {
        let mut u = hidden_user();
        u["xp"] = json!(68966508.61132418_f64);
        assert!((parse_user(&u).xp - 68966508.61132418).abs() < 1e-6);
    }

    #[test]
    fn unranked_league_is_all_none() {
        let raw = json!({
            "gamesplayed": 0, "gameswon": 0, "glicko": -1, "rd": -1, "gxe": -1,
            "tr": -1, "rank": "z", "decaying": false, "standing": -1,
            "percentile": -1, "percentile_rank": "z"
        });
        let standing = parse_league_standing(&raw);
        assert_eq!(standing.tr, None);
        assert_eq!(standing.rank, None);
        assert_eq!(standing.glicko, None);
        assert_eq!(standing.gxe, None);
        assert_eq!(standing.games_played, 0);
        assert_eq!(standing.recent_stats(), None);
    }

    #[test]
    fn ranked_league_keeps_values() {
        let raw = json!({
            "gamesplayed": 1711, "gameswon": 1471, "glicko": 4662.16, "rd": 99.99,
            "tr": 24863.06, "gxe": 99.99, "rank": "x+", "bestrank": "x+",
            "apm": 170.6, "pps": 2.73, "vs": 353.52, "decaying": false
        });
        let standing = parse_league_standing(&raw);
        assert_eq!(standing.tr, Some(24863.06));
        assert_eq!(standing.rank.as_deref(), Some("x+"));
        let stats = standing.recent_stats().unwrap();
        assert!((stats.app().unwrap() - 170.6 / 2.73 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn cursor_is_triple_encoded() {
        let raw = json!({"p": {"pri": 24863.062055852624, "sec": 0, "ter": 0}});
        assert_eq!(
            parse_cursor(&raw).as_deref(),
            Some("24863.062055852624:0:0")
        );
    }

    #[test]
    fn badges_with_false_timestamp_lose_the_timestamp() {
        let mut u = hidden_user();
        u["badges"] = json!([
            {"id": "supporter", "label": "Supporter", "group": "supporter", "ts": false},
            {"id": "x", "label": "X", "group": "y", "ts": "2024-01-01T00:00:00.000Z"}
        ]);
        let badges = parse_user(&u).badges;
        assert_eq!(badges[0].granted_at, None);
        assert_eq!(
            badges[1].granted_at.as_deref(),
            Some("2024-01-01T00:00:00.000Z")
        );
    }

    #[test]
    fn record_parses_single_player_shape() {
        let raw = json!({
            "_id": "6aa95b6f2b2123035d867734",
            "replayid": "25bf3326f56d",
            "stub": false,
            "gamemode": "40l",
            "pb": true,
            "oncepb": true,
            "ts": "2026-09-15T14:51:27.844Z",
            "revolution": null,
            "user": {"id": "65d869b285867cbaf9a8bf82", "username": "kwikscoper",
                     "avatar_revision": 1789496002468_i64, "banner_revision": 0,
                     "country": "AE", "supporter": true},
            "otherusers": [],
            "leaderboards": ["40l_global", "40l_country_AE"],
            "results": {
                "aggregatestats": {"apm": 0, "pps": 7.547169811320755, "vsscore": 0},
                "stats": {"lines": 40, "score": 17216, "piecesplaced": 100, "inputs": 272,
                          "holds": 30, "topcombo": 3, "topbtb": 6, "tspins": 0, "level": 1,
                          "clears": {"quads": 9}}
            }
        });
        let record = parse_record(&raw);
        assert_eq!(record.gamemode, "40l");
        assert!(record.personal_best);
        assert_eq!(record.user.as_ref().unwrap().username, "kwikscoper");
        assert_eq!(record.leaderboards.len(), 2);
        assert_eq!(record.stats.quads, Some(9));
        assert_eq!(record.stats.pieces_placed, Some(100));
        // `vsscore` must be normalized to `vs`.
        assert_eq!(record.aggregate.unwrap().vs, 0.0);
        assert_eq!(record.revolution, None);
    }

    #[test]
    fn record_without_results_does_not_fail() {
        let raw = json!({"_id": "x", "gamemode": "40l"});
        let record = parse_record(&raw);
        assert_eq!(record.stats.lines, None);
        assert!(record.user.is_none());
    }

    #[test]
    fn scoreflow_offsets_become_timestamps_and_sentinel_is_dropped() {
        let raw = json!({
            "startTime": 1_600_000_000_000_i64,
            "points": [[0, 0, -30000], [3600, 1, -25000]]
        });
        let flow = parse_score_flow(&raw);
        assert_eq!(flow.start_time, Some(1_600_000_000));
        assert_eq!(flow.points[0].timestamp, 1_600_000_000);
        assert_eq!(flow.points[1].timestamp, 1_600_003_600);
        assert!(flow.points[1].is_pb);
        assert_eq!(flow.points[1].score, -25000);

        // The empty-feed sentinel must not leak out as a date.
        let empty =
            parse_score_flow(&json!({"startTime": 9_007_199_254_740_991_i64, "points": []}));
        assert_eq!(empty.start_time, None);
        assert!(empty.points.is_empty());
    }

    #[test]
    fn league_ranks_double_nesting_is_unwrapped_and_sorted() {
        let raw = json!({
            "_id": "6ad676c25852578e3e46b9f6",
            "data": {
                "total": 36229,
                "x+": {"pos": 72, "percentile": 0.002, "tr": 24164.1, "targettr": 24000,
                        "apm": 180.1, "pps": 3.43, "vs": 358.5, "count": 78},
                "x": {"pos": 362, "percentile": 0.01, "tr": 22879.5, "targettr": 22500,
                       "count": 286},
                "u": {"pos": 1811, "percentile": 0.05, "tr": 20458.2, "targettr": 20000,
                       "count": 1458}
            }
        });
        let ranks = parse_league_ranks(&raw);
        assert_eq!(ranks.total, 36229);
        assert_eq!(ranks.bands.len(), 3);
        assert_eq!(ranks.bands[0].rank, "x+");
        assert_eq!(ranks.bands[2].rank, "u");
        assert_eq!(ranks.bands[0].position, 72);
        assert_eq!(ranks.bands[1].apm, None, "absent apm stays None");
    }

    #[test]
    fn server_stats_derives_rates() {
        let raw = json!({
            "usercount": 9832113, "usercount_delta": 0.1989, "anoncount": 6377072,
            "totalaccounts": 27396208, "rankedcount": 36218, "recordcount": 216393767,
            "gamesplayed": 1098173835, "gamesplayed_delta": 5.37, "gamesfinished": 875846392,
            "gametime": 188377554846.7591, "inputs": 712872180654_i64, "piecesplaced": 166798284484_i64
        });
        let stats = parse_server_stats(&raw);
        assert_eq!(stats.user_count, 9832113);
        let pps = stats.average_pps.unwrap();
        assert!(pps > 0.8 && pps < 1.0, "server-wide pps sanity: {pps}");
        assert!(stats.average_kps.unwrap() > pps);
    }

    #[test]
    fn missing_entries_field_yields_empty_slice() {
        assert!(entries_of(&json!({"entries": []})).is_empty());
        assert!(entries_of(&json!({})).is_empty());
        assert_eq!(entries_of(&json!({"entries": [1, 2]})).len(), 2);
    }
}
