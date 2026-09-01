use super::*;

fn flow(yaml: &str) -> Flow {
    Flow::from_yaml(yaml).expect("the fixture is a flow")
}

/// A document committed beside these tests.
fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name),
    )
    .expect("the committed fixture")
}

/// The shape the crate documentation shows: a step, a sub-tree, and a step that needs the sub-tree.
const NESTED: &str = r"
id: development
root:
  id: root
  nodes:
    - id: receive
    - id: shape
      needs: [receive]
      nodes:
        - id: specify
        - id: decompose
          needs: [specify]
    - id: implement
      needs: [shape]
";

struct Always(StepOutcome);

impl StepRunner for Always {
    fn run(&mut self, _path: &str, _step: &Step, _cx: &StepContext) -> StepOutcome {
        self.0.clone()
    }
}

/// Fails exactly the steps whose path ends with one of these names.
struct FailsAt(Vec<&'static str>);

impl StepRunner for FailsAt {
    fn run(&mut self, path: &str, _step: &Step, _cx: &StepContext) -> StepOutcome {
        if self.0.iter().any(|name| path.ends_with(name)) {
            StepOutcome::Failed
        } else {
            StepOutcome::Passed
        }
    }
}

#[test]
fn a_group_is_one_node_to_its_siblings_and_a_whole_plan_inside() {
    let flow = flow(NESTED);
    let plan = flow.plan().expect("valid");

    assert_eq!(
        plan.layers,
        vec![
            Layer {
                nodes: vec!["receive".to_owned()]
            },
            Layer {
                nodes: vec!["shape".to_owned()]
            },
            Layer {
                nodes: vec!["implement".to_owned()]
            },
        ],
        "the root sees three nodes, one of which happens to hold two"
    );
    let shape = plan.groups.get("shape").expect("the sub-tree was planned");
    assert_eq!(shape.path, "root.shape");
    assert_eq!(shape.depth(), 2, "and inside it, its own two layers");
    assert_eq!(
        flow.steps(),
        vec![
            "root.receive",
            "root.shape.specify",
            "root.shape.decompose",
            "root.implement",
        ]
    );
}

#[test]
fn siblings_with_no_edge_between_them_share_a_layer() {
    let flow = flow(
        r"
id: fan
root:
  id: root
  nodes:
    - id: a
    - id: b
    - id: c
      needs: [a, b]
",
    );
    let plan = flow.plan().expect("valid");
    assert_eq!(plan.width(), 2, "a and b may run together");
    assert_eq!(plan.layers[0].nodes, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(plan.layers[1].nodes, vec!["c".to_owned()]);
}

#[test]
fn an_edge_that_reaches_into_a_group_is_refused_and_says_what_to_do_instead() {
    // The restriction the whole design rests on. `implement` may depend on `shape`; it may not
    // depend on `specify`, which is inside `shape`.
    let flow = flow(
        r"
id: reach
root:
  id: root
  nodes:
    - id: shape
      nodes:
        - id: specify
    - id: implement
      needs: [specify]
",
    );
    let error = flow.plan().expect_err("refused");
    let message = error.to_string();
    assert!(matches!(error, FlowError::UnknownNeed { .. }), "{message}");
    assert!(message.contains("root.implement"), "names where: {message}");
    assert!(message.contains("specify"), "names what: {message}");
    assert!(
        message.contains("depend on the group"),
        "and what to do instead: {message}"
    );
}

#[test]
fn a_cycle_is_found_in_the_group_that_holds_it_and_named_by_path() {
    let flow = flow(
        r"
id: loop
root:
  id: root
  nodes:
    - id: outer
      nodes:
        - id: a
          needs: [b]
        - id: b
          needs: [a]
",
    );
    let error = flow.plan().expect_err("refused");
    let message = error.to_string();
    assert!(matches!(error, FlowError::Cycle { .. }), "{message}");
    assert!(
        message.contains("root.outer"),
        "a cycle is local to one group, and the message says which: {message}"
    );
    assert!(message.contains("a -> b"), "{message}");
}

#[test]
fn a_name_repeated_among_siblings_is_refused_and_the_same_name_in_two_groups_is_not() {
    let clash = flow(
        r"
id: clash
root:
  id: root
  nodes:
    - id: a
    - id: a
",
    );
    assert!(matches!(
        clash.plan().expect_err("refused"),
        FlowError::DuplicateId { .. }
    ));

    // Scoping is the point: `check` inside two sub-trees is two different steps.
    let scoped = flow(
        r"
id: scoped
root:
  id: root
  nodes:
    - id: left
      nodes: [{id: check}]
    - id: right
      nodes: [{id: check}]
",
    );
    assert_eq!(scoped.steps(), vec!["root.left.check", "root.right.check"]);
}

#[test]
fn a_name_carrying_a_dot_is_refused_because_a_path_is_made_of_them() {
    // `root.shape.specify` is a name joined to its ancestors with the character this refuses.
    // A section called `shape.specify` sitting beside a group `shape` holding a step `specify`
    // would read as the same path everywhere the walk names one — and a caller that files a
    // session per (scope, attempt) would file both of them into one.
    let dotted = flow(
        r"
id: dotted
root:
  id: root
  nodes:
    - id: shape.specify
",
    );
    let error = dotted.plan().expect_err("refused");
    assert!(matches!(error, FlowError::DottedName { .. }), "{error}");
    let message = error.to_string();
    assert!(message.contains("`shape.specify`"), "{message}");
    assert!(
        message.contains("`root`"),
        "and where it was found: {message}"
    );

    // The root is where a path starts, and it is nobody's sibling, so it is read on its own.
    let dotted_root = flow(
        r"
id: dotted-root
root:
  id: root.shape
  nodes:
    - id: specify
",
    );
    assert!(matches!(
        dotted_root.plan().expect_err("refused"),
        FlowError::DottedName { .. }
    ));
}

#[test]
fn an_empty_group_is_refused_rather_than_walked_over() {
    let flow = flow(
        r"
id: hollow
root:
  id: root
  nodes:
    - id: nothing
      nodes: []
",
    );
    assert!(matches!(
        flow.plan().expect_err("refused"),
        FlowError::EmptyGroup { .. }
    ));
}

#[test]
fn a_walk_runs_every_step_in_plan_order_and_nests_its_report() {
    let flow = flow(NESTED);
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(&mut Always(StepOutcome::Passed), &mut sink)
        .expect("valid");

    assert_eq!(report.ran, 4);
    assert!(report.clean());
    assert_eq!(
        sink.steps_started(),
        vec![
            "root.receive",
            "root.shape.specify",
            "root.shape.decompose",
            "root.implement",
        ]
    );
    // The nesting is in the stream, not only in the document.
    let entered: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            FlowEvent::GroupEntered { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(entered, vec!["root", "root.shape"]);
}

#[test]
fn an_operator_step_pauses_the_walk_without_finishing_or_skipping_anything_after_it() {
    struct PausesAt;

    impl StepRunner for PausesAt {
        fn run(&mut self, path: &str, _step: &Step, _cx: &StepContext) -> StepOutcome {
            if path == "root.shape.specify" {
                StepOutcome::Paused {
                    reason: "approve the specification".to_owned(),
                }
            } else {
                StepOutcome::Passed
            }
        }
    }

    let flow = flow(NESTED);
    let mut sink = VecFlowSink::new();
    let report = flow.run(&mut PausesAt, &mut sink).expect("valid");

    assert_eq!(report.status(), FlowStatus::AwaitingOperator);
    assert_eq!(
        report.reached, 2,
        "receive finished and specify was reached"
    );
    assert_eq!(report.ran, 1, "an awaiting step has not run");
    assert_eq!(report.failed, 0);
    assert_eq!(report.skipped, 0, "the rest is pending, not skipped");
    assert_eq!(report.retreats, 0);
    assert_eq!(
        sink.steps_started(),
        vec!["root.receive", "root.shape.specify"]
    );
    assert!(matches!(
        sink.events().last(),
        Some(FlowEvent::FlowPaused {
            flow,
            path,
            reason,
            reached: 2,
            failed: 0,
            skipped: 0,
            retreats: 0,
        }) if flow == "root"
            && path == "root.shape.specify"
            && reason == "approve the specification"
    ));
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        FlowEvent::StepFinished { path, .. } if path == "root.shape.specify"
    )));
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        FlowEvent::GroupLeft { path, .. } if path == "root.shape" || path == "root"
    )));
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, FlowEvent::FlowFinished { .. }))
    );
}

