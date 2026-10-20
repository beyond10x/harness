---
format: aep.planning-md/3
id: review-result:adversary-search-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:search-moves-onto-tool-search (unit over 422809bb)
relations:
- reviews: story:search-moves-onto-tool-search
revision: 1
---
search U1 adversary pass 1: NEEDS-CHANGE, 2 red cases, 1 warning, 3 notes, no blocker. Uncommitted unit
over base 422809bb (tree harness-search-u1, branch impl/search-moves-onto-tool-search).

Added `crates/harness-catalogue/tests/search_against_0_15_1.rs`, 6 cases: 4 green, 2 red.

| case | asserts | result |
|---|---|---|
| `a_refusal_the_changelog_does_not_list_keeps_its_0_15_1_wording` | the CHANGELOG's "every other refusal's wording" claim | red |
| `a_path_that_climbs_out_is_refused_as_outside_whether_or_not_anything_is_there` | the CHANGELOG's climbing-path bullet | red |
| `a_search_answers_0_15_1s_reply_byte_for_byte` | the full 0.15.1 reply as a literal (context, a 400-char cut in multibyte text, regex/glob echoed, context cap note) | green |
| `the_bounds_bite_where_0_15_1s_did` | 200/201 matches, max_results 1000, 1 MiB and 1 MiB + 1, context 5 and 6 | green |
| `search_never_answers_from_outside_the_workspace` | links out, `..`, `/`, absolute paths, a sibling, a workspace opened through a link | green |
| `a_listing_check_is_not_turned_into_a_rewrite_by_an_argument_spelled_as_an_option` | `["-w", …]` refused before anything starts | green |

Red: an absolute or link-mediated path naming nothing now answers "resolves outside the workspace and
was refused" where 0.15.1 answered "No such file or directory"; `missing/../..` still answers
not-found (refusal goes by the deepest existing ancestor, `tool-search/src/lib.rs:220`).

Findings:

1. `CHANGELOG.md:20` (warning) — the tools 0.3.0 pin brings `b10x-tool-toolchain`'s refusal of any
   argument beginning with `-` (`tool-toolchain/src/lib.rs:1692`, reached through `toolchain.rs` →
   `tool.plan`), which closes `go_fmt_check {"paths":["-w",…]}` rewriting files under a check tool
   past the write scope; the CHANGELOG says the bump only brings `tool-process`. Measured on the unit;
   the 0.15.1 behaviour inferred from the tools 0.2.0 → 0.3.0 diff and tools' own CHANGELOG
   (`[0.3.0]` Fixed), not run on the base.
2. `CHANGELOG.md:52` (note) — the not-found → outside wording change above is unlisted.
3. `CHANGELOG.md:38` (note) — "refused whether or not anything is there" overstates it.
4. `crates/harness-catalogue/src/local.rs:669` (note) — the reply-equality test compares
   `tool_search` with itself; the literal 0.15.1 pin is the one that would catch drift.

Attacked and could not break: published `tools` output byte-identical to 0.15.1 (8903 bytes, `cmp`);
key order (no `preserve_order`); regex/literal and invalid-regex/glob wording; `Split::search` →
reads; invariants 2 and 6; no leftover references to deleted symbols; `tool-fs`, `tool-code-editor`,
`tool-contract` have no behaviour diff 0.2.0 → 0.3.0; no temp directories left by the runs.

```findings
[{"file":"CHANGELOG.md","line":20,"category":"contract-drift","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"The tools 0.3.0 pin makes b10x-tool-toolchain refuse any argument beginning with '-' (it closes the go_fmt_check '-w' rewrite past the write scope), and the CHANGELOG says the bump only brings tool-process, which nothing calls."},
{"file":"CHANGELOG.md","line":52,"category":"contract-drift","severity":"note","verdict":"NEEDS-CHANGE","origin":"introduced","message":"An absolute or link-mediated path that names nothing outside the workspace now gets 'resolves outside the workspace' instead of 0.15.1's not-found wording, while the CHANGELOG says every other refusal's wording is unchanged."},
{"file":"CHANGELOG.md","line":38,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"'refused whether or not anything is there' is false for missing/../.., which is still not-found because refusal goes by the deepest existing ancestor."},
{"file":"crates/harness-catalogue/src/local.rs","line":669,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"The reply-equality test compares tool_search with itself, so a tools-side reply change stays green; the literal 0.15.1 pin in tests/search_against_0_15_1.rs is what would catch it."}]
```
