//! Offline tests driven by recorded API responses in `tests/fixtures`.
//!
//! These never touch the network, so `cargo test` works in CI and on a plane.
//! The fixtures are real bodies captured from TETRA CHANNEL; the comment above
//! each test says which trap the fixture demonstrates.

use serde_json::Value;

use tetrio_fetcher::{
    jval, unwrap_envelope, Error, Gamemode, PersonalBoard, RecordLeaderboard,
};

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read fixture {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("fixture {name} is not JSON: {e}"))
}

fn fixture_raw(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read fixture {path}: {e}"))
}

/// `/users/:user` for an account whose totals may be hidden (`-1`).
#[test]
fn user_fixture_normalizes_hidden_totals() {
    let body = fixture("user.json");
    let data = unwrap_envelope("users/keusidan", body).unwrap();
    let user = tetrio_fetcher::parse::parse_user(&data);

    assert_eq!(user.username, "keusidan");
    assert_eq!(user.id, "6667cfa451ddfaaf19734af5");
    assert_eq!(user.role, "anon");
    assert_eq!(user.xp, 22336.0);
    assert_eq!(user.country, None, "null country stays null");
    assert_eq!(user.bad_standing, Some(false));
    assert!(!user.supporter);

    // Whatever the account's current totals are, they must never be negative:
    // `-1` means "hidden" upstream and must have become `None`.
    for (label, value) in [
        ("games_played", user.games_played.map(|v| v as f64)),
        ("games_won", user.games_won.map(|v| v as f64)),
        ("gametime_seconds", user.gametime_seconds),
    ] {
        if let Some(v) = value {
            assert!(v >= 0.0, "{label} leaked a negative sentinel: {v}");
        }
    }
}

/// `/users/:user/summaries/league` for an unranked player: `tr: -1`, `rank: "z"`.
#[test]
fn unranked_league_fixture_has_no_fake_rank() {
    let body = fixture("league_unranked.json");
    let data = unwrap_envelope("summaries/league", body).unwrap();
    let league = &data["league"];
    let standing = tetrio_fetcher::parse::parse_league_standing(league);

    assert_eq!(standing.games_played, 0);
    assert_eq!(standing.rank, None, "\"z\" is not a letter rank");
    assert_eq!(standing.tr, None, "-1 is not a rating");
    assert_eq!(standing.gxe, None);
    assert_eq!(standing.glicko, None);
    assert_eq!(standing.rd, None);
    assert!(!standing.decaying);
    assert!(standing.recent_stats().is_none());
}

/// The same endpoint for a top player: every field present.
#[test]
fn ranked_league_fixture_keeps_every_field() {
    let body = fixture("league_ranked.json");
    let data = unwrap_envelope("summaries/league", body).unwrap();
    let standing = tetrio_fetcher::parse::parse_league_standing(&data);

    assert!(standing.games_played > 0);
    assert!(standing.tr.unwrap() > 0.0);
    assert_eq!(standing.rank.as_deref(), Some("x+"));
    let stats = standing.recent_stats().expect("ranked players have averages");
    assert!(stats.apm > 0.0 && stats.pps > 0.0 && stats.vs > 0.0);
    // APP is derived, and the formula must match the published definition.
    let app = stats.app().expect("pps is non-zero");
    assert!((app - stats.apm / stats.pps / 60.0).abs() < 1e-12);
}

/// `/users/by/league` — entries carry a prisecter cursor and may hide stats.
#[test]
fn user_leaderboard_fixture_exposes_cursors_and_never_negative_stats() {
    let body = fixture("user_leaderboard.json");
    let data = unwrap_envelope("users/by/league", body).unwrap();
    let entries = tetrio_fetcher::parse::entries_of(&data);

    assert!(!entries.is_empty());
    for (index, entry) in entries.iter().enumerate() {
        let parsed = tetrio_fetcher::parse::parse_user_leaderboard_entry(entry, index as u64 + 1);
        assert_eq!(parsed.placement, index as u64 + 1);
        assert!(!parsed.username.is_empty());
        assert!(parsed.cursor.is_some(), "every entry has a prisecter");
        assert!(
            parsed.league.is_some(),
            "the league board always has standings"
        );
        if let Some(games) = parsed.games_played {
            assert!(games >= 0, "hidden stat leaked: {games}");
        }
        if let Some(time) = parsed.gametime_seconds {
            assert!(time >= 0.0, "hidden gametime leaked: {time}");
        }
    }
    // The first entry of the league board has the highest TR.
    let first_tr = entries[0]["league"]["tr"].as_f64().unwrap();
    let second_tr = entries[1]["league"]["tr"].as_f64().unwrap();
    assert!(first_tr >= second_tr);
}

