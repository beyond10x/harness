use super::*;

fn flow(yaml: &str) -> Flow {
    serde_yaml::from_str(yaml).expect("the fixture is a flow")
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
        self.0
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
            Layer { nodes: vec!["receive".to_owned()] },
            Layer { nodes: vec!["shape".to_owned()] },
            Layer { nodes: vec!["implement".to_owned()] },
        ],
        "the root sees three nodes, one of which happens to hold two"
    );
    let shape = plan.groups.get("shape").expect("the sub-tree was planned");
    assert_eq!(shape.path, "root.shape");
    assert_eq!(shape.depth(), 2, "and inside it, its own two layers");
    assert_eq!(flow.steps(), vec![
        "root.receive",
        "root.shape.specify",
        "root.shape.decompose",
        "root.implement",
    ]);
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
    assert_eq!(sink.steps_started(), vec![
        "root.receive",
        "root.shape.specify",
        "root.shape.decompose",
        "root.implement",
    ]);
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
    assert!(because.contains('a'), "the reason names the blocker: {because}");
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

    assert_eq!(sink.steps_started(), vec!["root.receive", "root.shape.specify"]);
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
    let report = flow.run(&mut FailsAt(vec!["gate"]), &mut sink).expect("valid");
    assert_eq!(report.skipped, 2, "both steps inside the skipped group count");
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
    struct Capture(Vec<serde_json::Value>);
    impl StepRunner for Capture {
        fn run(&mut self, _path: &str, step: &Step, _cx: &StepContext) -> StepOutcome {
            self.0.push(step.run.clone());
            StepOutcome::Passed
        }
    }
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
            &mut HealsAfter { step: "verify", heal_after: 2, seen: 0 },
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
    assert_eq!(report.skipped, 0, "review was not skipped: the group came out clean in the end");
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
    assert_eq!(left.len(), 1, "a repeated group is left once, not once per attempt");
    let FlowEvent::GroupLeft { failed, attempts, exhausted, .. } = left[0] else {
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
    let FlowEvent::GroupLeft { attempts, exhausted, failed, .. } = sink
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
    // `engineering-protocols`' own development workflow, as `protocol workflow flow` emits it. The
    // fixture is committed rather than generated, so this repository stays free of a dependency on
    // that one and a change on either side shows up as a diff rather than as a surprise at runtime.
    //
    // What it pins is the contract between the two notations: a state graph with three back-edges
    // arrives here as a chain with one repeating sub-tree, and this crate plans it without
    // complaint.
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/adp-default.projected.yaml"),
    )
    .expect("the committed projection");
    let flow: Flow = serde_yaml::from_str(&text).expect("a flow");
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
    assert_eq!(
        flow.steps(),
        vec![
            "root.receive",
            "root.specify",
            "root.decompose",
            "root.establish_verifiers",
            "root.implement-to-review.implement",
            "root.implement-to-review.verify",
            "root.implement-to-review.adversarial_verify",
            "root.implement-to-review.review",
        ]
    );
}

#[test]
fn the_projected_workflow_retreats_when_verification_fails() {
    // The behaviour the whole translation exists to preserve: a red suite sends the work back to
    // `implement`, and the states after it run again rather than being skipped.
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/adp-default.projected.yaml"),
    )
    .expect("the committed projection");
    let flow: Flow = serde_yaml::from_str(&text).expect("a flow");

    let mut sink = VecFlowSink::new();
    let report = flow
        .run(
            &mut HealsAfter { step: "verify", heal_after: 2, seen: 0 },
            &mut sink,
        )
        .expect("valid");

    let started = sink.steps_started();
    assert_eq!(
        started.iter().filter(|path| path.ends_with("implement")).count(),
        2,
        "implement ran twice: {started:?}"
    );
    assert!(
        started.last().is_some_and(|path| path.ends_with("review")),
        "and the run reached review in the end: {started:?}"
    );
    assert!(report.clean(), "a run that retreated and then succeeded is a successful run");
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
    flow.run(&mut runner, &mut VecFlowSink::new()).expect("valid");

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
    flow.run(&mut runner, &mut VecFlowSink::new()).expect("valid");

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
fn a_handoff_is_asked_for_once_after_the_last_attempt_and_not_once_per_draft() {
    // A group that retreated three times hands over what it ended up with, not three drafts of it.
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
    let report = flow.run(&mut runner, &mut VecFlowSink::new()).expect("valid");
    assert!(report.clean());
    assert_eq!(runner.seen, 3, "three attempts");
    assert_eq!(runner.asked, 1, "and one handoff");
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
            if self.0.len() < 2 { StepOutcome::Failed } else { StepOutcome::Passed }
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
    flow.run(&mut runner, &mut VecFlowSink::new()).expect("valid");
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
    assert!(error.to_string().contains("no sibling on the other side"), "{error}");
}
