//! What a run cost, at rates somebody declared.
//!
//! # Why this exists at all
//!
//! Until now this loop reported tokens and no price, on the reasoning that *a gateway relays bytes
//! and reports no price*. That reasoning was half right and the wrong half mattered: the provider
//! does not return a price, but a price still exists, and every other harness in the comparison
//! states one. Claude Code prices a run it served on a **subscription** — the figure comes from a
//! model catalogue it fetches, not from the response — and reports `total_cost_usd` all the same.
//! A record that answered "tokens: 4,700" beside one that answered "$0.11" could not be compared on
//! the axis an evaluation programme cares about most.
//!
//! # Declared, never guessed
//!
//! Rates are not compiled in. A table baked into this binary would be a set of numbers nobody could
//! date, wrong the first time a rate moved, and wrong silently. Instead the operator points the run
//! at a **rate card** — a small JSON document that names its own source and the day it was read —
//! and every figure the run reports can be traced back to it.
//!
//! ```json
//! {
//!   "source": "openai.com/api/pricing, read by hand",
//!   "as_of": "2026-08-24",
//!   "models": {
//!     "gpt-5.6-sol": {
//!       "input_usd_per_mtok": 1.25,
//!       "cached_input_usd_per_mtok": 0.125,
//!       "output_usd_per_mtok": 10.0
//!     }
//!   }
//! }
//! ```
//!
//! No card means no price, and **no price is reported as absent rather than as zero**. A model the
//! card does not list is warned about by name, once, at the start of the run — so a reader who sees
//! no cost knows nobody supplied a rate, rather than concluding the run was free.
//!
//! # Millionths of a dollar, and why the parts sum to the whole
//!
//! Every figure here is an integer count of micro-US-dollars. Arithmetic happens in
//! **pico**-dollars per token so that no rate has to be rounded before it is multiplied, and each
//! turn is rounded to a micro-dollar exactly once. The run total is the sum of the rounded turns
//! rather than a separately rounded total, which is what makes a reader's addition of the per-turn
//! figures agree with the total printed beside them.

use std::collections::BTreeMap;

use harness_wire::Usage;
use serde::{Deserialize, Serialize};

/// Pico-dollars in one micro-dollar.
const PICO_PER_MICRO: u128 = 1_000_000;
/// Pico-dollars per token, for a rate written as dollars per million tokens.
const PICO_PER_USD_PER_MTOK: f64 = 1_000_000.0;

/// A rate as an operator wrote it: US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRates {
    /// Input tokens the provider did **not** serve from cache.
    ///
    /// The Responses API reports `input_tokens` as the whole figure and cached tokens as a subset
    /// of it, so the uncached count this rate applies to is the difference — see [`RateCard::price`].
    pub input_usd_per_mtok: f64,
    /// Input tokens the provider served from its prompt cache.
    pub cached_input_usd_per_mtok: f64,
    /// Output tokens, including any reasoning tokens the provider billed as output.
    pub output_usd_per_mtok: f64,
}

/// Rates for one model, converted once into pico-dollars per token.
///
/// Integers, so that a run's cost is exact arithmetic rather than a float sum whose result depends
/// on the order the turns arrived in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    input: u64,
    cached_input: u64,
    output: u64,
}

/// Rates for some set of models, and where they came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateCard {
    /// Where these rates came from, in the operator's own words.
    ///
    /// Required and free text. A price with no provenance is a number nobody can check, and this
    /// string is what a reader of the record follows to check it.
    pub source: String,
    /// The day the rates were read, as `YYYY-MM-DD`.
    ///
    /// A rate is a fact with a date. Without one, a figure from a card written a year ago is
    /// indistinguishable from one written this morning.
    pub as_of: String,
    /// One entry per model. A model with no entry is a model this card does not price.
    pub models: BTreeMap<String, ModelRates>,
}

