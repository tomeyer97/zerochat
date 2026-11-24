//! The timestamps in provider session logs and Zeroshot's notices: RFC 3339 in UTC
//! (`2026-11-04T10:12:31.984Z`) or with an offset. zerochat reads only this one format, so it
//! parses it here instead of depending on a date crate; the calendar arithmetic is the
//! standard days-from-civil algorithm.

/// `2026-11-04T10:12:31.984Z` (or with an offset) → Unix ms.
pub fn parse_iso_millis(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (
        num(0..4)?,
        num(5..7)?,
        num(8..10)?,
        num(11..13)?,
        num(14..16)?,
        num(17..19)?,
    );
    let mut rest = &text[19..];
    let mut millis = 0i64;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        rest = &frac[digits.len()..];
        let padded = format!("{:0<3}", &digits[..digits.len().min(3)]);
        millis = padded.parse().ok()?;
    }
    let offset_min = match rest.chars().next() {
        None | Some('Z') => 0,
        Some(sign @ ('+' | '-')) => {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6).and_then(|m| m.parse().ok()).unwrap_or(0);
            let total = oh * 60 + om;
            if sign == '+' { total } else { -total }
        }
        _ => return None,
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s - offset_min * 60;
    u64::try_from(secs * 1000 + millis).ok()
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`:
/// the year starts in March so the leap day is last; 146 097 days per 400-year era; 719 468 is
/// 1970-01-01 counted from 0000-03-01).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times() {
        assert_eq!(parse_iso_millis("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(
            parse_iso_millis("2026-11-04T10:12:31.984Z"),
            Some(1_793_787_151_984)
        );
        assert_eq!(
            parse_iso_millis("2026-11-04T11:12:31.984+01:00"),
            Some(1_793_787_151_984)
        );
    }

    #[test]
    fn civil_days_around_leap_days_and_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 2, 29), 11_016);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 11, 4), 20_761);
    }
}
