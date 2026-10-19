//! Route and identifier normalization.
//!
//! This is where most "the API is broken" reports actually come from: the
//! identifiers humans (and other libraries) use do not match what the server
//! accepts.
//!
//! * Gamemodes: `40l` / `40L` / `40lines` / `blitz` / `zenith` / `zenithex` /
//!   `league`. Anything else is rejected *before* it becomes a 200-with-empty
//!   response.
//! * Personal-record leaderboards: the URL segment must be `top`, `recent` or
//!   `progression`. Handing it `pb`, `global` or `personal` yields
//!   `422 ... The params field 'leaderboard' is illegal`, so we translate.
//! * Record leaderboard ids must be combined: `40l_global`, `blitz_country_XM`,
//!   `zenith_global@2024w31`. Passing the bare `40l` silently returns `[]`.

use crate::error::{Error, Result};

/// A gamemode accepted by `/users/:user/records/:gamemode/:leaderboard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gamemode {
    /// 40 LINES
    FourtyLines,
    /// BLITZ
    Blitz,
    /// QUICK PLAY
    Zenith,
    /// EXPERT QUICK PLAY
    ZenithEx,
    /// TETRA LEAGUE history
    League,
}

impl Gamemode {
    /// The wire token, e.g. `"40l"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Gamemode::FourtyLines => "40l",
            Gamemode::Blitz => "blitz",
            Gamemode::Zenith => "zenith",
            Gamemode::ZenithEx => "zenithex",
            Gamemode::League => "league",
        }
    }

    /// Parse a gamemode, tolerating the spellings people actually type.
    ///
    /// Accepts case differences, `40lines`, `quickplay`, and the C# library's
    /// `Expert_QuickPlay`.
    pub fn parse(input: &str) -> Result<Self> {
        let normalized: String = input
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let mode = match normalized.as_str() {
            "40l" | "40lines" | "40line" | "40" => Gamemode::FourtyLines,
            "blitz" => Gamemode::Blitz,
            "zenith" | "quickplay" | "zen" => Gamemode::Zenith,
            "zenithex" | "expertquickplay" | "quickplayexpert" => Gamemode::ZenithEx,
            "league" | "tetraleague" | "tl" => Gamemode::League,
            _ => {
                return Err(Error::Invalid {
                    field: "gamemode",
                    value: input.to_string(),
                    reason: "expected one of 40l, blitz, zenith, zenithex, league",
                })
            }
        };
        Ok(mode)
    }
}

/// A personal-record leaderboard segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonalBoard {
    /// Their top scores.
    Top,
    /// Their most recently placed records.
    Recent,
    /// Their top scores, PBs only.
    Progression,
}

impl PersonalBoard {
    /// The wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            PersonalBoard::Top => "top",
            PersonalBoard::Recent => "recent",
            PersonalBoard::Progression => "progression",
        }
    }

    /// Parse a leaderboard segment, repairing the common wrong guesses.
    ///
    /// `pb` / `pbs` / `personal` / `personalbest` mean "progression";
    /// `global` / `country` / `all` mean "top"; `latest` / `new` mean "recent".
    /// These are the names other TETR.IO clients use for the same data, so
    /// code ported from them keeps working.
    pub fn parse(input: &str) -> Result<Self> {
        let normalized: String = input
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let board = match normalized.as_str() {
            "top" | "global" | "country" | "all" | "best" => PersonalBoard::Top,
            "recent" | "latest" | "new" | "history" => PersonalBoard::Recent,
            "progression" | "pb" | "pbs" | "personal" | "personalbest" => {
                PersonalBoard::Progression
            }
            _ => {
                return Err(Error::Invalid {
                    field: "leaderboard",
                    value: input.to_string(),
                    reason: "expected one of top, recent, progression (pb/global are aliases)",
                })
            }
        };
        Ok(board)
    }
}