#[test]
fn a_failed_step_stops_what_needed_it_and_leaves_the_rest_alone() {
    let flow = flow(
        r"
id: partial
root:
  id: root
  nodes:
    - id: a
    - id: b
      needs: [a]
    - id: c
",
    );
    let mut sink = VecFlowSink::new();
    let report = flow.run(&mut FailsAt(vec!["a"]), &mut sink).expect("valid");

    assert_eq!(report.ran, 2, "a and c ran");
    assert_eq!(report.failed, 1);
    assert_eq!(report.skipped, 1, "b did not");
    assert_eq!(sink.steps_started(), vec!["root.a", "root.c"]);
    let skipped: Vec<&FlowEvent> = sink
        .events()
        .iter()
        .filter(|event| matches!(event, FlowEvent::NodeSkipped { .. }))
        .collect();
    assert_eq!(skipped.len(), 1);
    let FlowEvent::NodeSkipped { path, because } = skipped[0] else {
        unreachable!()
    };
    assert_eq!(path, "root.b");
    assert!(
        because.contains('a'),
        "the reason names the blocker: {because}"
    );
}

#[test]
fn a_group_that_failed_inside_is_failed_to_its_siblings() {
    // The other half of opacity. `implement` cannot see `specify`, so what it waits on is whether
    // `shape` came out clean — and a step buried three levels down must be able to stop it.
    let flow = flow(NESTED);
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(&mut FailsAt(vec!["specify"]), &mut sink)
        .expect("valid");

    assert_eq!(
        sink.steps_started(),
        vec!["root.receive", "root.shape.specify"]
    );
    assert_eq!(report.failed, 1);
    assert_eq!(
        report.skipped, 2,
        "decompose inside the group, and implement outside it"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            FlowEvent::GroupLeft { path, failed: true, .. } if path == "root.shape"
        )),
        "the group reports itself failed on the way out"
    );
}

