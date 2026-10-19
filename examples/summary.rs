//! A small report that would take a lot of defensive code without this crate.
//!
//! Run with:
//!
//! ```text
//! cargo run --example summary -- keusidan
//! ```

use tetrio_fetcher::{Fetcher, TransportConfig};

fn main() {
    let mut args = std::env::args().skip(1);
    let username = args.next().unwrap_or_else(|| "keusidan".to_string());

    let fetcher = Fetcher::with_config(TransportConfig {
        session_id: Some(format!("example-{}", std::process::id())),
        ..Default::default()
    });

    // 1. Profile. Note the capitalisation is irrelevant.
    let user = match fetcher.user(&username) {
        Ok(user) => user,
        Err(err) if err.is_likely_user_mistake() => {
            eprintln!("no such player `{username}`: {err}");
            return;
        }
        Err(err) => {
            eprintln!("could not reach TETRA CHANNEL: {err}");
            return;
        }
    };

    println!("== {} ==", user.username);
    println!("id           {}", user.id);
    println!("role         {}", user.role);
    println!("xp           {:.0}", user.xp);

    // Every "hidden" statistic is None rather than -1 or a misleading 0.
    match user.games_played {
        Some(games) => println!(
            "games        {games} played / {} won",
            user.games_won.unwrap_or(0)
        ),
        None => println!("games        hidden by the player"),
    }
    if let Some(seconds) = user.gametime_seconds {
        println!(
            "playtime     {}h {}m",
            (seconds / 3600.0) as u64,
            ((seconds % 3600.0) / 60.0) as u64
        );
    }
    match &user.country {
        Some(cc) => println!("country      {cc}"),
        None => println!("country      not set or hidden"),
    }

    // 2. League. An unranked player has rank/tr == None, not "z"/-1.
    match fetcher.user_league(&user.username) {
        Ok(league) => match (league.rank, league.tr) {
            (Some(rank), Some(tr)) => println!(
                "league       {} ({tr:.0} TR, {} games)",
                rank.to_uppercase(),
                league.games_played
            ),
            _ => println!("league       unranked"),
        },
        Err(err) => println!("league       unavailable: {err}"),
    }

    // 3. Personal bests. `pb` is not a real leaderboard name upstream; the
    //    crate rewrites it to `progression`.
    match fetcher.user_records(&user.username, "40l", "pb", 3) {
        Ok(page) if page.entries.is_empty() => {
            println!("40l pb       none recorded");
        }
        Ok(page) => {
            for (index, record) in page.entries.iter().enumerate() {
                println!(
                    "40l pb #{:<3} {:>6} pts, {} lines, {:.2} pps",
                    index + 1,
                    record.stats.score.unwrap_or(0),
                    record.stats.lines.unwrap_or(0),
                    record.aggregate.map(|a| a.pps).unwrap_or(0.0),
                );
            }
        }
        Err(err) => println!("40l pb       unavailable: {err}"),
    }

    // 4. Progression graph. An empty feed has `start_time: None`, not the JS
    //    MAX_SAFE_INTEGER sentinel.
    if let Ok(flow) = fetcher.score_flow(&user.username, "40l") {
        println!("scoreflow    {} points", flow.points.len());
        if let Some(start) = flow.start_time {
            println!("             since unix {start}");
        }
    }

    // 5. Server-wide numbers, with derived rates.
    if let Ok(stats) = fetcher.server_stats() {
        println!(
            "server       {} players, {:.2} pps average",
            stats.user_count,
            stats.average_pps.unwrap_or(0.0)
        );
    }

    // 6. Show what the cache saved us.
    let stats = fetcher.transport().stats();
    println!(
        "\n(cache: {} from network, {} reused, {} retries)",
        stats.network_requests, stats.cache_hits, stats.retries
    );
}
