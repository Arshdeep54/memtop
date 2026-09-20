const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

pub(crate) fn format_bytes(bytes: u64, raw: bool) -> String {
    if raw {
        return bytes.to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty value".to_string());
    }

    let split_at = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split_at);

    let value: f64 = num
        .parse()
        .map_err(|_| format!("invalid number '{num}'"))?;
    if value < 0.0 {
        return Err("negative values are not allowed".to_string());
    }

    let multiplier: f64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1.0,
        "K" | "KB" | "KIB" => 1024.0,
        "M" | "MB" | "MIB" => 1024.0 * 1024.0,
        "G" | "GB" | "GIB" => 1024.0f64.powi(3),
        "T" | "TB" | "TIB" => 1024.0f64.powi(4),
        other => return Err(format!("unknown unit '{other}'")),
    };

    Ok((value * multiplier) as u64)
}

pub(crate) fn pct_of(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    if max == 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_units() {
        assert_eq!(parse_size("512"), Ok(512));
        assert_eq!(parse_size("512B"), Ok(512));
        assert_eq!(parse_size("1K"), Ok(1024));
        assert_eq!(parse_size("1KB"), Ok(1024));
        assert_eq!(parse_size("1KiB"), Ok(1024));
        assert_eq!(parse_size("512M"), Ok(512 * 1024 * 1024));
        assert_eq!(parse_size("1G"), Ok(1024 * 1024 * 1024));
        assert_eq!(parse_size("2T"), Ok(2 * 1024_u64.pow(4)));
        assert_eq!(parse_size(" 1g "), Ok(1024 * 1024 * 1024));
    }

    #[test]
    fn parse_size_decimals() {
        assert_eq!(parse_size("1.5K"), Ok(1536));
        assert_eq!(parse_size("0.5M"), Ok(512 * 1024));
    }

    #[test]
    fn parse_size_errors() {
        assert!(parse_size("").is_err());
        assert!(parse_size("   ").is_err());
        assert!(parse_size("-5M").is_err());
        assert!(parse_size("12Z").is_err());
        assert!(parse_size("abc").is_err());
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0, false), "0 B");
        assert_eq!(format_bytes(512, false), "512 B");
        assert_eq!(format_bytes(1024, false), "1.0 KiB");
        assert_eq!(format_bytes(1536, false), "1.5 KiB");
        assert_eq!(format_bytes(1024 * 1024, false), "1.0 MiB");
        assert_eq!(format_bytes(3 * 1024_u64.pow(3), false), "3.0 GiB");
        // unit ladder stops at TiB
        assert_eq!(format_bytes(1024_u64.pow(5), false), "1024.0 TiB");
    }

    #[test]
    fn format_bytes_raw() {
        assert_eq!(format_bytes(2048, true), "2048");
    }

    #[test]
    fn truncate_limits() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello", 3), "he…");
        assert_eq!(truncate("hello", 0), "");
        assert_eq!(truncate("hello", 1), "…");
    }

    #[test]
    fn truncate_multibyte() {
        assert_eq!(truncate("héllo", 4), "hél…");
        assert_eq!(truncate("日本語です", 3), "日本…");
    }

    #[test]
    fn pct_of_basic() {
        assert_eq!(pct_of(50, 200), 25.0);
        assert_eq!(pct_of(1, 0), 0.0);
    }
}