/// Why a rate card was refused. Every variant refuses the run rather than pricing part of it.
///
/// No `Eq`: [`RateCardError::BadRate`] carries the rate as it was written, and a float that
/// compares equal to another float is a stronger claim than this crate has any reason to make.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RateCardError {
    #[error("the rate card is not readable JSON: {0}")]
    Unreadable(String),
    #[error("the rate card names no `source`; a price with no provenance cannot be checked")]
    MissingSource,
    #[error("`as_of` is `{0}`, which is not a `YYYY-MM-DD` date")]
    BadDate(String),
    #[error("`{model}.{field}` is `{value}`, which is not a rate this card can hold")]
    BadRate {
        model: String,
        field: &'static str,
        value: f64,
    },
}

impl RateCard {
    /// Reads a card, refusing every rate it cannot hold exactly.
    ///
    /// # Errors
    ///
    /// Returns [`RateCardError`] for unreadable JSON, a missing source, a malformed date, or a rate
    /// that is negative, not finite, or too large to convert. Nothing is priced from a card that
    /// failed here: a partly-valid card would report a figure for some turns and silence for
    /// others, and the silence would read as free.
    pub fn parse(text: &str) -> Result<Self, RateCardError> {
        let card: Self = serde_json::from_str(text)
            .map_err(|error| RateCardError::Unreadable(error.to_string()))?;
        if card.source.trim().is_empty() {
            return Err(RateCardError::MissingSource);
        }
        if !is_iso_date(&card.as_of) {
            return Err(RateCardError::BadDate(card.as_of.clone()));
        }
        for (model, rates) in &card.models {
            for (field, value) in [
                ("input_usd_per_mtok", rates.input_usd_per_mtok),
                ("cached_input_usd_per_mtok", rates.cached_input_usd_per_mtok),
                ("output_usd_per_mtok", rates.output_usd_per_mtok),
            ] {
                if pico_per_token(value).is_none() {
                    return Err(RateCardError::BadRate {
                        model: model.clone(),
                        field,
                        value,
                    });
                }
            }
        }
        Ok(card)
    }

    /// The rates this card holds for one model, or `None` when it holds none.
    #[must_use]
    pub fn rates_for(&self, model: &str) -> Option<Rates> {
        let written = self.models.get(model)?;
        Some(Rates {
            input: pico_per_token(written.input_usd_per_mtok)?,
            cached_input: pico_per_token(written.cached_input_usd_per_mtok)?,
            output: pico_per_token(written.output_usd_per_mtok)?,
        })
    }

    /// Every model this card prices, in order. What a run reports when it cannot price its own.
    pub fn priced_models(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(String::as_str)
    }

    /// What one turn's reported tokens cost, in millionths of a US dollar.
    ///
    /// Priced against the model the **provider** reported for that turn rather than the one the run
    /// asked for: an endpoint that served a different model billed for the one it served, and
    /// pricing the request would state a figure nobody was charged.
    ///
    /// `None` when this card does not price that model. Never `Some(0)` for an unpriced model — a
    /// zero is a claim that the turn was free.
    #[must_use]
    pub fn price(&self, usage: &Usage) -> Option<u64> {
        let rates = self.rates_for(&usage.model)?;
        // Cached tokens are a subset of `input_tokens` on this wire, so charging both figures at
        // the input rate would bill the cached ones twice — at the dearer rate, which is the
        // direction that flatters nothing and misleads a comparison.
        let uncached = u128::from(usage.input_tokens.saturating_sub(usage.cached_input_tokens));
        let pico = uncached * u128::from(rates.input)
            + u128::from(usage.cached_input_tokens) * u128::from(rates.cached_input)
            + u128::from(usage.output_tokens) * u128::from(rates.output);
        Some(to_micro(pico))
    }
}

