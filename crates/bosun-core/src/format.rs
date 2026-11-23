//! Human-readable number formatting used across the UI.

/// `307.46 MB`, `1.20 GB`, `512 B` — binary units, two decimals above bytes.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.2} {}", UNITS[i])
}

/// Bytes per second, `1.4 MB/s` style (one decimal).
pub fn rate(bps: u64) -> String {
    if bps < 1024 {
        return format!("{bps} B/s");
    }
    let mut v = bps as f64;
    let mut i = 0;
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.00 KB");
        assert_eq!(bytes(322_396_365), "307.46 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn formats_rate() {
        assert_eq!(rate(500), "500 B/s");
        assert_eq!(rate(1536), "1.5 KB/s");
        assert_eq!(rate(5 * 1024 * 1024), "5.0 MB/s");
    }
}
