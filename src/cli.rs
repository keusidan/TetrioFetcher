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