/// A rate in dollars per million tokens, as pico-dollars per token.
///
/// `None` for anything not finite, negative, or beyond what the conversion can hold exactly. Real
/// published rates carry at most six decimal places, which is exactly what survives here.
fn pico_per_token(usd_per_mtok: f64) -> Option<u64> {
    if !usd_per_mtok.is_finite() || usd_per_mtok < 0.0 {
        return None;
    }
    let pico = usd_per_mtok * PICO_PER_USD_PER_MTOK;
    if pico > 1e18 {
        return None;
    }
    // A rate that does not land on a pico-dollar is one this card cannot state exactly, and
    // silently rounding it would put a figure in the record that the rate does not support.
    if (pico - pico.round()).abs() > 1e-6 {
        return None;
    }
    // Through the decimal rather than through `as`: a float cast to an integer is a lossy
    // conversion that reports nothing when it loses something, and this is the one place the
    // operator's own number becomes the arithmetic every figure below is built on.
    format!("{pico:.0}").parse().ok()
}

/// Millionths of a US dollar as a plain decimal, exactly.
///
/// Integer arithmetic rather than a division into a float. Every micro-dollar is representable at
/// six decimal places, so the figure a person reads and the figure in the record are the same
/// number rather than two that agree to within a rounding.
#[must_use]
pub fn micro_usd_as_decimal(micro_usd: u64) -> String {
    format!("{}.{:06}", micro_usd / 1_000_000, micro_usd % 1_000_000)
}

/// Rounds pico-dollars to the nearest micro-dollar, half away from zero.
fn to_micro(pico: u128) -> u64 {
    let micro = (pico + PICO_PER_MICRO / 2) / PICO_PER_MICRO;
    u64::try_from(micro).unwrap_or(u64::MAX)
}