#[test]
fn skipping_a_group_names_every_step_inside_it_rather_than_the_group_alone() {
    let flow = flow(
        r"
id: cascade
root:
  id: root
  nodes:
    - id: gate
    - id: rest
      needs: [gate]
      nodes:
        - id: one
        - id: two
",
    );
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(&mut FailsAt(vec!["gate"]), &mut sink)
        .expect("valid");
    assert_eq!(
        report.skipped, 2,
        "both steps inside the skipped group count"
    );
    let skipped: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            FlowEvent::NodeSkipped { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(skipped, vec!["root.rest", "root.rest.one", "root.rest.two"]);
}

/// Keeps every `run:` block it is handed, so a test can assert what reached the runner.
struct Capture(Vec<serde_json::Value>);

impl StepRunner for Capture {
    fn run(&mut self, _path: &str, step: &Step, _cx: &StepContext) -> StepOutcome {
        self.0.push(step.run.clone());
        StepOutcome::Passed
    }
}

#[test]
fn a_step_payload_is_carried_and_never_read() {
    let flow = flow(
        r"
id: opaque
root:
  id: root
  nodes:
    - id: only
      run: {prompt: 'do the thing', tools: [a, b]}
",
    );
    let mut capture = Capture(Vec::new());
    flow.run(&mut capture, &mut VecFlowSink::new())
        .expect("valid");
    assert_eq!(capture.0.len(), 1);
    assert_eq!(capture.0[0]["prompt"], "do the thing");
    assert_eq!(capture.0[0]["tools"][1], "b");
}

#[test]
fn a_flow_without_an_id_names_nothing_and_is_refused() {
    let flow = flow(
        r"
id: '  '
root:
  id: root
  nodes: [{id: a}]
",
    );
    assert!(matches!(
        flow.plan().expect_err("refused"),
        FlowError::NoId { .. }
    ));
}

#[test]
fn a_node_that_needs_itself_is_refused_before_the_cycle_check_reaches_it() {
    let flow = flow(
        r"
id: self
root:
  id: root
  nodes:
    - id: a
      needs: [a]
",
    );
    assert!(matches!(
        flow.plan().expect_err("refused"),
        FlowError::SelfNeed { .. }
    ));
}

// --- the retreat --------------------------------------------------------------------------------

/// Fails the named step until it has been attempted `heal_after` times, then passes it.
struct HealsAfter {
    step: &'static str,
    heal_after: usize,
    seen: usize,
}

impl StepRunner for HealsAfter {
    fn run(&mut self, path: &str, _step: &Step, _cx: &StepContext) -> StepOutcome {
        if !path.ends_with(self.step) {
            return StepOutcome::Passed;
        }
        self.seen += 1;
        if self.seen >= self.heal_after {
            StepOutcome::Passed
        } else {
            StepOutcome::Failed
        }
    }
}

/// `adp/default/2`'s retreat, as this notation writes it: `verify -> implement` is a group that
/// repeats, not an edge that goes backwards.
const RETREAT: &str = r"
id: development
root:
  id: root
  nodes:
    - id: build
      repeat: {max: 3}
      nodes:
        - id: implement
        - id: verify
          needs: [implement]
    - id: review
      needs: [build]
";

#[test]
fn a_group_that_did_not_come_out_clean_runs_again_and_the_whole_scope_re_runs() {
    // The rule the design turns on: a retreat re-enters the *scope*. A run that went back to
    // `implement` and did not re-verify would have skipped a check, not retreated.
    let flow = flow(RETREAT);
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(
            &mut HealsAfter {
                step: "verify",
                heal_after: 2,
                seen: 0,
            },
            &mut sink,
        )
        .expect("valid");

    assert_eq!(
        sink.steps_started(),
        vec![
            "root.build.implement",
            "root.build.verify",
            "root.build.implement",
            "root.build.verify",
            "root.review",
        ],
        "implement ran again, not only verify"
    );
    assert_eq!(report.failed, 1, "the first verify");
    assert_eq!(report.ran, 5);
    assert_eq!(
        report.skipped, 0,
        "review was not skipped: the group came out clean in the end"
    );
    assert!(report.clean(), "the outcome is the verdict, not the tally");
    assert_eq!(report.retreats, 1);
}

#[test]
fn the_bound_is_a_number_in_the_document_and_exhausting_it_is_said_out_loud() {
    let flow = flow(RETREAT);
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(&mut FailsAt(vec!["verify"]), &mut sink)
        .expect("valid");

    assert_eq!(
        sink.steps_started().len(),
        6,
        "three attempts of two steps, and then review is skipped"
    );
    assert_eq!(report.failed, 3);
    assert_eq!(report.skipped, 1, "review");

    let left: Vec<&FlowEvent> = sink
        .events()
        .iter()
        .filter(|event| matches!(event, FlowEvent::GroupLeft { path, .. } if path == "root.build"))
        .collect();
    assert_eq!(
        left.len(),
        1,
        "a repeated group is left once, not once per attempt"
    );
    let FlowEvent::GroupLeft {
        failed,
        attempts,
        exhausted,
        ..
    } = left[0]
    else {
        unreachable!()
    };
    assert!(failed);
    assert_eq!(*attempts, 3);
    assert!(
        exhausted,
        "*it broke* and *it kept breaking until the document stopped it* are different facts"
    );

    let repeats = sink
        .events()
        .iter()
        .filter(|event| matches!(event, FlowEvent::GroupRepeating { .. }))
        .count();
    assert_eq!(repeats, 2, "two retreats between three attempts");
}

#[test]
fn a_group_that_comes_out_clean_first_time_does_not_repeat_and_says_which_attempt_it_was() {
    let flow = flow(RETREAT);
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(&mut Always(StepOutcome::Passed), &mut sink)
        .expect("valid");

    assert!(report.clean());
    assert_eq!(report.ran, 3);
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, FlowEvent::GroupRepeating { .. })),
        "a bound is a ceiling, not a quota"
    );
    // Always present, so *first time* is read the same way as *third time*.
    assert!(sink.events().iter().any(|event| matches!(
        event,
        FlowEvent::GroupEntered { path, attempt: 1, of: 3, .. } if path == "root.build"
    )));
}

#[test]
fn a_group_without_repeat_runs_once_and_is_not_exhausted_when_it_fails() {
    let flow = flow(
        r"
id: once
root:
  id: root
  nodes:
    - id: build
      nodes: [{id: verify}]
",
    );
    let mut sink = VecFlowSink::new();
    flow.run(&mut FailsAt(vec!["verify"]), &mut sink)
        .expect("valid");
    let FlowEvent::GroupLeft {
        attempts,
        exhausted,
        failed,
        ..
    } = sink
        .events()
        .iter()
        .find(|event| matches!(event, FlowEvent::GroupLeft { path, .. } if path == "root.build"))
        .expect("left")
    else {
        unreachable!()
    };
    assert!(failed);
    assert_eq!(*attempts, 1);
    assert!(
        !exhausted,
        "a group that was never allowed to retry did not use up a budget it never had"
    );
}

#[test]
fn a_group_that_may_run_zero_times_is_refused_by_name() {
    let flow = flow(
        r"
id: never
root:
  id: root
  nodes:
    - id: build
      repeat: {max: 0}
      nodes: [{id: a}]
",
    );
    let error = flow.plan().expect_err("refused");
    assert!(matches!(error, FlowError::RepeatsNever { .. }), "{error}");
    assert!(error.to_string().contains("root.build"), "{error}");
}

