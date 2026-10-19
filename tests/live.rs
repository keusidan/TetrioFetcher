//! Live smoke tests against the real TETRA CHANNEL API.
//!
//! These are the regression tests for every quirk this crate claims to mask.
//! Each one would fail against a naive client.
//!
//! They are `#[ignore]` by default so `cargo test` works offline and in CI
//! without depending on a third-party service. Run them deliberately:
//!
//! ```text
//! cargo test --test live -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! They use `--test-threads=1` because the client throttles to ~1 req/s.

use tetrio_fetcher::{Error, Fetcher, PageDirection, TransportConfig};

fn fetcher() -> Fetcher {
    Fetcher::with_config(TransportConfig {
        // Tests hammer a few endpoints; keep the cache on so repeated reads of
        // the same URL are free, exactly as production use does.
        honour_server_cache: true,
        ..Default::default()
    })
}

/// Trap: `/users/Keusidan` and `/users/KEUSIDAN` both answer 404 because the
/// route is case sensitive.
#[test]
#[ignore = "network"]
fn username_case_is_not_a_user_not_found() {
    let f = fetcher();
    let lower = f.user("keusidan").expect("lowercase works upstream");
    let upper = f
        .user("KEUSIDAN")
        .expect("the fetcher should lowercase before calling");
    let mixed = f.user("Keusidan").expect("mixed case should work too");
    assert_eq!(lower.username, upper.username);
    assert_eq!(lower.username, mixed.username);
    assert_eq!(lower.id, mixed.id);
}

/// Trap: `pb` is not a valid leaderboard segment; upstream answers
/// `422 The params field 'leaderboard' is illegal`.
#[test]
#[ignore = "network"]
fn pb_is_translated_to_progression() {
    let f = fetcher();
    for alias in ["pb", "pbs", "personal", "personalbest", "progression"] {
        let page = f
            .user_records("keusidan", "40l", alias, 5)
            .unwrap_or_else(|e| panic!("`{alias}` should be repaired, got {e}"));
        assert!(page.requested_limit <= 100);
    }
}

#[test]
#[ignore = "network"]
fn genuinely_bad_leaderboard_name_is_rejected_locally() {
    let f = fetcher();
    let err = f
        .user_records("keusidan", "40l", "nonsense", 5)
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Invalid {
                field: "leaderboard",
                ..
            }
        ),
        "expected a local Invalid error, got {err:?}"
    );
    // Nothing was sent upstream for this one.
    assert!(err.is_likely_user_mistake());
}

/// Trap: `/records/40l` answers `200` with `entries: []` instead of an error.
#[test]
#[ignore = "network"]
fn bare_gamemode_leaderboard_is_expanded_not_empty() {
    let f = fetcher();
    let bare = f.records("40l", 3).expect("bare gamemode should be repaired");
    let explicit = f.records("40l_global", 3).expect("explicit id");

    assert!(!bare.entries.is_empty(), "upstream returns [] for `40l`");
    assert_eq!(bare.entries.len(), explicit.entries.len());
    assert_eq!(bare.entries[0].id, explicit.entries[0].id);
    assert_eq!(bare.entries[0].gamemode, "40l");
}

#[test]
#[ignore = "network"]
fn country_and_revolution_ids_are_canonicalized() {
    let f = fetcher();
    // Short form `40l_jp` must mean `40l_country_JP`, which is what upstream
    // expects; passing `40l_jp` straight through would return [].
    let page = f.records("40l_jp", 2).expect("country short form");
    assert!(!page.entries.is_empty(), "the JP board has records");
    for entry in &page.entries {
        assert!(
            entry.leaderboards.iter().any(|lb| lb == "40l_country_JP"),
            "expected a JP-country record, got {:?}",
            entry.leaderboards
        );
    }
}

/// Trap: `limit=0`, `limit=999` and `limit=abc` produce three different 422s.
#[test]
#[ignore = "network"]
fn outrageous_limits_are_clamped_not_rejected() {
    let f = fetcher();
    let clamped_high = f.records("40l_global", 999).expect("999 clamps to 100");
    assert_eq!(clamped_high.requested_limit, 100);
    assert!(clamped_high.entries.len() <= 100);

    let clamped_low = f.records("40l_global", 0).expect("0 clamps to 1");
    assert_eq!(clamped_low.requested_limit, 1);
    assert_eq!(clamped_low.entries.len(), 1);
}

/// Trap: a typo'd gamemode returns `200` + `[]`, indistinguishable from
/// "this player has no records". The fetcher refuses the typo up front.
#[test]
#[ignore = "network"]
fn unknown_gamemode_never_reaches_the_network() {
    let f = fetcher();
    let err = f.user_records("keusidan", "foo", "top", 5).unwrap_err();
    assert!(matches!(err, Error::Invalid { field: "gamemode", .. }));
}

/// Trap: `200 OK` with `{"success": false}` — a status-code client sees success.
#[test]
#[ignore = "network"]
fn nonexistent_user_is_a_typed_error_not_a_parse_failure() {
    let f = fetcher();
    let err = f.user("thisuserdoesnotexist12345xyzzy").unwrap_err();
    match err {
        Error::Api { status, ref msg, .. } => {
            assert_eq!(status, 404, "expected the inferred 404, message: {msg}");
            assert!(msg.contains("No such user"), "message was {msg}");
        }
        other => panic!("expected Error::Api, got {other:?}"),
    }
}

#[test]
#[ignore = "network"]
fn bad_leaderboard_for_user_leaderboard_is_a_typed_error() {
    let f = fetcher();
    let err = f.user_leaderboard("not-a-board", None, 5).unwrap_err();
    assert!(matches!(err, Error::Invalid { .. }));
}