/// `YYYY-MM-DD`, checked for shape only.
///
/// Shape and not validity: this crate has no calendar, and refusing `2026-02-30` while accepting a
/// card from the wrong year would be precision aimed at the harmless half of the problem.
fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&index| bytes[index].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> RateCard {
        RateCard::parse(
            r#"{
                "source": "a table the operator read",
                "as_of": "2026-08-24",
                "models": {
                    "m": {
                        "input_usd_per_mtok": 1.25,
                        "cached_input_usd_per_mtok": 0.125,
                        "output_usd_per_mtok": 10.0
                    }
                }
            }"#,
        )
        .expect("a valid card")
    }

    fn usage(input: u64, cached: u64, output: u64) -> Usage {
        Usage {
            model: "m".to_owned(),
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: cached,
        }
    }

    #[test]
    fn a_rate_becomes_an_exact_count_of_pico_dollars_per_token() {
        assert_eq!(pico_per_token(1.25), Some(1_250_000));
        assert_eq!(pico_per_token(0.0), Some(0));
        assert_eq!(pico_per_token(-1.0), None, "a negative rate is not a rate");
        assert_eq!(pico_per_token(f64::NAN), None);
        assert_eq!(pico_per_token(f64::INFINITY), None);
    }

    #[test]
    fn a_million_tokens_costs_exactly_what_the_card_says_a_million_tokens_costs() {
        // The property the unit conversion exists for: no drift between the rate as written and the
        // figure reported.
        assert_eq!(card().price(&usage(1_000_000, 0, 0)), Some(1_250_000));
        assert_eq!(card().price(&usage(0, 0, 1_000_000)), Some(10_000_000));
    }

    #[test]
    fn cached_tokens_are_billed_once_and_at_the_cache_rate() {
        // `input_tokens` includes the cached ones on this wire. Charging the whole figure at the
        // input rate and the cached figure again would bill a cache hit at 1.1x the miss.
        let priced = card().price(&usage(1_000_000, 800_000, 0)).expect("priced");
        assert_eq!(
            priced,
            200_000 * 1_250_000 / 1_000_000 + 800_000 * 125_000 / 1_000_000
        );
        assert_eq!(priced, 350_000, "$0.35, not $1.35");
    }

    #[test]
    fn a_provider_reporting_more_cache_than_input_bills_the_cache_and_nothing_negative() {
        // An inconsistent report is the provider's, and the run still has to state a figure. The
        // uncached share clamps at nothing rather than wrapping into an enormous charge.
        assert_eq!(
            card().price(&usage(10, 99, 0)),
            Some(12),
            "99 * $0.125/Mtok"
        );
        assert_eq!(
            card().price(&usage(0, 0, 0)),
            Some(0),
            "a turn with no tokens"
        );
    }

    #[test]
    fn a_model_the_card_does_not_price_is_absent_rather_than_free() {
        let mut other = usage(1_000_000, 0, 1_000_000);
        other.model = "somebody-elses-model".to_owned();
        assert_eq!(
            card().price(&other),
            None,
            "a zero here would say the run cost nothing"
        );
    }

    #[test]
    fn the_turn_is_priced_against_the_model_the_provider_served() {
        // The run asked for `m`; if the endpoint answers as `m-preview`, the card must miss rather
        // than bill the run at a rate nobody charged it.
        let mut served = usage(1_000, 0, 1_000);
        served.model = "m-preview".to_owned();
        assert_eq!(card().price(&served), None);
    }

    #[test]
    fn a_card_with_no_source_is_refused_rather_than_used() {
        let refused = RateCard::parse(r#"{"source": "  ", "as_of": "2026-08-24", "models": {}}"#)
            .expect_err("refused");
        assert_eq!(refused, RateCardError::MissingSource);
    }

    #[test]
    fn a_card_with_no_usable_date_is_refused_by_name() {
        let refused = RateCard::parse(r#"{"source": "s", "as_of": "last tuesday", "models": {}}"#)
            .expect_err("refused");
        assert_eq!(refused, RateCardError::BadDate("last tuesday".to_owned()));
        assert!(is_iso_date("2026-08-24"));
        assert!(!is_iso_date("2026-8-24"));
    }

    #[test]
    fn one_bad_rate_refuses_the_whole_card_rather_than_pricing_the_rest() {
        // Half a card would price some turns and go quiet on others, and the quiet ones would read
        // as free.
        let refused = RateCard::parse(
            r#"{"source": "s", "as_of": "2026-08-24", "models": {"m": {
                "input_usd_per_mtok": -1.0,
                "cached_input_usd_per_mtok": 0.0,
                "output_usd_per_mtok": 0.0
            }}}"#,
        )
        .expect_err("refused");
        assert_eq!(
            refused,
            RateCardError::BadRate {
                model: "m".to_owned(),
                field: "input_usd_per_mtok",
                value: -1.0,
            }
        );
    }

    #[test]
    fn an_unknown_field_is_refused_so_a_typo_is_not_a_silent_zero() {
        let refused = RateCard::parse(
            r#"{"source": "s", "as_of": "2026-08-24", "models": {"m": {
                "input_usd_per_mtok": 1.0,
                "cached_input_usd_per_mtok": 0.0,
                "output_usd_per_mtok": 1.0,
                "reasoning_usd_per_mtok": 5.0
            }}}"#,
        )
        .expect_err("refused");
        assert!(
            matches!(refused, RateCardError::Unreadable(_)),
            "{refused:?}"
        );
    }

    #[test]
    fn the_card_can_list_what_it_prices_so_a_miss_can_name_the_alternatives() {
        assert_eq!(card().priced_models().collect::<Vec<_>>(), vec!["m"]);
    }

    #[test]
    fn rounding_lands_on_the_nearer_micro_dollar() {
        assert_eq!(to_micro(1_499_999), 1);
        assert_eq!(to_micro(1_500_000), 2);
    }

    #[test]
    fn a_figure_reads_the_same_as_it_is_stored() {
        assert_eq!(micro_usd_as_decimal(106_233), "0.106233");
        assert_eq!(micro_usd_as_decimal(0), "0.000000");
        assert_eq!(micro_usd_as_decimal(1_000_000), "1.000000");
        assert_eq!(micro_usd_as_decimal(12_345_678), "12.345678");
    }
}
