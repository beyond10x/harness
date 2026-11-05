//! The wires compaction knows the reasoning of are the wires the shipped model port speaks.
//!
//! `harness-loop` names, by identifier, the wires whose opaque reasoning items a compaction may
//! drop; `harness-llm` names the identifiers its wires tag their opaque items with. Neither crate
//! depends on the other, so this crate, which depends on both, is where the two lists meet. A wire
//! renamed on one side only would leave every reasoning item of that wire kept through every
//! compaction, and nothing else would say so.

#[test]
fn every_wire_the_model_port_speaks_is_one_whose_reasoning_compaction_recognises() {
    let known: Vec<&str> = harness_loop::reasoning_wires().collect();
    for wire in [harness_llm::RESPONSES_WIRE, harness_llm::MESSAGES_WIRE] {
        assert!(
            known.contains(&wire),
            "`{wire}` tags the opaque items harness-llm produces, and compaction does not know \
             its reasoning; it knows {known:?}"
        );
    }
}