#[test]
fn a_nested_repeat_is_the_inner_groups_business_and_the_outer_one_counts_attempts_of_it() {
    // An inner retreat exhausting itself fails the inner group, which is one failed attempt of the
    // outer one — the same opacity rule the rest of the crate rests on, applied to attempts.
    let flow = flow(
        r"
id: nested
root:
  id: root
  nodes:
    - id: outer
      repeat: {max: 2}
      nodes:
        - id: inner
          repeat: {max: 2}
          nodes: [{id: flaky}]
",
    );
    let mut sink = VecFlowSink::new();
    let report = flow
        .run(&mut FailsAt(vec!["flaky"]), &mut sink)
        .expect("valid");
    assert_eq!(report.ran, 4, "two attempts of the inner group, twice");
    let outer_attempts = sink
        .events()
        .iter()
        .filter(
            |event| matches!(event, FlowEvent::GroupEntered { path, .. } if path == "root.outer"),
        )
        .count();
    assert_eq!(outer_attempts, 2);
}

// --- the other side of a real projection --------------------------------------------------------

#[test]
fn a_real_workflow_projected_by_another_component_plans_here() {
    // `AEP`' own development workflow, as `aep workflow flow` emits it. The
    // fixture is committed rather than generated, so this repository stays free of a dependency on
    // that one and a change on either side shows up as a diff rather than as a surprise at runtime.
    //
    // What it pins is the contract between the two notations: a state graph with three back-edges
    // arrives here as a chain of sections with one repeating sub-tree of sections, and this crate
    // plans it without complaint. **Every state is a section** — a group of one when the state has
    // one step — because the runner asks its governor at group boundaries and nowhere else, and a
    // state that were a bare step would be a state nobody is asked about.
    let flow = Flow::from_yaml(&fixture("adp-default.projected.yaml")).expect("a flow");
    let plan = flow.plan().expect("it plans");

    assert_eq!(
        plan.layers.len(),
        5,
        "four states before the retreat, and the retreat itself"
    );
    assert_eq!(plan.width(), 1, "a workflow is a chain at the top level");

    let retreat = plan
        .groups
        .get("implement-to-review")
        .expect("the retreating group");
    assert_eq!(retreat.attempts, 3, "the bound the projection was given");
    assert_eq!(
        retreat.depth(),
        4,
        "implement, verify, adversarial_verify, review - in that order"
    );
    for state in ["receive", "specify", "decompose", "establish_verifiers"] {
        let section = plan
            .groups
            .get(state)
            .unwrap_or_else(|| panic!("`{state}` is a section"));
        assert_eq!(section.attempts, 1, "`{state}`: only the retreat repeats");
    }
    for state in ["implement", "verify", "adversarial_verify", "review"] {
        let section = retreat
            .groups
            .get(state)
            .unwrap_or_else(|| panic!("`{state}` is a section inside the retreat"));
        assert_eq!(section.attempts, 1, "`{state}`: only the retreat repeats");
        assert_eq!(section.path, format!("root.implement-to-review.{state}"));
    }
    assert_eq!(
        flow.steps(),
        vec![
            "root.receive.receive-1",
            "root.specify.specify-1",
            "root.decompose.decompose-1",
            "root.establish_verifiers.establish_verifiers-1",
            "root.implement-to-review.implement.implement-1",
            "root.implement-to-review.verify.verify-1",
            "root.implement-to-review.adversarial_verify.adversarial_verify-1",
            "root.implement-to-review.review.review-1",
        ]
    );
}

#[test]
fn the_projected_workflow_retreats_when_verification_fails() {
    // The behaviour the whole translation exists to preserve: a red suite sends the work back to
    // `implement`, and the states after it run again rather than being skipped.
    let flow = Flow::from_yaml(&fixture("adp-default.projected.yaml")).expect("a flow");

    let mut sink = VecFlowSink::new();
    let report = flow
        .run(
            &mut HealsAfter {
                step: "verify-1",
                heal_after: 2,
                seen: 0,
            },
            &mut sink,
        )
        .expect("valid");

    let started = sink.steps_started();
    assert_eq!(
        started
            .iter()
            .filter(|path| path.ends_with("implement-1"))
            .count(),
        2,
        "implement ran twice: {started:?}"
    );
    assert!(
        started
            .last()
            .is_some_and(|path| path.ends_with("review-1")),
        "and the run reached review in the end: {started:?}"
    );
    assert!(
        report.clean(),
        "a run that retreated and then succeeded is a successful run"
    );
    assert_eq!(report.retreats, 1);
    assert_eq!(
        (report.failed, report.skipped),
        (1, 2),
        "and the tallies still carry the failed attempt: the first verify, and the two states \
         after it that did not run that time round"
    );
}

// --- the context boundary ------------------------------------------------------------------------

/// Records the scope every step ran in and what it could see, and hands over whatever it is asked
/// for — named after the group that promised it.
#[derive(Default)]
struct Scopes {
    seen: Vec<(String, String, Vec<String>)>,
    withhold: Option<&'static str>,
}

impl StepRunner for Scopes {
    fn run(&mut self, path: &str, _step: &Step, cx: &StepContext) -> StepOutcome {
        let mut names: Vec<String> = cx.available.keys().cloned().collect();
        names.sort();
        self.seen.push((path.to_owned(), cx.scope.clone(), names));
        StepOutcome::Passed
    }

    fn handoff(&mut self, scope: &str, gives: &[NodeId]) -> Handoff {
        gives
            .iter()
            .filter(|name| Some(name.as_str()) != self.withhold)
            .map(|name| (name.clone(), serde_json::json!({"from": scope})))
            .collect()
    }
}

const SCOPED: &str = r"
id: scoped
root:
  id: root
  nodes:
    - id: shape
      gives: [specification_id]
      nodes:
        - id: specify
        - id: decompose
          needs: [specify]
    - id: build
      needs: [shape]
      gives: [diff]
      nodes:
        - id: implement
    - id: review
      needs: [build]
";

#[test]
fn steps_inside_one_group_share_a_scope_and_steps_in_another_do_not() {
    let mut runner = Scopes::default();
    let flow = flow(SCOPED);
    flow.run(&mut runner, &mut VecFlowSink::new())
        .expect("valid");

    let scopes: Vec<(&str, &str)> = runner
        .seen
        .iter()
        .map(|(path, scope, _)| (path.as_str(), scope.as_str()))
        .collect();
    assert_eq!(
        scopes,
        vec![
            ("root.shape.specify", "root.shape"),
            ("root.shape.decompose", "root.shape"),
            ("root.build.implement", "root.build"),
            ("root.review", "root"),
        ],
        "two steps of one group share one conversation; a step outside it does not"
    );
}

