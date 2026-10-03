---
format: aep.planning-md/3
id: story:allowed-program-is-root-exec
kind: story
status: implemented
title: The allowed program is the root executable
summary: Help and documentation state that the allow-list gates argv zero while descendants remain in the same confined process tree.
relations:
- derived_from: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T08:08:12Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T08:08:12Z", actor: "human:timo", revision: 4, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T08:58:18Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

The implementation checks the initial `argv[0]`, and the tool schema says "first item", but operator-facing prose calls the declaration a program allow-list without stating its process-tree boundary. A build driver such as Go starts compiler and linker descendants; readers can wrongly infer that every descendant needs a second declaration, or that the declaration mediates every later exec.

## Acceptance

CLI help, the repository README, and the public confinement/tool reference state one rule: `--allow-program` admits the root executable in the requested argv. Programs it starts remain inside the same sandbox, cgroup limits, no-network namespace, workspace boundary, and whole-tree timeout/cancellation; they are not individually matched against the root allow-list. Code behavior and JSON contracts do not change.
