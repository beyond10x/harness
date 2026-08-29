//! What the two wires ask of `harness-http`, compared.
//!
//! The transport half is one crate now, and the reason it could become one is that both wires
//! wanted the same thing from it. That claim needs somewhere it can fail: a wire that quietly
//! doubled its attempts or halved a timeout would otherwise be found by a person reading two files
//! side by side, which is how the duplication survived a release in the first place.
//!
//! It lives in this crate's suite because both wires are visible here — `harness-responses` is a
//! **dev**-dependency and nothing under `src/` may import it. Same shape as
//! `provider_emulated.rs`'s `the_two_wires_serve_the_same_scenarios`, which compares the two
//! emulators for the same reason.

use b10x_harness_messages::TRANSPORT as MESSAGES;
use harness_http::{Framing, RetryPolicy, Settings};
use harness_responses::TRANSPORT as RESPONSES;

#[test]
fn the_two_wires_configure_one_transport_and_differ_only_in_their_framing() {
    // Field by field, by comparing whole values with the one permitted difference substituted in:
    // a setting added later is compared without anybody remembering to add a line here.
    assert_eq!(
        Settings {
            framing: MESSAGES.framing,
            ..RESPONSES
        },
        MESSAGES,
        "the two wires disagree about something other than framing; either that is a finding worth \
         a line in STATUS.md or one of them drifted"
    );
}

#[test]
fn the_framing_is_the_difference_and_each_wire_names_its_own() {
    // The one thing that is genuinely per-route. The first route ends its stream with
    // `data: [DONE]`; the second has no sentinel at all and ends on a `message_stop` payload, so a
    // `[DONE]` line there is a payload that is not JSON and refuses as one. Unifying these would
    // have taught the second route a sentinel it does not speak.
    assert_eq!(RESPONSES.framing, Framing::DoneSentinel);
    assert_eq!(MESSAGES.framing, Framing::PayloadsOnly);
    assert_ne!(RESPONSES.framing, MESSAGES.framing);
}

#[test]
fn both_wires_retry_on_the_shared_policy_rather_than_one_of_their_own() {
    // The numbers the emulated suites depend on: four attempts is what
    // `a_cold_gateway_is_retriable_transport_rather_than_a_refusal` reads back out of the message
    // both wires produce.
    assert_eq!(RESPONSES.retry, RetryPolicy::DEFAULT);
    assert_eq!(MESSAGES.retry, RetryPolicy::DEFAULT);
    assert_eq!(RetryPolicy::DEFAULT.max_attempts, 4);
}
