//! RFC 3339 timestamps ("2026-10-01T10:00:00.123Z", "+02:00" offsets) to Unix milliseconds.

/// The line's `timestamp` in Unix milliseconds, else `prev` (a missing or odd time never fails a line).
pub(crate) fn ts_ms(v: &serde_json::Value, prev: i64) -> i64 {
    v["timestamp"].as_str().and_then(rfc3339_ms).unwrap_or(prev)
}

pub(crate) fn rfc3339_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    let mut rest = s.get(19..)?;
    let mut ms = 0;
    if let Some(f) = rest.strip_prefix('.') {
        let digits = f.bytes().take_while(u8::is_ascii_digit).count();
        ms = format!("{:0<3}", &f[..digits.min(3)]).parse().ok()?;
        rest = &f[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        o if o.len() == 6 && matches!(o.as_bytes()[0], b'+' | b'-') && o.as_bytes()[3] == b':' => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            sign * (o[1..3].parse::<i64>().ok()? * 60 + o[4..6].parse::<i64>().ok()?) * 60_000
        }
        _ => return None,
    };
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se) * 1000 + ms - offset)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {

    #[test]
    fn utc_offsets_and_fractions() {
        assert_eq!(
            crate::portable::convert::time::rfc3339_ms("1970-01-01T00:00:00Z"),
            Some(0)
        );
        assert_eq!(
            crate::portable::convert::time::rfc3339_ms("2026-10-01T10:00:00.5Z"),
            Some(1_790_848_800_500)
        );
        assert_eq!(
            crate::portable::convert::time::rfc3339_ms("2026-10-01T12:00:00.000+02:00"),
            Some(1_790_848_800_000)
        );
        assert_eq!(
            crate::portable::convert::time::rfc3339_ms("2024-02-29T00:00:00.123456Z"),
            Some(1_709_164_800_123)
        );
        assert_eq!(
            crate::portable::convert::time::rfc3339_ms("yesterday"),
            None
        );
    }
}
