//! The capability predicates the wire contract carries, evaluated against a machine's facts.
//!
//! These are **substrate's** predicates, read out of its own operations document, not a policy
//! written here. A second policy would be a second thing to keep true.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Facts;

/// How a predicate compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateOp {
    /// The fact equals a literal.
    Eq,
    /// The fact is at least what the caller asked for — a ceiling the machine declares, checked
    /// against the number in the request.
    Gte,
    /// What the caller asked for is one of the values the fact lists.
    OneOf,
}

/// A condition on when a predicate applies at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct When {
    /// Where in the input to look.
    pub input_pointer: String,
    /// What it has to be.
    pub equals: Value,
}

/// One capability predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Predicate {
    /// The fact it reads.
    pub fact: String,
    /// How it compares.
    pub op: PredicateOp,
    /// The literal it compares against, for [`PredicateOp::Eq`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Where in the request the compared number lives, for [`PredicateOp::Gte`] and
    /// [`PredicateOp::OneOf`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_pointer: Option<String>,
    /// When this predicate applies at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

/// A predicate that did not hold, and everything a reader needs to act on it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "this machine does not admit it: `{fact}` {wanted}, and the machine says {found}. \
     Nothing that needs it is published here."
)]
pub struct Unmet {
    /// The fact that decided it.
    pub fact: String,
    /// What the predicate wanted, in words.
    pub wanted: String,
    /// What the machine actually said — `nothing` when it stated no such fact.
    pub found: String,
}

impl Predicate {
    /// Whether this predicate holds for `input` on a machine with these `facts`.
    ///
    /// # Errors
    ///
    /// Returns [`Unmet`], naming the fact, the want and the answer. An absent fact is a failure and
    /// never a pass: *the machine did not say* and *the machine said yes* are the two things a
    /// publication gate must never confuse, and substrate's own position is that missing
    /// confinement facts mean the operation is unavailable.
    pub fn check(&self, facts: &Facts, input: &Value) -> Result<(), Unmet> {
        if let Some(when) = &self.when {
            let at = input.pointer(&when.input_pointer);
            if at != Some(&when.equals) {
                return Ok(());
            }
        }

        let found = facts.get(&self.fact);
        let describe = |value: Option<&Value>| {
            value.map_or_else(|| "nothing".to_owned(), std::string::ToString::to_string)
        };

        match self.op {
            PredicateOp::Eq => {
                let wanted = self.value.as_ref();
                if found.is_some() && found == wanted {
                    return Ok(());
                }
                Err(Unmet {
                    fact: self.fact.clone(),
                    wanted: format!("must be {}", describe(wanted)),
                    found: describe(found),
                })
            }
            PredicateOp::Gte => {
                let asked = self
                    .input_pointer
                    .as_ref()
                    .and_then(|pointer| input.pointer(pointer))
                    .and_then(Value::as_u64);
                let Some(asked) = asked else {
                    // Nothing was asked for, so no ceiling can be exceeded. The predicate is about
                    // a number in the request; a request without one is not a request this
                    // predicate has anything to say about.
                    return Ok(());
                };
                match found.and_then(Value::as_u64) {
                    Some(ceiling) if ceiling >= asked => Ok(()),
                    other => Err(Unmet {
                        fact: self.fact.clone(),
                        wanted: format!("must be at least the {asked} that was asked for"),
                        found: other.map_or_else(|| describe(found), |value| value.to_string()),
                    }),
                }
            }
            PredicateOp::OneOf => {
                let asked = self
                    .input_pointer
                    .as_ref()
                    .and_then(|pointer| input.pointer(pointer));
                let Some(asked) = asked else { return Ok(()) };
                match found.and_then(Value::as_array) {
                    Some(admitted) if admitted.contains(asked) => Ok(()),
                    _ => Err(Unmet {
                        fact: self.fact.clone(),
                        wanted: format!("must list {asked}"),
                        found: describe(found),
                    }),
                }
            }
        }
    }
}
