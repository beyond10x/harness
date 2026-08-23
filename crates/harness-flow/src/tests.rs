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
    fn run(&mut self, _path: &str, _step: &Step) -> StepOutcome {
        self.0
    }
}

/// Fails exactly the steps whose path ends with one of these names.
struct FailsAt(Vec<&'static str>);

impl StepRunner for FailsAt {
    fn run(&mut self, path: &str, _step: &Step) -> StepOutcome {
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
            FlowEvent::GroupLeft { path, failed: true } if path == "root.shape"
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
        fn run(&mut self, _path: &str, step: &Step) -> StepOutcome {
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
