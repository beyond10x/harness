//! What an HTTP status means to a run.

use harness_wire::{WireError, WireErrorCode};

/// Maps a status the far side answered with onto a typed refusal, and says whether to try again.
///
/// `wire` is the caller's own name for itself, so the message says who was answered — the only
/// vendor-shaped thing in this mapping, and it arrives as an argument rather than living here.
///
/// # Where this came from
///
/// `harness-responses`, copied unchanged into `harness-messages`. Both arrived at the same table
/// independently of any field name, which is what makes it transport-shaped: it reads the status
/// line and nothing else.
///
/// # The table, and the two entries that are arguments rather than lookups
///
/// Every status in the retriable set says *the far side never got to answer*. 503 is a gateway
/// still starting a backend, and 529 — one route's own *overloaded* status — lands with the rest
/// of the 5xx range for the same reason. 408 is a far side that gave up waiting for a request it
/// never finished reading, so no model saw it and nothing was produced from it; sending it again
/// is the entire remedy.
///
/// 409 is deliberately **not** there: a conflict is a disagreement about state, and the identical
/// request conflicts identically a second later, so a retry would spend the run's budget to be
/// refused four times.
pub fn status_error(wire: &str, status: reqwest::StatusCode, body: &str) -> WireError {
    let (code, retriable) = match status.as_u16() {
        401 | 403 => (WireErrorCode::Unauthorized, false),
        429 => (WireErrorCode::RateLimited, true),
        408 | 500..=599 => (WireErrorCode::Transport, true),
        _ => (WireErrorCode::Refused, false),
    };
    WireError::new(code, format!("{wire} answered {status}: {body}"), retriable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn http_statuses_map_to_actionable_codes() {
        assert_eq!(
            status_error("w", StatusCode::UNAUTHORIZED, "").code,
            WireErrorCode::Unauthorized
        );
        assert_eq!(
            status_error("w", StatusCode::FORBIDDEN, "").code,
            WireErrorCode::Unauthorized
        );
        assert_eq!(
            status_error("w", StatusCode::TOO_MANY_REQUESTS, "").code,
            WireErrorCode::RateLimited
        );
        let cold = status_error("w", StatusCode::SERVICE_UNAVAILABLE, "");
        assert_eq!(cold.code, WireErrorCode::Transport);
        assert!(cold.retriable, "a cold gateway is worth another attempt");
        // One route answers 529 when it is momentarily full. It is not a registered status and
        // needs no entry of its own: the 5xx range already says the far side never answered.
        let overloaded = status_error("w", StatusCode::from_u16(529).expect("a valid status"), "");
        assert_eq!(overloaded.code, WireErrorCode::Transport);
        assert!(overloaded.retriable, "an overloaded route answers later");
        // The far side stopped waiting for a request it never finished reading, so no model saw
        // it. Sending it again is the whole remedy.
        let timeout = status_error("w", StatusCode::REQUEST_TIMEOUT, "");
        assert_eq!(timeout.code, WireErrorCode::Transport);
        assert!(timeout.retriable);
        // 409 is deliberately not in the retriable set: a conflict is a disagreement about state,
        // and the identical request conflicts identically a second later.
        let conflict = status_error("w", StatusCode::CONFLICT, "");
        assert_eq!(conflict.code, WireErrorCode::Refused);
        assert!(!conflict.retriable);
        assert_eq!(
            status_error("w", StatusCode::BAD_REQUEST, "").code,
            WireErrorCode::Refused
        );
    }

    #[test]
    fn the_message_names_the_wire_that_was_answered_and_carries_the_body() {
        let error = status_error("some-wire", StatusCode::BAD_REQUEST, "no such model");
        assert!(error.message.contains("some-wire"), "{error}");
        assert!(error.message.contains("400"), "{error}");
        assert!(error.message.contains("no such model"), "{error}");
    }
}
