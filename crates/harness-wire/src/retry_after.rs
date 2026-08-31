use std::time::{Duration, SystemTime};

/// Decodes the standard server-requested retry delay without reading a clock.
///
/// The caller supplies both response headers and `now`, preserving this crate's no-I/O,
/// no-clock boundary. Delta seconds and HTTP dates are accepted. A past date requests no extra
/// delay; malformed or non-UTF-8 headers are absent rather than guessed.
#[must_use]
pub fn retry_after(headers: &[(String, String)], now: SystemTime) -> Option<Duration> {
    let value = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))?
        .1
        .trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let instant = httpdate::parse_http_date(value).ok()?;
    Some(instant.duration_since(now).unwrap_or(Duration::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_seconds_and_http_dates_are_both_decoded_against_the_supplied_clock() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(
            retry_after(&[("Retry-After".to_owned(), "17".to_owned())], now),
            Some(Duration::from_secs(17))
        );
        let later = now + Duration::from_secs(23);
        assert_eq!(
            retry_after(
                &[("retry-after".to_owned(), httpdate::fmt_http_date(later))],
                now
            ),
            Some(Duration::from_secs(23))
        );
    }

    #[test]
    fn malformed_values_are_absent_and_past_dates_request_no_extra_delay() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(
            retry_after(&[("retry-after".to_owned(), "later".to_owned())], now),
            None
        );
        assert_eq!(
            retry_after(
                &[(
                    "retry-after".to_owned(),
                    httpdate::fmt_http_date(now - Duration::from_secs(1))
                )],
                now
            ),
            Some(Duration::ZERO)
        );
    }
}
