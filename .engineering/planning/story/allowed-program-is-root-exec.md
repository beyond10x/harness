---
format: aep.planning-md/1
id: story:allowed-program-is-root-exec
kind: story
status: implemented
title: The allowed program is the root executable
summary: Help and documentation state that the allow-list gates argv zero while descendants remain in the same confined process tree.
relations:
- derived_from: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Defect

The implementation checks the initial `argv[0]`, and the tool schema says "first item", but operator-facing prose calls the declaration a program allow-list without stating its process-tree boundary. A build driver such as Go starts compiler and linker descendants; readers can wrongly infer that every descendant needs a second declaration, or that the declaration mediates every later exec.

## Acceptance

CLI help, the repository README, and the public confinement/tool reference state one rule: `--allow-program` admits the root executable in the requested argv. Programs it starts remain inside the same sandbox, cgroup limits, no-network namespace, workspace boundary, and whole-tree timeout/cancellation; they are not individually matched against the root allow-list. Code behavior and JSON contracts do not change.