#[test]
fn what_crosses_a_boundary_is_the_declared_handoff_and_never_a_transcript() {
    let mut runner = Scopes::default();
    let flow = flow(SCOPED);
    flow.run(&mut runner, &mut VecFlowSink::new())
        .expect("valid");

    let available = |path: &str| -> Vec<String> {
        runner
            .seen
            .iter()
            .find(|(seen, _, _)| seen == path)
            .map(|(_, _, names)| names.clone())
            .expect("ran")
    };

    assert!(
        available("root.shape.specify").is_empty(),
        "the first group starts from nothing"
    );
    assert_eq!(
        available("root.build.implement"),
        vec!["specification_id".to_owned()],
        "and the next one starts from what the first promised - by name, not by transcript"
    );
    assert_eq!(
        available("root.review"),
        vec!["diff".to_owned(), "specification_id".to_owned()],
        "a later sibling sees everything that has crossed so far"
    );
}

#[test]
fn a_group_that_breaks_its_promise_fails_and_stops_what_needed_it() {
    // `gives` is a contract the document wrote down. A group that promised `specification_id` and
    // handed over nothing with that name has not finished, and letting its siblings run on would
    // give them a hole they cannot see.
    let mut runner = Scopes {
        withhold: Some("specification_id"),
        ..Scopes::default()
    };
    let mut sink = VecFlowSink::new();
    let report = flow(SCOPED).run(&mut runner, &mut sink).expect("valid");

    assert!(!report.clean());
    assert_eq!(report.failed, 0, "no step failed - the group did");
    assert_eq!(report.skipped, 2, "build and review");

    let incomplete = sink
        .events()
        .iter()
        .find_map(|event| match event {
            FlowEvent::HandoffIncomplete { path, missing } => Some((path.clone(), missing.clone())),
            _ => None,
        })
        .expect("said so");
    assert_eq!(incomplete.0, "root.shape");
    assert_eq!(incomplete.1, vec!["specification_id".to_owned()]);
}

#[test]
fn a_handoff_is_asked_for_once_per_attempt_that_leaves_and_not_once_per_draft() {
    // A group that retreated three times hands over what it ended up with, not three drafts of it.
    // The handoff is collected on the attempt that is about to leave — one that came out clean, or
    // the last one the document allows — so the two attempts that were going round again are never
    // asked at all.
    #[derive(Default)]
    struct Counting {
        asked: usize,
        seen: usize,
    }
    impl StepRunner for Counting {
        fn run(&mut self, path: &str, _step: &Step, _cx: &StepContext) -> StepOutcome {
            if path.ends_with("flaky") {
                self.seen += 1;
                if self.seen < 3 {
                    return StepOutcome::Failed;
                }
            }
            StepOutcome::Passed
        }
        fn handoff(&mut self, _scope: &str, gives: &[NodeId]) -> Handoff {
            self.asked += 1;
            gives
                .iter()
                .map(|name| (name.clone(), serde_json::json!(true)))
                .collect()
        }
    }

    let flow = flow(
        r"
id: retried
root:
  id: root
  nodes:
    - id: build
      repeat: {max: 3}
      gives: [diff]
      nodes: [{id: flaky}]
",
    );
    let mut runner = Counting::default();
    let report = flow
        .run(&mut runner, &mut VecFlowSink::new())
        .expect("valid");
    assert!(report.clean());
    assert_eq!(runner.seen, 3, "three attempts");
    assert_eq!(
        runner.asked, 1,
        "and one handoff — only the third attempt was ever leaving"
    );
}

#[test]
fn a_section_that_did_not_come_out_clean_hands_nothing_to_its_siblings() {
    // Two siblings with no edge between them, so the second runs whatever the first did. What it
    // can *see* is the claim: a handoff from a section that did not come out clean is a result
    // nobody accepted, and letting it cross would build the rest of the walk on a value the same
    // record calls failed.
    //
    // Both ways a section fails, because they must not differ here: its own step said no, or
    // whoever was asked declined its leave.
    #[derive(Default)]
    struct Watching {
        fails_at: Option<&'static str>,
        refuses_leave_of: Option<&'static str>,
        available: Vec<(String, Vec<String>)>,
    }
    impl StepRunner for Watching {
        fn run(&mut self, path: &str, _step: &Step, cx: &StepContext) -> StepOutcome {
            let mut names: Vec<String> = cx.available.keys().cloned().collect();
            names.sort();
            self.available.push((path.to_owned(), names));
            if self.fails_at == Some(path) {
                StepOutcome::Failed
            } else {
                StepOutcome::Passed
            }
        }
        fn handoff(&mut self, scope: &str, gives: &[NodeId]) -> Handoff {
            gives
                .iter()
                .map(|name| (name.clone(), serde_json::json!({"from": scope})))
                .collect()
        }
        fn leaving(
            &mut self,
            path: &str,
            _attempt: u32,
            _failed: bool,
            _handoff: &Handoff,
        ) -> Gate {
            if self.refuses_leave_of == Some(path) {
                return Gate::Refused {
                    reason: "that specification was never approved".to_owned(),
                };
            }
            Gate::Proceed
        }
    }

    let document = r"
id: unclean
root:
  id: root
  nodes:
    - id: shape
      gives: [specification_id]
      nodes:
        - id: specify
        - id: check
          needs: [specify]
    - id: build
      nodes:
        - id: implement
";
    let seen_by_build = |runner: &Watching| -> Vec<String> {
        runner
            .available
            .iter()
            .find(|(path, _)| path == "root.build.implement")
            .map(|(_, names)| names.clone())
            .expect("the sibling ran: it needs nothing")
    };

    // It handed over what it promised and its own step still failed.
    let mut broke = Watching {
        fails_at: Some("root.shape.check"),
        ..Watching::default()
    };
    let report = flow(document)
        .run(&mut broke, &mut VecFlowSink::new())
        .expect("valid");
    assert!(!report.clean());
    assert!(
        seen_by_build(&broke).is_empty(),
        "a failed section hands nothing on: {:?}",
        seen_by_build(&broke)
    );

    // It came out clean and whoever was asked declined the result. Same consequence, because the
    // walk has one word for a section that did not come out clean.
    let mut declined = Watching {
        refuses_leave_of: Some("root.shape"),
        ..Watching::default()
    };
    let report = flow(document)
        .run(&mut declined, &mut VecFlowSink::new())
        .expect("valid");
    assert!(!report.clean());
    assert!(
        seen_by_build(&declined).is_empty(),
        "a refused leave is a section nobody accepted: {:?}",
        seen_by_build(&declined)
    );

    // And the sibling of a section that *did* come out clean still sees what it promised, so this
    // is a rule about failure and not a handoff that stopped working.
    let mut clean = Watching::default();
    let report = flow(document)
        .run(&mut clean, &mut VecFlowSink::new())
        .expect("valid");
    assert!(report.clean());
    assert_eq!(seen_by_build(&clean), vec!["specification_id".to_owned()]);
}

