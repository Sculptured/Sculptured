pub mod community_stub;

use std::path::PathBuf;

/// Scrubbed real protocol recordings from the M0 spike.
pub fn recordings_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recordings")
}

/// Fixture commands live in the work message; architecture is context for a real model.
pub fn work_message(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find("Team architecture (operator configuration):") {
        let Some(end) = out[start..].find("\n\n--- Current message ---\n") else { break };
        out.replace_range(start..start + end + "\n\n--- Current message ---\n".len(), "");
    }
    out
}

/// The time now as RFC 3339 in UTC with milliseconds (`2026-10-15T19:47:31.841Z`).
pub fn now_rfc3339() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) as i64;
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, mi, sec, milli) = (rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{sec:02}.{milli:03}Z")
}

#[cfg(test)]
mod clock_tests {
    #[test]
    fn now_is_an_rfc3339_time_in_this_century() {
        let t = super::now_rfc3339();
        assert!(t.starts_with("20") && t.ends_with('Z') && t.len() == 24, "{t}");
        assert_eq!(&t[10..11], "T");
    }
}
