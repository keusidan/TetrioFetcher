//! `tfetch` — a small CLI over [`tetrio_fetcher`].
//!
//! ```text
//! tfetch user keusidan
//! tfetch records 40l --limit 3
//! tfetch user-records keusidan 40l pb --limit 3     # `pb` is translated
//! tfetch leaderboard league --country JP --limit 3
//! tfetch stats
//! tfetch ranks
//! tfetch raw general/activity
//! ```
//!
//! Errors go to stderr in a one-line human form and set exit code 1, so the
//! tool composes in shell pipelines.

use std::process::ExitCode;

use serde::Serialize;

use tetrio_fetcher::{Fetcher, TransportConfig};

const USAGE: &str = "\
tfetch — TETR.IO TETRA CHANNEL fetcher

USAGE:
    tfetch <command> [args] [--limit N] [--country CC] [--base URL] [--no-cache]

COMMANDS:
    stats                          server statistics
    activity                       player counts over the last 24h
    user <user>                    user profile
    league <user>                  TETRA LEAGUE summary
    user-records <user> <mode> <board>
                                   personal records (board: top|recent|pb|progression)
    leaderboard <board>            users by league|xp|ar
    records <leaderboard>          record leaderboard (e.g. 40l_global)
    ranks                          TETRA LEAGUE rank bands
    scoreflow <user> <mode>        progression graph
    raw <path>                     any endpoint, normalized envelope
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let parsed = parse_args(args)?;
    let Some(command) = parsed.command.first() else {
        print!("{USAGE}");
        return Ok(());
    };

    let fetcher = Fetcher::with_config(TransportConfig {
        base_url: parsed.base_url.clone(),
        honour_server_cache: !parsed.no_cache,
        ..Default::default()
    });

    let rest = &parsed.command[1..];
    let json = match command.as_str() {
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            return Ok(());
        }
        "stats" => to_json(&fetcher.server_stats().map_err(fmt_err)?)?,
        "activity" => to_json(&fetcher.activity().map_err(fmt_err)?)?,
        "user" => to_json(&fetcher.user(need(rest, 0, "user")?).map_err(fmt_err)?)?,
        "league" => to_json(
            &fetcher
                .user_league(need(rest, 0, "user")?)
                .map_err(fmt_err)?,
        )?,
        "user-records" => {
            let page = fetcher
                .user_records(
                    need(rest, 0, "user")?,
                    need(rest, 1, "gamemode")?,
                    need(rest, 2, "leaderboard")?,
                    parsed.limit,
                )
                .map_err(fmt_err)?;
            to_json(&page)?
        }
        "leaderboard" => {
            let page = fetcher
                .user_leaderboard(
                    need(rest, 0, "board")?,
                    parsed.country.as_deref(),
                    parsed.limit,
                )
                .map_err(fmt_err)?;
            to_json(&page)?
        }
        "records" => {
            let page = fetcher
                .records(need(rest, 0, "leaderboard")?, parsed.limit)
                .map_err(fmt_err)?;
            to_json(&page)?
        }
        "ranks" => to_json(&fetcher.league_ranks().map_err(fmt_err)?)?,
        "scoreflow" => to_json(
            &fetcher
                .score_flow(need(rest, 0, "user")?, need(rest, 1, "gamemode")?)
                .map_err(fmt_err)?,
        )?,
        "raw" => to_json(
            &fetcher
                .raw_get(need(rest, 0, "path")?, &[])
                .map_err(fmt_err)?,
        )?,
        other => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    println!("{json}");
    Ok(())
}

struct Args {
    command: Vec<String>,
    limit: u32,
    country: Option<String>,
    base_url: String,
    no_cache: bool,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut command = Vec::new();
    let mut limit = 25u32;
    let mut country = None;
    let mut base_url = tetrio_fetcher::DEFAULT_BASE_URL.to_string();
    let mut no_cache = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--limit" => {
                let value = iter.next().ok_or("--limit needs a value")?;
                limit = value.parse().map_err(|_| {
                    format!("--limit must be an integer, got `{value}` (it is clamped to 1..=100)")
                })?;
            }
            "--country" => {
                country = Some(iter.next().ok_or("--country needs a value")?.clone());
            }
            "--base" => {
                base_url = iter.next().ok_or("--base needs a value")?.clone();
            }
            "--no-cache" => no_cache = true,
            other if other.starts_with("--") => {
                return Err(format!("unknown flag `{other}`"));
            }
            other => command.push(other.to_string()),
        }
    }

    Ok(Args {
        command,
        limit,
        country,
        base_url,
        no_cache,
    })
}

fn need<'a>(args: &'a [String], index: usize, what: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing argument <{what}>"))
}

fn to_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

/// Flatten an error into one line a shell user can read and grep.
fn fmt_err(err: tetrio_fetcher::Error) -> String {
    match &err {
        tetrio_fetcher::Error::Api { key: Some(key), .. } => {
            format!("{err} (the API rejected `{key}`; this crate normally repairs that)")
        }
        tetrio_fetcher::Error::Api { .. } => err.to_string(),
        _ => err.to_string(),
    }
}