#[test]
fn a_group_that_breaks_its_promise_is_not_re_entered_however_many_attempts_it_has_left() {
    // `gives` is the document's own contract, and a second attempt cannot make it truer: the
    // section came out clean and still did not produce the name it declared, which is a document
    // that does not describe what it runs rather than a run that went badly. A caller who wants
    // that retreat has the leave gate, where somebody decided it.
    let mut runner = Scopes {
        withhold: Some("specification_id"),
        ..Scopes::default()
    };
    let mut sink = VecFlowSink::new();
    let report = flow(
        r"
id: promised
root:
  id: root
  nodes:
    - id: shape
      repeat: {max: 3}
      gives: [specification_id]
      nodes:
        - id: specify
        - id: decompose
          needs: [specify]
",
    )
    .run(&mut runner, &mut sink)
    .expect("valid");

    assert_eq!(
        sink.steps_started(),
        vec!["root.shape.specify", "root.shape.decompose"],
        "one pass, though the document allowed three"
    );
    assert_eq!(report.retreats, 0);
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, FlowEvent::GroupRepeating { .. }))
            .count(),
        0,
    );
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, FlowEvent::HandoffIncomplete { .. }))
            .count(),
        1,
        "said once, when it happened"
    );
    let FlowEvent::GroupLeft {
        failed,
        attempts,
        exhausted,
        ..
    } = sink
        .events()
        .iter()
        .find(|event| matches!(event, FlowEvent::GroupLeft { path, .. } if path == "root.shape"))
        .expect("left")
    else {
        unreachable!()
    };
    assert!(failed);
    assert_eq!(*attempts, 1);
    assert!(
        !exhausted,
        "it did not use up its bound; it never asked for a second attempt"
    );
    assert!(!report.clean());
}

#[test]
fn a_step_is_told_which_attempt_of_its_scope_it_is_running_in() {
    // A runner that keeps one context per scope needs this to know whether to start a conversation
    // or continue one.
    #[derive(Default)]
    struct Attempts(Vec<u32>);
    impl StepRunner for Attempts {
        fn run(&mut self, _path: &str, _step: &Step, cx: &StepContext) -> StepOutcome {
            self.0.push(cx.attempt);
            if self.0.len() < 2 {
                StepOutcome::Failed
            } else {
                StepOutcome::Passed
            }
        }
    }
    let flow = flow(
        r"
id: attempts
root:
  id: root
  nodes:
    - id: build
      repeat: {max: 2}
      nodes: [{id: only}]
",
    );
    let mut runner = Attempts::default();
    flow.run(&mut runner, &mut VecFlowSink::new())
        .expect("valid");
    assert_eq!(runner.0, vec![1, 2]);
}

#[test]
fn the_root_cannot_promise_anything_because_there_is_nobody_on_the_other_side() {
    let flow = flow(
        r"
id: rootgives
root:
  id: root
  gives: [something]
  nodes: [{id: a}]
",
    );
    let error = flow.plan().expect_err("refused");
    assert!(matches!(error, FlowError::RootGives { .. }), "{error}");
    assert!(
        error.to_string().contains("no sibling on the other side"),
        "{error}"
    );
}

// --- reading a document ---------------------------------------------------------------------------

