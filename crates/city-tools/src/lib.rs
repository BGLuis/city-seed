use std::collections::BTreeMap;

/// Parses `--key value` pairs and bare `--flag`s.
pub fn parse_args() -> BTreeMap<String, String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut parsed = BTreeMap::new();
    while let Some(arg) = args.next() {
        let key = arg.trim_start_matches("--").to_string();
        let has_value = args.peek().is_some_and(|next| !next.starts_with("--"));
        let value = if has_value {
            args.next().unwrap_or_default()
        } else {
            String::new()
        };
        parsed.insert(key, value);
    }
    parsed
}

pub fn number<T: std::str::FromStr>(args: &BTreeMap<String, String>, key: &str, default: T) -> T {
    args.get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