/// A full `/records/:leaderboard` id such as `40l_global` or `blitz_country_XM`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordLeaderboard {
    /// The gamemode part.
    pub gamemode: Gamemode,
    /// Either `GLOBAL` or a two-letter country code.
    pub scope: String,
    /// Optional Revolution id, e.g. `2024w31`.
    pub revolution: Option<String>,
}

impl RecordLeaderboard {
    /// Build from parts, canonicalizing the scope.
    pub fn new(gamemode: Gamemode, scope: &str) -> Self {
        Self {
            gamemode,
            scope: scope.trim().to_ascii_uppercase(),
            revolution: None,
        }
    }

    /// The global leaderboard for a gamemode.
    pub fn global(gamemode: Gamemode) -> Self {
        Self::new(gamemode, "GLOBAL")
    }

    /// Attach a Revolution id (the `@2024w31` suffix).
    pub fn with_revolution(mut self, rev: &str) -> Self {
        let rev = rev.trim().trim_start_matches('@');
        self.revolution = if rev.is_empty() {
            None
        } else {
            Some(rev.to_ascii_lowercase())
        };
        self
    }

    /// Render the wire id, e.g. `blitz_country_JP@2024w31`.
    pub fn to_id(&self) -> String {
        let scope = if self.scope.eq_ignore_ascii_case("GLOBAL") || self.scope.is_empty() {
            "global".to_string()
        } else {
            format!("country_{}", self.scope.to_ascii_uppercase())
        };
        match &self.revolution {
            Some(rev) => format!("{}_{scope}@{rev}", self.gamemode.as_str()),
            None => format!("{}_{scope}", self.gamemode.as_str()),
        }
    }

    /// Parse a wire id, tolerating the "bare gamemode" and `40l_JP` mistakes.
    ///
    /// * `40l_global` -> global
    /// * `blitz_country_XM` -> country XM
    /// * `zenith_global@2024w31` -> global + revolution
    /// * `40l` -> **global** (upstream would answer `[]`)
    /// * `40l_JP` -> country JP (upstream would answer `[]`)
    pub fn parse(input: &str) -> Result<Self> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err(Error::Invalid {
                field: "leaderboard",
                value: String::new(),
                reason: "leaderboard id must not be empty",
            });
        }
        let (body, revolution) = match raw.split_once('@') {
            Some((b, r)) => (b, Some(r.to_string())),
            None => (raw, None),
        };
        let mut parts = body.split('_');
        let mode_raw = parts.next().unwrap_or_default();
        let gamemode = Gamemode::parse(mode_raw)?;
        let rest: Vec<&str> = parts.collect();

        let mut scope = "GLOBAL".to_string();
        if !rest.is_empty() {
            scope = match rest.as_slice() {
                ["global"] => "GLOBAL".to_string(),
                ["country", cc] => cc.to_ascii_uppercase(),
                [cc] if cc.len() == 2 => cc.to_ascii_uppercase(),
                _ => {
                    return Err(Error::Invalid {
                        field: "leaderboard",
                        value: raw.to_string(),
                        reason: "expected <mode>_global, <mode>_country_XX or <mode>_XX",
                    })
                }
            };
        }

        let mut lb = RecordLeaderboard {
            gamemode,
            scope,
            revolution: None,
        };
        if let Some(rev) = revolution {
            lb = lb.with_revolution(&rev);
        }
        Ok(lb)
    }
}