#[test]
fn a_document_that_cannot_be_read_is_refused_by_format_and_in_the_readers_own_words() {
    let error = Flow::from_yaml("id: [unclosed").expect_err("refused");
    assert_eq!(error.format, "YAML");
    assert!(error.to_string().contains("YAML"), "{error}");
    assert!(
        !error.message.is_empty(),
        "the reader's own message, which knows a line and a column this crate does not"
    );

    let error = Flow::from_json(r#"{"id": "trailing",}"#).expect_err("refused");
    assert_eq!(error.format, "JSON");
    assert!(error.to_string().contains("JSON"), "{error}");

    // Reading is not validating, and the two refusals are different types on purpose: a document
    // that reads and does not validate comes back from `plan`, so a caller may hold one before
    // deciding to run it.
    let flow = Flow::from_yaml("id: hollow\nroot: {id: root, nodes: []}\n").expect("it reads");
    assert!(matches!(
        flow.plan().expect_err("refused"),
        FlowError::EmptyGroup { .. }
    ));
}

#[test]
fn the_projection_reads_the_same_from_yaml_and_from_json() {
    // One notation, two readers: YAML is what a person writes and JSON is what another program
    // emits, and a workflow that could only arrive one way would push whoever generates one into
    // writing a YAML serialiser. The twin is generated from the YAML, and this is what fails when
    // somebody edits one of them alone.
    let from_yaml = Flow::from_yaml(&fixture("adp-default.projected.yaml")).expect("a flow");
    let from_json = Flow::from_json(&fixture("adp-default.projected.json")).expect("a flow");

    assert_eq!(from_yaml, from_json);
    assert_eq!(
        from_json.plan().expect("it plans").layers.len(),
        5,
        "and the document that arrived as JSON plans the same as the one that arrived as YAML"
    );
}

// --- being told no at a boundary ------------------------------------------------------------------

/// Answers the two boundary gates from a list, and writes down everything it was asked, in order.
///
/// A refusal is named by `(moment, path, attempt)`; `None` for the attempt refuses every one of
/// them. `asked` is the whole transcript, which is how the ordering of the gates against `run` and
/// `handoff` is asserted rather than assumed.
#[derive(Default)]
struct Gated {
    refuse: Vec<(Moment, &'static str, Option<u32>)>,
    fails_at: Vec<&'static str>,
    asked: Vec<String>,
}

impl Gated {
    fn gate(&self, moment: Moment, path: &str, attempt: u32) -> Gate {
        let refused = self.refuse.iter().any(|(when, at, which)| {
            *when == moment && *at == path && which.is_none_or(|only| only == attempt)
        });
        if !refused {
            return Gate::Proceed;
        }
        Gate::Refused {
            reason: match moment {
                Moment::Enter => format!("`{path}` may not run now"),
                Moment::Leave => format!("attempt {attempt} of `{path}` is not accepted"),
            },
        }
    }
}

impl StepRunner for Gated {
    fn run(&mut self, path: &str, _step: &Step, _cx: &StepContext) -> StepOutcome {
        self.asked.push(format!("run {path}"));
        if self.fails_at.iter().any(|name| path.ends_with(name)) {
            StepOutcome::Failed
        } else {
            StepOutcome::Passed
        }
    }

    fn handoff(&mut self, scope: &str, gives: &[NodeId]) -> Handoff {
        self.asked.push(format!("handoff {scope}"));
        gives
            .iter()
            .map(|name| (name.clone(), serde_json::json!({"from": scope})))
            .collect()
    }

    fn entering(&mut self, path: &str, attempt: u32) -> Gate {
        self.asked.push(format!("enter {path} {attempt}"));
        self.gate(Moment::Enter, path, attempt)
    }

    fn leaving(&mut self, path: &str, attempt: u32, failed: bool, handoff: &Handoff) -> Gate {
        let carrying: Vec<&str> = handoff.keys().map(String::as_str).collect();
        self.asked.push(format!(
            "leave {path} {attempt} {} [{}]",
            if failed { "failed" } else { "clean" },
            carrying.join(", ")
        ));
        self.gate(Moment::Leave, path, attempt)
    }
}

/// A section with something before it, something inside it and something after it — so a refusal
/// at its boundary can be seen to stop the right things and leave the rest alone.
const GATED: &str = r"
id: gated
root:
  id: root
  nodes:
    - id: prepare
    - id: build
      needs: [prepare]
      repeat: {max: 3}
      gives: [diff]
      nodes:
        - id: implement
        - id: verify
          needs: [implement]
    - id: review
      needs: [build]
";

#[test]
fn a_refused_entering_skips_the_section_as_failed_and_names_every_step_inside_it() {
    let mut runner = Gated {
        refuse: vec![(Moment::Enter, "root.build", None)],
        ..Gated::default()
    };
    let mut sink = VecFlowSink::new();
    let report = flow(GATED).run(&mut runner, &mut sink).expect("valid");

    assert_eq!(
        sink.steps_started(),
        vec!["root.prepare"],
        "nothing inside the refused section ran"
    );
    assert!(
        !runner.asked.iter().any(|line| line.starts_with("handoff")),
        "and it was not asked to hand anything over: {:?}",
        runner.asked
    );

    // The refusal is emitted before its consequence, so a record reads *why* ahead of *what next*.
    let record: Vec<&FlowEvent> = sink
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event,
                FlowEvent::TransitionRefused { .. } | FlowEvent::NodeSkipped { .. }
            )
        })
        .collect();
    let FlowEvent::TransitionRefused {
        path,
        moment,
        attempt,
        reason,
    } = record[0]
    else {
        panic!("the refusal comes first: {record:?}")
    };
    assert_eq!(
        (path.as_str(), *moment, *attempt),
        ("root.build", Moment::Enter, 1)
    );
    assert_eq!(
        reason, "`root.build` may not run now",
        "carried, not rewritten"
    );

    let skipped: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            FlowEvent::NodeSkipped { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        skipped,
        vec!["root.build.implement", "root.build.verify", "root.review"],
        "every step inside it by name, and then what needed it"
    );

    let FlowEvent::GroupLeft {
        failed,
        exhausted,
        gave,
        ..
    } = sink
        .events()
        .iter()
        .find(|event| matches!(event, FlowEvent::GroupLeft { path, .. } if path == "root.build"))
        .expect("left")
    else {
        unreachable!()
    };
    assert!(
        failed,
        "a section nobody allowed to run is failed to its siblings"
    );
    assert!(
        !exhausted,
        "what stopped it was a refusal, not a bound it used up"
    );
    assert!(gave.is_empty());

    assert_eq!(report.ran, 1);
    assert_eq!(report.skipped, 3);
    assert_eq!(
        report.retreats, 0,
        "a section that may not run now is not retried"
    );
    assert!(!report.clean());
}

#[test]
fn a_refused_entering_of_the_root_runs_nothing_at_all() {
    // The root is a group and is gated like one. This is the whole run being told no.
    let mut runner = Gated {
        refuse: vec![(Moment::Enter, "root", None)],
        ..Gated::default()
    };
    let mut sink = VecFlowSink::new();
    let report = flow(GATED).run(&mut runner, &mut sink).expect("valid");

    assert_eq!(
        runner.asked,
        vec!["enter root 1"],
        "asked once, and then nothing"
    );
    assert!(sink.steps_started().is_empty());
    assert_eq!(report.ran, 0);
    assert_eq!(report.skipped, 4, "every step in the document");
    assert!(!report.clean());
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, FlowEvent::GroupEntered { .. })),
        "a section that was refused was not entered"
    );
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, FlowEvent::FlowFinished { clean: false, .. })),
        "the walk still finishes and still files a verdict"
    );
}

