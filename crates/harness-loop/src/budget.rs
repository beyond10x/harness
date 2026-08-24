use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Upper bounds on one run.
///
/// Every field here is one this loop counts itself. That is the difference from driving someone
/// else's harness: a bound we cannot observe is a bound we cannot honour, so rather than accept it
/// and hope, [`Budget::validate`] refuses it by name before the first request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Model turns, counting the first. A tool round trip costs one turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u64>,
    /// Total reported input tokens across the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// Total reported output tokens across the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Output tokens offered to the provider for any single turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens_per_turn: Option<u64>,
    /// Wall-clock ceiling, checked between turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_ms: Option<u64>,
    /// Total spend across the run, in millionths of a US dollar.
    ///
    /// Enforceable **only for a run carrying a rate card that prices its model** — see
    /// [`crate::RateCard`]. Without one, nothing converts tokens to money and this is refused by
    /// name rather than ignored, because silently dropping it would let a caller believe a spend
    /// ceiling is in force when none is.
    ///
    /// The field was refused unconditionally until rate cards existed, on the reasoning that a
    /// gateway relays bytes and reports no price. Half of that is still true — the provider returns
    /// no price — and the other half was wrong: the price exists, and every other harness in the
    /// comparison states one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_microunits: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    #[error("`{name}` is zero, which admits nothing; omit it instead")]
    Zero { name: &'static str },
    #[error("`{name}` cannot be enforced by this harness and was refused rather than ignored")]
    Unenforceable { name: &'static str },
}

impl Budget {
    /// Checks every bound before the first request.
    ///
    /// `priced` says whether the run can compute what it spends — a rate card is loaded and it
    /// covers this model. A spend ceiling is only a ceiling where that holds.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError::Zero`] for a bound that admits nothing and
    /// [`BudgetError::Unenforceable`] for one this run cannot observe.
    pub fn validate(&self, priced: bool) -> Result<(), BudgetError> {
        for (name, value) in [
            ("max_turns", self.max_turns),
            ("max_input_tokens", self.max_input_tokens),
            ("max_output_tokens", self.max_output_tokens),
            (
                "max_output_tokens_per_turn",
                self.max_output_tokens_per_turn,
            ),
            ("max_duration_ms", self.max_duration_ms),
            ("max_cost_microunits", self.max_cost_microunits),
        ] {
            if value == Some(0) {
                return Err(BudgetError::Zero { name });
            }
        }
        if self.max_cost_microunits.is_some() && !priced {
            return Err(BudgetError::Unenforceable {
                name: "max_cost_microunits",
            });
        }
        Ok(())
    }

    pub fn max_duration(&self) -> Option<Duration> {
        self.max_duration_ms.map(Duration::from_millis)
    }

    #[must_use]
    pub fn with_max_turns(mut self, turns: u64) -> Self {
        self.max_turns = Some(turns);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_budget_is_valid() {
        assert!(Budget::default().validate(false).is_ok());
    }

    #[test]
    fn a_zero_bound_refuses_by_name() {
        let budget = Budget {
            max_turns: Some(0),
            ..Budget::default()
        };
        assert_eq!(
            budget.validate(false).expect_err("zero refuses"),
            BudgetError::Zero { name: "max_turns" }
        );
    }

    #[test]
    fn a_cost_ceiling_on_an_unpriced_run_is_refused_rather_than_ignored() {
        let budget = Budget {
            max_cost_microunits: Some(1_000),
            ..Budget::default()
        };
        assert_eq!(
            budget.validate(false).expect_err("cost refuses"),
            BudgetError::Unenforceable {
                name: "max_cost_microunits"
            }
        );
    }

    #[test]
    fn a_cost_ceiling_stands_once_the_run_can_price_itself() {
        let budget = Budget {
            max_cost_microunits: Some(1_000),
            ..Budget::default()
        };
        assert!(budget.validate(true).is_ok());
    }

    #[test]
    fn a_zero_cost_ceiling_refuses_even_where_it_could_be_enforced() {
        // Zero admits nothing, and a caller who meant "spend nothing" wants a run that never
        // started rather than one that stops after the turn it already paid for.
        let budget = Budget {
            max_cost_microunits: Some(0),
            ..Budget::default()
        };
        assert_eq!(
            budget.validate(true).expect_err("zero refuses"),
            BudgetError::Zero {
                name: "max_cost_microunits"
            }
        );
    }

    #[test]
    fn duration_converts_from_milliseconds() {
        let budget = Budget {
            max_duration_ms: Some(1_500),
            ..Budget::default()
        };
        assert_eq!(budget.max_duration(), Some(Duration::from_millis(1_500)));
        assert_eq!(Budget::default().max_duration(), None);
    }
}