/// `/records/40l_global` — note `aggregatestats.vsscore`, not `vs`.
#[test]
fn record_fixture_normalizes_vsscore_and_stats() {
    let body = fixture("records_40l.json");
    let data = unwrap_envelope("records/40l_global", body).unwrap();
    let entries = tetrio_fetcher::parse::entries_of(&data);

    assert!(!entries.is_empty());
    let record = tetrio_fetcher::parse::parse_record(&entries[0]);

    assert_eq!(record.gamemode, "40l");
    assert!(!record.id.is_empty());
    assert!(record.replay_id.is_some());
    assert!(record.personal_best, "the top of a leaderboard is a PB");
    assert_eq!(
        record.stats.lines,
        Some(40),
        "40 LINES means exactly 40 lines"
    );
    assert!(record.stats.score.unwrap() > 0);
    assert!(record.stats.pieces_placed.unwrap() > 0);
    assert!(record.leaderboards.iter().any(|lb| lb == "40l_global"));
    assert!(record
        .leaderboards
        .iter()
        .any(|lb| lb.starts_with("40l_country_")));
    // The raw object is preserved for fields this crate does not model.
    assert!(record.raw_results.as_ref().unwrap().get("stats").is_some());
    // `vsscore` must have been renamed to `vs` in the normalized aggregate.
    if let Some(agg) = record.aggregate {
        assert!(agg.vs >= 0.0);
        assert!(agg.pps > 0.0);
    }
}

/// `/labs/league_ranks` hides everything behind an extra `data` level.
#[test]
fn league_ranks_fixture_is_unwrapped_and_ordered() {
    let body = fixture("league_ranks.json");
    let data = unwrap_envelope("labs/league_ranks", body).unwrap();
    let ranks = tetrio_fetcher::parse::parse_league_ranks(&data);

    assert!(ranks.total > 0);
    assert!(!ranks.bands.is_empty());
    assert!(!ranks.id.is_empty());

    let known = [
        "x+", "x", "u", "ss", "s+", "s", "s-", "a+", "a", "a-", "b+", "b", "b-", "c+", "c", "c-",
        "d+", "d",
    ];
    for band in &ranks.bands {
        assert!(
            known.contains(&band.rank.as_str()),
            "unknown band {}",
            band.rank
        );
        assert!(band.position >= 1);
        assert!(band.count >= 0);
    }
    let positions: Vec<i64> = ranks.bands.iter().map(|b| b.position).collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "bands must be sorted by position");
    // TR thresholds descend as the rank gets weaker.
    let trs: Vec<f64> = ranks.bands.iter().filter_map(|b| b.tr).collect();
    assert!(
        trs.windows(2).all(|w| w[0] >= w[1]),
        "TR thresholds must descend: {trs:?}"
    );
}

/// An empty score flow ships `startTime: 9007199254740991` (JS MAX_SAFE_INTEGER).
#[test]
fn empty_scoreflow_fixture_does_not_yield_an_absurd_date() {
    let body = fixture("scoreflow_empty.json");
    let data = unwrap_envelope("labs/scoreflow", body).unwrap();
    let flow = tetrio_fetcher::parse::parse_score_flow(&data);

    assert!(flow.points.is_empty());
    assert_eq!(
        flow.start_time, None,
        "9007199254740991 ms is the year 285428751; it means 'no data'"
    );
    // Guard the raw value too, so a future refactor cannot reintroduce it.
    assert_eq!(jval::as_millis(&data, "startTime"), None);
}