#[test]
fn a_refused_leaving_of_a_clean_attempt_re_enters_the_section_until_the_bound_stops_it() {
    // How a caller forces a retreat: not with a new verb, but by declining the result. What
    // happens next is the document's, which is why the bound still ends it.
    let mut runner = Gated {
        refuse: vec![(Moment::Leave, "root.build", None)],
        ..Gated::default()
    };
    let mut sink = VecFlowSink::new();
    let report = flow(GATED).run(&mut runner, &mut sink).expect("valid");

    assert_eq!(
        sink.steps_started(),
        vec![
            "root.prepare",
            "root.build.implement",
            "root.build.verify",
            "root.build.implement",
            "root.build.verify",
            "root.build.implement",
            "root.build.verify",
        ],
        "three attempts of the whole scope, and review never reached"
    );
    assert_eq!(
        report.failed, 0,
        "no step failed - the section was not accepted"
    );
    assert_eq!(report.retreats, 2, "two retreats between three attempts");
    assert_eq!(report.skipped, 1, "review");
    assert!(!report.clean());

    let refusals: Vec<(&str, u32)> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            FlowEvent::TransitionRefused {
                path,
                moment: Moment::Leave,
                attempt,
                ..
            } => Some((path.as_str(), *attempt)),
            _ => None,
        })
        .collect();
    assert_eq!(
        refusals,
        vec![("root.build", 1), ("root.build", 2), ("root.build", 3)],
        "asked, and refused, once per attempt"
    );
    assert!(
        runner
            .asked
            .contains(&"leave root.build 1 clean [diff]".to_owned()),
        "and asked having seen what the section hands over: {:?}",
        runner.asked
    );

    let FlowEvent::GroupLeft {
        failed,
        attempts,
        exhausted,
        ..
    } = sink
        .events()
        .iter()
        .find(|event| matches!(event, FlowEvent::GroupLeft { path, .. } if path == "root.build"))
        .expect("left")
    else {
        unreachable!()
    };
    assert!(failed);
    assert_eq!(*attempts, 3);
    assert!(
        exhausted,
        "at the bound it is exhausted, exactly as a section that kept breaking"
    );
}

#[test]
fn a_refused_leaving_of_an_attempt_that_already_failed_is_recorded_and_changes_nothing() {
    let flow = flow(GATED);

    let mut ungoverned = Gated {
        fails_at: vec!["verify"],
        ..Gated::default()
    };
    let mut ungoverned_sink = VecFlowSink::new();
    let ungoverned_report = flow
        .run(&mut ungoverned, &mut ungoverned_sink)
        .expect("valid");

    let mut refusing = Gated {
        fails_at: vec!["verify"],
        refuse: vec![(Moment::Leave, "root.build", None)],
        ..Gated::default()
    };
    let mut refusing_sink = VecFlowSink::new();
    let refusing_report = flow.run(&mut refusing, &mut refusing_sink).expect("valid");

    assert_eq!(
        refusing_report, ungoverned_report,
        "the section had already failed; there was nothing left for a refusal to change"
    );
    let without_the_refusals: Vec<&FlowEvent> = refusing_sink
        .events()
        .iter()
        .filter(|event| !matches!(event, FlowEvent::TransitionRefused { .. }))
        .collect();
    let ungoverned_events: Vec<&FlowEvent> = ungoverned_sink.events().iter().collect();
    assert_eq!(
        without_the_refusals, ungoverned_events,
        "the two records differ by the refusals themselves and by nothing else"
    );
    assert_eq!(
        refusing_sink
            .events()
            .iter()
            .filter(|event| matches!(event, FlowEvent::TransitionRefused { .. }))
            .count(),
        3,
        "recorded once per attempt of the failing section"
    );
    assert!(
        refusing
            .asked
            .contains(&"leave root.build 1 failed []".to_owned()),
        "and told, each time, that it was answering about an attempt that had already failed: {:?}",
        refusing.asked
    );
}

#[test]
fn the_two_gates_bracket_every_attempt_and_leaving_is_asked_after_the_handoff() {
    // One transcript, because the order is the claim: nothing runs before `entering`, the handoff
    // is collected before `leaving` sees it, and each gate is asked once per attempt.
    let mut runner = Gated {
        refuse: vec![(Moment::Leave, "root.build", Some(1))],
        ..Gated::default()
    };
    let report = flow(GATED)
        .run(&mut runner, &mut VecFlowSink::new())
        .expect("valid");

    assert_eq!(
        runner.asked,
        vec![
            "enter root 1",
            "run root.prepare",
            "enter root.build 1",
            "run root.build.implement",
            "run root.build.verify",
            "handoff root.build",
            "leave root.build 1 clean [diff]",
            "enter root.build 2",
            "run root.build.implement",
            "run root.build.verify",
            "handoff root.build",
            "leave root.build 2 clean [diff]",
            "run root.review",
            "leave root 1 clean []",
        ]
    );
    assert_eq!(report.retreats, 1);
    assert!(
        report.clean(),
        "the second attempt was accepted, and a run that retreated and then succeeded is a \
         successful run"
    );
}

#[test]
fn the_example_in_this_crates_own_header_reads_and_plans() {
    // An example nobody parses is a description of a format that does not exist — the header once
    // showed `- step: receive`, which this notation has never accepted. The text is taken out of
    // the source exactly as a reader of the documentation sees it, so it cannot drift again.
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
    )
    .expect("this crate's own source");
    let example = source
        .lines()
        .map(|line| {
            line.strip_prefix("//! ")
                .or_else(|| line.strip_prefix("//!"))
                .unwrap_or(line)
        })
        .skip_while(|line| *line != "```yaml")
        .skip(1)
        .take_while(|line| *line != "```")
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!example.is_empty(), "the header still shows one");

    let flow = Flow::from_yaml(&example).expect("the header's example is a document");
    let plan = flow.plan().expect("and it plans");
    assert_eq!(
        flow.steps(),
        vec![
            "root.receive",
            "root.shape.specify",
            "root.shape.decompose",
            "root.implement",
        ],
        "a node with `nodes:` is a group and one without is a step, as the header says"
    );
    assert_eq!(
        plan.layers.len(),
        3,
        "`implement` waits for the whole sub-tree, not for a step inside it"
    );
}
