---
format: aep.planning-md/3
id: review-result:adversary-scope-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:a-written-directory-is-judged-by-what-it-holds (unit over ae6e45bc)
relations:
- reviews: story:a-written-directory-is-judged-by-what-it-holds
revision: 1
---
written-directory U4 adversary pass 1: 4 red cases (1 fixed by the case's own correction), 3 warnings, 3
notes. Uncommitted unit over base ae6e45bc (tree harness-scope-u4, branch impl/scope-written-directories).

Added `crates/harness-catalogue/tests/written_directory_adversary.rs` (6 cases) and one substrate case at
`crates/harness-substrate/src/tests.rs:1639`. Red: the default-profile admission case (:135), the
hard-link case (:173), the no-scope link-out case (:206), the `rust_fmt` module case (:242). Green: the
bounds case (:278), the step-layer link case (:306), the confined any-route case.

Findings:

1. `crates/harness-catalogue/src/written.rs:243`, `:179` (warning, introduced, not in class) — under the
   default profile a directory holding a link to an admitted directory, or more than 10,000 entries, is
   refused although no rule denies anything it can hold; breaks acceptance bullet 2.
2. `crates/harness-catalogue/src/written.rs:191` (warning, INFEASIBLE, pre-existing, in class) — the walk
   sees only symbolic links; a refused file hard-linked beneath a written directory is reached. No
   harness tool or git checkout creates a hard link.
3. `crates/harness-catalogue/src/catalogue.rs:567` (warning, pre-existing, not in class) — `rust_fmt`
   on a named file also rewrites every module that file declares, `#[path]` modules included, while
   only the named file is judged.
4. `crates/harness-catalogue/src/written.rs:107` (note, INFEASIBLE, pre-existing, not in class) — with
   no refusing rule the walk is skipped, so a library embedder running toolchains unconfined could
   write outside the workspace through a link beneath a written directory; the CLI runs toolchains
   only confined.
5. `crates/harness-catalogue/src/written.rs:255` (note, introduced, in class) — no case had a link
   under a rule only the step layer declares; now pinned (:306).
6. `CHANGELOG.md:210` (note, introduced) — the entry understates what is refused.

Attacked and could not break: `.`, `./`, `x/.`, `./x`, trailing slash; `..` and absolute paths;
symlinked written directory, link to a link, dangling, looping, climbing and out-of-workspace links,
links to directories; rule shapes `*`, `**`, `**/`, `?`, `[`, `partial-only`, two layers, `allowed`
before and after a deny; exact bounds; unreadable directories; the confined path by any route;
invariants 6 and 13; argv byte-identical.

```findings
[{"file":"crates/harness-catalogue/src/written.rs","line":243,"category":"acceptance","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"Under the default profile a directory holding a link to an admitted directory, or more than 10,000 entries, is refused although no rule denies anything it can hold."},
{"file":"crates/harness-catalogue/src/written.rs","line":191,"category":"boundary","severity":"warning","verdict":"INFEASIBLE","origin":"pre-existing","message":"The walk sees only symbolic links; a refused file hard-linked beneath a written directory is reached; no harness tool or git checkout creates one."},
{"file":"crates/harness-catalogue/src/catalogue.rs","line":567,"category":"contract-drift","severity":"warning","verdict":"CONFIRMED","origin":"pre-existing","message":"rust_fmt on a named file also rewrites the modules it declares, #[path] modules included, while only the named file is judged."},
{"file":"crates/harness-catalogue/src/written.rs","line":107,"category":"boundary","severity":"note","verdict":"INFEASIBLE","origin":"pre-existing","message":"With no refusing rule the walk is skipped, so an unconfined library embedder could write outside the workspace through a link beneath a written directory; the CLI runs toolchains only confined."},
{"file":"crates/harness-catalogue/src/written.rs","line":255,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"No case had a link beneath a written directory under a step-only rule; now pinned."},
{"file":"CHANGELOG.md","line":210,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"The entry understates what is refused."}]
```