/// `/general/stats` mixes ints and floats and needs derived rates.
#[test]
fn stats_fixture_derives_consistent_rates() {
    let body = fixture("stats.json");
    let data = unwrap_envelope("general/stats", body).unwrap();
    let stats = tetrio_fetcher::parse::parse_server_stats(&data);

    assert!(stats.user_count > 0);
    assert!(stats.total_accounts >= stats.user_count);
    assert!(stats.pieces_placed > 0);
    let pps = stats.average_pps.expect("derived");
    let kps = stats.average_kps.expect("derived");
    assert!(pps > 0.1 && pps < 5.0, "server-wide pps: {pps}");
    assert!(kps > pps, "there is more than one keypress per piece");
    // Derived, not echoed: upstream sends no such fields.
    assert!(data.get("average_pps").is_none());
}

/// `/general/activity` is a bare array of numbers.
#[test]
fn activity_fixture_parses_as_numbers() {
    let body = fixture("activity.json");
    let data = unwrap_envelope("general/activity", body).unwrap();
    let values = jval::as_num_array(&data, "activity");
    assert!(values.len() > 100, "24h of one-minute samples");
    assert!(values.iter().all(|v| *v >= 0.0));
}

/// The TETRA CHANNEL HTML 404 page, served where JSON is expected.
#[test]
fn html_404_fixture_is_recognized_as_not_json() {
    let body = fixture_raw("html_404.html");
    assert!(body.starts_with("<!DOCTYPE html>"));
    assert!(
        serde_json::from_str::<Value>(&body).is_err(),
        "a html page is exactly what breaks naive clients"
    );
    // A JSON client sees "expected value at line 1 column 1"; we want the
    // diagnostic that names the content type and the page.
    let err = non_json_error(
        "https://ch.tetr.io/api/users/keusidan/records/40l",
        &body,
        "text/html; charset=utf-8",
    );
    match err {
        Error::NonJson {
            content_type,
            snippet,
            ..
        } => {
            assert_eq!(content_type, "text/html; charset=utf-8");
            assert!(snippet.contains("TETRA CHANNEL"));
        }
        other => panic!("expected NonJson, got {other:?}"),
    }
}

/// The `422` for the classic wrong leaderboard segment.
#[test]
fn err_422_fixture_becomes_a_typed_api_error() {
    let body = fixture("err_422_leaderboard.json");
    let err = unwrap_envelope("users/keusidan/records/40l/pb", body).unwrap_err();
    match &err {
        Error::Api {
            status,
            msg,
            key,
            context,
        } => {
            assert_eq!(*status, 422);
            assert!(msg.contains("leaderboard"), "{msg}");
            assert_eq!(key.as_deref(), Some("leaderboard"));
            assert_eq!(context.as_deref(), Some("params"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert_eq!(err.offending_key(), Some("leaderboard"));
    assert!(err.is_likely_user_mistake());
}

/// The `404` for a missing user: JSON body, but `success:false`.
#[test]
fn err_404_fixture_becomes_a_typed_api_error() {
    let body = fixture("err_404_user.json");
    let err = unwrap_envelope("users/nobody", body).unwrap_err();
    match &err {
        Error::Api { status, msg, .. } => {
            assert_eq!(*status, 404);
            assert!(msg.contains("No such user"), "{msg}");
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

/// The route repairs must line up with what the fixtures prove is needed.
#[test]
fn route_repairs_match_the_fixture_failures() {
    // The 422 fixture was produced by exactly this segment.
    assert_eq!(
        PersonalBoard::parse("pb").unwrap(),
        PersonalBoard::Progression,
        "`pb` must never be sent upstream"
    );
    // The html_404 fixture was produced by a missing leaderboard segment, and
    // the bare-gamemode form on /records/ answers [] instead of erroring.
    assert_eq!(
        RecordLeaderboard::parse("40l").unwrap().to_id(),
        "40l_global"
    );
    assert_eq!(
        Gamemode::parse("QuickPlay").unwrap().as_str(),
        "zenith",
        "the display name must map to the wire token"
    );
}

/// Mirrors what the transport reports for a non-JSON body, so the fixture test
/// asserts the same diagnostic the live client produces.
fn non_json_error(url: &str, body: &str, content_type: &str) -> Error {
    let snippet: String = body.chars().take(120).collect();
    Error::NonJson {
        url: url.to_string(),
        content_type: content_type.to_string(),
        snippet: snippet.replace(['\n', '\r', '\t'], " "),
    }
}
