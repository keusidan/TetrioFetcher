//! Tolerant accessors for `serde_json::Value`.
//!
//! The API is not consistent about JSON types. Over the course of a few probes
//! the same field came back as:
//!
//! ```text
//! xp: 22336                  (integer)
//! xp: 68966508.611           (float)
//! cached_at: 1792441227476   (integer, but a JS number: safe only below 2^53)
//! startTime: 9007199254740991 (JS MAX_SAFE_INTEGER, used as "no data")
//! gamesplayed: -1            (means "hidden")
//! rank: "z"                  (means "unranked")
//! ```
//!
//! `serde`'s derived deserializers reject `42.0` for an `i64` field, so typed
//! structs written directly against this API break whenever upstream changes a
//! number's representation. These helpers read *through* that: callers ask for
//! the value they semantically want and get `None` for sentinels.

use serde_json::Value;

/// Read `v[key]` when it exists and is not JSON `null`.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !x.is_null())
}

/// Read `v[key]` as a string, also accepting numbers and bools.
///
/// `rank: 3` and `rank: "3"` both become `"3"`.
pub fn as_str(v: &Value, key: &str) -> Option<String> {
    match get(v, key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Read `v[key]` as an `i64`, accepting `42`, `42.0`, `"42"`.
///
/// Floats are truncated toward zero, which is correct for counters (`xp`,
/// `gamesplayed`) that are the only int-ish fields arriving as floats.
pub fn as_i64(v: &Value, key: &str) -> Option<i64> {
    match get(v, key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok().or_else(|| {
            s.trim()
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite())
                .map(|f| f as i64)
        }),
        _ => None,
    }
}

/// Read `v[key]` as an `f64`, accepting `42` and `"42.5"`.
pub fn as_f64(v: &Value, key: &str) -> Option<f64> {
    match get(v, key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// Read `v[key]` as a bool, accepting `true`, `"true"`, `1`, `"1"`.
pub fn as_bool(v: &Value, key: &str) -> Option<bool> {
    match get(v, key)? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Read a "hidden" statistic.
///
/// The API uses `-1` for *hidden* and `0` for *genuinely zero* (`gamesplayed`,
/// `gameswon`, `gametime`). Collapsing them would turn "this player hides their
/// stats" into "this player never played", so `-1` becomes `None`.
pub fn as_stat(v: &Value, key: &str) -> Option<i64> {
    as_i64(v, key).filter(|n| *n >= 0)
}

/// Like [`as_stat`] but for float stats such as `gametime`.
pub fn as_stat_f64(v: &Value, key: &str) -> Option<f64> {
    as_f64(v, key).filter(|n| *n >= 0.0)
}

/// Read a rank string, mapping the unranked sentinels to `None`.
///
/// Unranked players come back as `rank: "z"`. Reporting `"z"` as a letter rank
/// confuses every consumer.
pub fn as_rank(v: &Value, key: &str) -> Option<String> {
    let s = as_str(v, key)?;
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("z") || s == "-1" {
        None
    } else {
        Some(s.to_string())
    }
}

/// Read a TR-like float that uses `-1` as "unranked".
pub fn as_rating(v: &Value, key: &str) -> Option<f64> {
    as_f64(v, key).filter(|n| *n >= 0.0)
}

/// Read an array of numbers from `v[key]`, skipping non-numeric entries.
pub fn as_num_array(v: &Value, key: &str) -> Vec<f64> {
    match get(v, key) {
        Some(Value::Array(items)) => items.iter().filter_map(number_of).collect(),
        _ => Vec::new(),
    }
}

fn number_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Interpret a millisecond timestamp that may be a JS sentinel.
///
/// `9007199254740991` (JS `Number.MAX_SAFE_INTEGER`) shows up as `startTime` in
/// `/labs/leagueflow` for users with no matches. It is not a date in any
/// calendar a human wants, so it becomes `None`.
pub fn as_millis(v: &Value, key: &str) -> Option<u64> {
    const JS_MAX_SAFE: i64 = 9_007_199_254_740_991;
    as_i64(v, key)
        .filter(|n| *n >= 0 && *n != JS_MAX_SAFE)
        .map(|n| n as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integers_and_floats_are_both_accepted() {
        let v = json!({"a": 42, "b": 42.0, "c": 42.9, "d": "42", "e": " 42.0 "});
        assert_eq!(as_i64(&v, "a"), Some(42));
        assert_eq!(as_i64(&v, "b"), Some(42));
        assert_eq!(as_i64(&v, "c"), Some(42), "truncates toward zero");
        assert_eq!(as_i64(&v, "d"), Some(42));
        assert_eq!(as_i64(&v, "e"), Some(42));
    }

    #[test]
    fn giant_floats_do_not_overflow() {
        // xp in the wild: 68966508.61132418
        let v = json!({"xp": 68966508.61132418_f64});
        assert_eq!(as_i64(&v, "xp"), Some(68966508));
        assert_eq!(as_f64(&v, "xp"), Some(68966508.61132418));
    }

    #[test]
    fn null_and_missing_are_none() {
        let v = json!({"a": null});
        assert_eq!(as_str(&v, "a"), None);
        assert_eq!(as_str(&v, "missing"), None);
        assert_eq!(as_i64(&v, "a"), None);
    }

    #[test]
    fn hidden_stats_are_none_but_zero_is_zero() {
        let v = json!({"hidden": -1, "zero": 0, "real": 12});
        assert_eq!(as_stat(&v, "hidden"), None);
        assert_eq!(as_stat(&v, "zero"), Some(0));
        assert_eq!(as_stat(&v, "real"), Some(12));
    }

    #[test]
    fn unranked_sentinels_are_none() {
        let v = json!({"rank": "z", "tr": -1.0, "standing": -1, "real": "x+"});
        assert_eq!(as_rank(&v, "rank"), None);
        assert_eq!(as_rank(&v, "real"), Some("x+".into()));
        assert_eq!(as_rating(&v, "tr"), None);
        assert_eq!(as_i64(&v, "standing"), Some(-1), "raw accessor is literal");
    }

    #[test]
    fn booleans_survive_every_encoding() {
        for raw in [json!(true), json!("true"), json!(1), json!("1")] {
            assert_eq!(as_bool(&json!({"b": raw}), "b"), Some(true));
        }
        for raw in [json!(false), json!("false"), json!(0), json!("0")] {
            assert_eq!(as_bool(&json!({"b": raw}), "b"), Some(false));
        }
    }

    #[test]
    fn js_max_safe_integer_is_not_a_time() {
        let v = json!({"startTime": 9007199254740991_i64, "ok": 1792440739406_i64});
        assert_eq!(as_millis(&v, "startTime"), None);
        assert_eq!(as_millis(&v, "ok"), Some(1792440739406));
    }

    #[test]
    fn numeric_arrays_skip_junk() {
        let v = json!({"activity": [1, 2.5, "3", null, "x"]});
        assert_eq!(as_num_array(&v, "activity"), vec![1.0, 2.5, 3.0]);
    }
}