/// Normalize a username or MongoID for the URL path.
///
/// The API is case sensitive and answers `404` for `KEUSIDAN` and `Keusidan`;
/// only the lowercase form works. MongoIDs are already lowercase hex so they
/// pass through unchanged.
pub fn normalize_user(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::Invalid {
            field: "user",
            value: String::new(),
            reason: "username must not be empty",
        });
    }
    if trimmed.len() > 64 {
        return Err(Error::Invalid {
            field: "user",
            value: format!("{}...", &trimmed[..16]),
            reason: "longer than any TETR.IO username or id",
        });
    }
    if trimmed.contains('/') || trimmed.contains('?') || trimmed.contains('#') {
        return Err(Error::Invalid {
            field: "user",
            value: trimmed.to_string(),
            reason: "contains a path or query separator",
        });
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// Clamp a `limit` parameter into the range the API accepts.
///
/// Upstream answers `422 The query field 'limit' must be at least 1` /
/// `must be no more than 100`, each with a different message. Clamping means a
/// caller asking for 500 quietly gets 100 instead of an error they must handle.
pub fn clamp_limit(requested: u32) -> u32 {
    requested.clamp(1, 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamemode_spellings_are_forgiving() {
        for raw in ["40l", "40L", "40lines", "40 Lines", "40Line"] {
            assert_eq!(
                Gamemode::parse(raw).unwrap(),
                Gamemode::FourtyLines,
                "{raw}"
            );
        }
        assert_eq!(Gamemode::parse("QuickPlay").unwrap(), Gamemode::Zenith);
        assert_eq!(
            Gamemode::parse("Expert_QuickPlay").unwrap(),
            Gamemode::ZenithEx
        );
        assert_eq!(Gamemode::parse("TETRA LEAGUE").unwrap(), Gamemode::League);
        assert!(Gamemode::parse("sprint").is_err());
    }

    #[test]
    fn wrong_leaderboard_names_are_repaired_not_forwarded() {
        // These are exactly the values that make upstream answer
        // "The params field 'leaderboard' is illegal" (HTTP 422).
        assert_eq!(
            PersonalBoard::parse("pb").unwrap(),
            PersonalBoard::Progression
        );
        assert_eq!(
            PersonalBoard::parse("PBS").unwrap(),
            PersonalBoard::Progression
        );
        assert_eq!(
            PersonalBoard::parse("personal").unwrap(),
            PersonalBoard::Progression
        );
        assert_eq!(PersonalBoard::parse("global").unwrap(), PersonalBoard::Top);
        assert_eq!(
            PersonalBoard::parse("recent").unwrap(),
            PersonalBoard::Recent
        );
        assert!(PersonalBoard::parse("whatever").is_err());
    }

    #[test]
    fn record_leaderboard_bare_mode_becomes_global() {
        // `/records/40l` returns 200 with [] upstream; we make it meaningful.
        assert_eq!(
            RecordLeaderboard::parse("40l").unwrap().to_id(),
            "40l_global"
        );
        assert_eq!(
            RecordLeaderboard::parse("40l_global").unwrap().to_id(),
            "40l_global"
        );
        assert_eq!(
            RecordLeaderboard::parse("blitz_country_xm")
                .unwrap()
                .to_id(),
            "blitz_country_XM"
        );
        assert_eq!(
            RecordLeaderboard::parse("zenith_global@2024w31")
                .unwrap()
                .to_id(),
            "zenith_global@2024w31"
        );
        // Short country form `40l_JP` -> `40l_country_JP`
        assert_eq!(
            RecordLeaderboard::parse("40l_JP").unwrap().to_id(),
            "40l_country_JP"
        );
    }

    #[test]
    fn record_leaderboard_rejects_nonsense() {
        assert!(RecordLeaderboard::parse("").is_err());
        assert!(RecordLeaderboard::parse("nonsense_global_extra_bits").is_err());
    }

    #[test]
    fn usernames_are_lowercased_because_the_api_is_case_sensitive() {
        assert_eq!(normalize_user("  Keusidan ").unwrap(), "keusidan");
        assert_eq!(normalize_user("KEUSIDAN").unwrap(), "keusidan");
        assert_eq!(
            normalize_user("6667cfa451ddfaaf19734af5").unwrap(),
            "6667cfa451ddfaaf19734af5"
        );
        assert!(normalize_user("").is_err());
        assert!(normalize_user("a/b").is_err());
    }

    #[test]
    fn limits_are_clamped_instead_of_erroring() {
        assert_eq!(clamp_limit(0), 1);
        assert_eq!(clamp_limit(25), 25);
        assert_eq!(clamp_limit(999), 100);
    }
}