/// Trap: `xp` is an integer for small accounts and a float for large ones.
#[test]
#[ignore = "network"]
fn xp_survives_both_integer_and_float_encodings() {
    let f = fetcher();
    let page = f.user_leaderboard("xp", None, 25).expect("xp leaderboard");
    assert!(!page.entries.is_empty());
    for entry in &page.entries {
        let xp = entry.xp.expect("the xp board always has xp");
        assert!(xp > 0.0);
    }
    assert!(page.entries[0].xp.unwrap() >= page.entries[1].xp.unwrap());
}

/// Trap: `gamesplayed: -1` means *hidden*, not minus one.
#[test]
#[ignore = "network"]
fn hidden_statistics_become_none_not_negative() {
    let f = fetcher();
    let page = f.user_leaderboard("league", None, 25).expect("league board");
    for entry in &page.entries {
        if let Some(games) = entry.games_played {
            assert!(games >= 0, "a negative games_played leaked: {games}");
        }
    }
}

/// Trap: unranked players have `rank: "z"`, `tr: -1`.
#[test]
#[ignore = "network"]
fn unranked_league_is_null_not_the_letter_z() {
    let f = fetcher();
    let standing = f.user_league("keusidan").expect("league summary");
    // This account has no ranked games; upstream sends rank "z" and tr -1.
    if standing.games_played == 0 {
        assert_eq!(standing.rank, None, "\"z\" must not be reported as a rank");
        assert_eq!(standing.tr, None, "-1 must not be reported as a rating");
    }
}

/// Trap: `/labs/league_ranks` nests its payload two levels deep.
#[test]
#[ignore = "network"]
fn league_ranks_unwraps_its_extra_nesting_and_sorts() {
    let f = fetcher();
    let ranks = f.league_ranks().expect("league ranks");
    assert!(ranks.total > 0);
    assert!(!ranks.bands.is_empty());
    assert_eq!(ranks.bands[0].rank, "x+", "highest band must come first");
    let positions: Vec<i64> = ranks.bands.iter().map(|b| b.position).collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "bands must be sorted by position");
}

/// Trap: `startTime: 9007199254740991` in an empty score-flow feed.
#[test]
#[ignore = "network"]
fn empty_scoreflow_has_no_absurd_timestamp() {
    let f = fetcher();
    let flow = f.score_flow("keusidan", "40l").expect("score flow");
    if flow.points.is_empty() {
        assert_eq!(flow.start_time, None, "JS MAX_SAFE_INTEGER must not leak");
    }
    for point in &flow.points {
        assert!(point.timestamp > 1_500_000_000, "sane epoch expected");
    }
}

/// The docs require honouring `cache.cached_until`. Second reads must not hit
/// the network.
#[test]
#[ignore = "network"]
fn server_cache_is_honoured_so_repeat_calls_are_free() {
    let f = fetcher();
    f.server_stats().expect("first call");
    let after_first = f.transport().stats();
    assert_eq!(after_first.network_requests, 1);

    f.server_stats().expect("second call");
    let after_second = f.transport().stats();
    assert_eq!(
        after_second.network_requests, 1,
        "the second call should have been served from cache"
    );
    assert_eq!(after_second.cache_hits, 1);
}

#[test]
#[ignore = "network"]
fn clearing_the_cache_forces_a_refetch() {
    let f = fetcher();
    f.activity().expect("first");
    f.transport().clear_cache();
    f.activity().expect("second");
    assert_eq!(f.transport().stats().network_requests, 2);
}

/// Cursors must round-trip: page 1's cursor must fetch page 2 without overlap.
#[test]
#[ignore = "network"]
fn pagination_cursor_round_trips() {
    let f = fetcher();
    let first = f.user_leaderboard("league", None, 5).expect("page 1");
    let cursor = first.next_cursor.clone().expect("cursor present");
    let second = f
        .user_leaderboard_opts("league", None, 5, Some((&cursor, PageDirection::Down)), true)
        .expect("page 2");
    assert_eq!(second.entries[0].placement, 1, "placement restarts per page");
    assert!(!second.entries.is_empty());
    let first_ids: Vec<&str> = first.entries.iter().map(|e| e.user_id.as_str()).collect();
    for entry in &second.entries {
        assert!(
            !first_ids.contains(&entry.user_id.as_str()),
            "page 2 repeated page 1's user {}",
            entry.username
        );
    }
}

/// Raw passthrough still rejects HTML error pages instead of exploding in serde.
#[test]
#[ignore = "network"]
fn raw_get_surfaces_html_pages_as_non_json() {
    let f = fetcher();
    // `/users/keusidan/records/40l` (no leaderboard segment) serves the HTML
    // TETRA CHANNEL 404 page, not JSON.
    let err = f.raw_get("users/keusidan/records/40l", &[]).unwrap_err();
    match err {
        Error::NonJson {
            content_type,
            ref snippet,
            ..
        } => {
            assert!(
                content_type.contains("html") || snippet.starts_with("<!DOCTYPE"),
                "content-type was {content_type}, snippet {snippet:?}"
            );
        }
        other => panic!("expected NonJson, got {other:?}"),
    }
}

#[test]
#[ignore = "network"]
fn server_stats_derives_consistent_rates() {
    let f = fetcher();
    let stats = f.server_stats().expect("stats");
    let pps = stats.average_pps.expect("derived pps");
    let kps = stats.average_kps.expect("derived kps");
    assert!(pps > 0.5 && pps < 1.5, "server-wide pps looks wrong: {pps}");
    assert!(kps > pps, "there is more than one key per piece");
}
