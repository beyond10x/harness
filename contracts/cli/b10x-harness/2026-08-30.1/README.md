# `b10x-harness` command line, version 2026-08-30.1

The exact argv surface this binary accepts. Immutable once released: a change to a flag's name, to
whether it takes a value, to its default, to what it may not appear beside or to what it may not
appear without opens a **new dated version** rather than editing this one (`AGENTS.md`
invariant 13).

## What changed since `2026-08-30`, and what did not

**Strictly additive.** Nothing in `2026-08-30` was renamed, removed, or changed in shape: every
flag keeps its spelling, its `takes_value`, its default, its conflicts and its requirements, and
`subcommands` names exactly the same commands. A consumer pinned to `2026-08-30` is correct against
this binary and needs to change nothing.

What is new is one flag, on the three commands that take a run's options — `run`, `chat` and
`workflow run`:

| added | what it takes |
| --- | --- |
| `--delegate-parallel <N>` | a count, default `4`, at least `1`, **requires `--delegate`** |

It bounds how many of one turn's delegates run at the same time. `1` is the behaviour every
version before this had: one child at a time. A consumer that wants that behaviour and does not
want to depend on this flag existing can go on not passing it only if it also pins `2026-08-30` —
under this version the **default is 4**, which is a change in what a run does rather than in what
the command line accepts, and is recorded in `CHANGELOG.md` rather than here.

## What `2026-08-30` got wrong, and what a consumer of `2026-08-29.3` has to do

`2026-08-30` is released, so it cannot be edited (`AGENTS.md` invariant 13) and this is the only
document in the chain that can carry the correction. Read it here even if you were pinned two
versions back — the version in force is the one you look at, so this is where it has to be.

Three things about it are wrong.

**Its date is a day ahead of its cut.** It was committed by `719f6e3` at `2026-08-29 23:08`, so a
directory named `2026-08-30` puts a date on a pinned artefact that is not the day it was made. The
day already held `2026-08-29`, `.1`, `.2` and `.3`; the honest name was `2026-08-29.4`. This
directory, `2026-08-30.1`, was cut by `c5bb2ed` at `2026-08-30 10:10` and is dated the day it was
made.

**It measured itself against the wrong version.** Its heading reads *What changed since
2026-08-29.1*, but `2026-08-29.2` (which added `workflow`) and `2026-08-29.3` (which added
`--agents-dir`, `--plugin-dir` and `--skills-dir`) stood between them. Its real predecessor is
`2026-08-29.3`.

**It was not strictly additive, and said it was.** Against `2026-08-29.3` — its real predecessor —
`profiles` and `providers` arrived and three flags moved on every command that takes a run's
options. This is the whole table:

| command | flag | field | in `2026-08-29.3` | in `2026-08-30` |
| --- | --- | --- | --- | --- |
| `chat` | `--base-url` | `required` | `true` | `false` |
| `chat` | `--model` | `required` | `true` | `false` |
| `chat` | `--wire` | `default` | `"openai-responses"` | `null` |
| `run` | `--base-url` | `required` | `true` | `false` |
| `run` | `--model` | `required` | `true` | `false` |
| `run` | `--wire` | `default` | `"openai-responses"` | `null` |
| `workflow run` | `--base-url` | `required` | `true` | `false` |
| `workflow run` | `--model` | `required` | `true` | `false` |
| `workflow run` | `--wire` | `default` | `"openai-responses"` | `null` |

None of the nine breaks an invocation that already worked: a flag that stopped being required may
still be passed, and the default that went away is still applied — by `RunOptions::wire()`'s
`unwrap_or_default()` (`crates/harness-cli/src/lib.rs:1030`), with or without a profile, after clap
has run. No profile is needed to get `openai-responses`; what changed is that the *document* no
longer says so.

What they break is a **driver that reads this document to decide what it must send**. Under
`2026-08-29.3` clap refused `run` without `--model` and `--base-url`, so a driver could rely on
being told; under `2026-08-30` and here it does not, and a run with neither a profile nor those
flags gets as far as harness code before it fails. Anything generating a command line, a form or a
validator off `required` — and anything reading `--wire`'s `default` to know which wire it will get
without asking — has to be re-read against this version.

`2026-08-30`'s claim that "a consumer pinned to `.1` is correct against this binary and needs to
change nothing" holds for what clap will *accept*. It does not hold for what this document *says*,
and the second is what a driver is pinning.

## Why the command line is a contract

`--substrate-embedded` changed from taking a value to being bare. The change was right — it had
demanded a value it then ignored, the README showed it bare and no test exercised it — but a
consumer pinned to `0.1.0` went on passing `--substrate-embedded 1`, and clap refused the whole
command line before any harness code ran. Nothing here could have caught it: the provider-wire
contracts pin what goes to a model and the app-server profile pins what a bridge client sees, and
the **command line is a third interface with consumers of its own** — metaharness's `b10x` adapter
launches this binary and reads its `--json` record.

## What is pinned

`argv.json`, generated from clap's own definition (`Cli::command()`), never written by hand:

| field | what it holds |
| --- | --- |
| `product` | the binary's name |
| `subcommands` | every command a caller can type, in name order, **nested verbs space-joined** |
| `arguments` | per command — the root under its own name, a nested verb under its whole path — one row per long flag |
| `arguments[…][].long` | the flag as it is typed, `--` and all |
| `arguments[…][].takes_value` | **whether the flag eats the next word.** The one that broke a consumer |
| `arguments[…][].value_name` | the placeholder in the usage line, or `null` |
| `arguments[…][].default` | the value used when the flag is absent, or `null` |
| `arguments[…][].required` | whether omitting it is a parse error |
| `arguments[…][].conflicts_with` | every flag it may not appear beside, **both directions** |
| `arguments[…][].requires` | every flag it may not appear **without** |

`conflicts_with` is symmetric on purpose. clap stores a conflict on the argument that declared it
and enforces it on both, so a document recording only the declaration would say `--approve-up-to`
conflicts with `--yes` and that `--yes` conflicts with nothing — true of the definition, false of
the behaviour, and the behaviour is what a consumer is pinning.

`requires` is the other half of the same question, and it is refused the same way — before any
harness code runs. `--delegate-turns` without `--delegate`, and `--oauth-token-pointer` without an
oauth source, are both parse errors; a consumer reading only `conflicts_with` would see a flag with
no conflicts and no default, pass it alone, and be refused. A requirement on a **group of
alternatives** records the group's members — `--oauth-token-pointer` requires
`["--oauth-token-env", "--oauth-token-file"]` — and those two also conflict with each other, so the
two fields read together say *one* of them, not both.

Both lists are always present and empty rather than absent when there is nothing to say. Both are
read out of the parser rather than out of the declaration: clap exposes no getter for an argument's
requirements, and the behaviour is what is being pinned either way.

A nested verb is recorded under the words that reach it — `workflow`, `workflow plan`,
`workflow run` — because that is the command line a consumer types. Recording only the top level
would say `workflow` accepts no flags at all: true of the word, false of every verb under it, and
the second is the half that breaks a driver. The word itself is listed too, with the empty flag row
set it really has, so `subcommands` still names everything that exists.

Positional arguments are not recorded because this command line has none: every value is named,
which is what makes an invocation in a driver's source readable three months later.

## What is not pinned

The help text, the summaries, the order clap prints things in, and the exit statuses — those last
are stated in `README.md` and are `0` answered, `2` stopped for a named reason, `1` could not run.

## Checked from both directions

| half | where |
| --- | --- |
| the manifest digest matches the file | `scripts/check-cli-contract.py` |
| this binary's clap definition produces exactly these bytes | `crates/harness-cli/src/contract.rs`, `the_pinned_argv_contract_is_what_this_binary_defines` |
| what this README says moved is what actually moved | `crates/harness-cli/src/contract.rs`, `the_version_in_force_names_every_field_that_moved_between_pinned_versions` |

Neither is sufficient alone: a checker alone pins a document nothing produces, and a Rust test
alone pins a document nothing else can verify was not quietly edited alongside the code
(`AGENTS.md` invariant 14).

## Cutting the next version

1. Copy this directory to **today's** date, or to today's date with the next `.N` when there has
   already been a cut today. Never tomorrow's: a date that is not the day of the cut is the defect
   corrected above, and it is unrecoverable once pushed.
2. Regenerate `argv.json` from `contract::argv()` and re-pin `manifest.json`.
3. Write *What changed since* against the version **immediately** before it, naming that version
   in the heading as a backticked token — the cut before yours in time, which is not the directory
   before yours as a string: `2026-08-29.10` is the eleventh cut of that day and comes after `.9`.
4. State every move as a table row of its own — the command, the flag, the field, the value
   before and the value after, five cells side by side in that order, each in backticks — inside
   a `##` section whose heading names the versions it is about. A move is any field of a
   surviving flag that changed **and** any flag or command that is gone: a rename is a departure
   and an arrival, and only the departure can break a consumer, so a vanished flag is recorded as
   `present` moving from `true` to `false`. Arrivals belong in the *what is new* prose above and
   need no row. `2026-08-30` skipped this step and shipped "strictly additive" over nine moved
   fields.
5. Carry the *What `2026-08-30` got wrong* section forward, until nobody can still be pinned to
   `2026-08-29.3`. It is only in this document because the version it corrects is immutable, and a
   correction a reader of the pin in force cannot see is not a correction.
6. Point `ARGV_CONTRACT_VERSION` at it, and add it to the list in
   `every_released_argv_version_is_still_pinned_beside_the_current_one`.
7. Enter what changed in `CHANGELOG.md`, naming any flag whose `takes_value` moved — that is the
   change a consumer cannot survive silently.

Steps 3 to 5 are checked, not trusted:
`the_version_in_force_names_every_field_that_moved_between_pinned_versions` in
`crates/harness-cli/src/contract.rs` orders the versions by the day and the cut within the day,
diffs every consecutive pair of pinned `argv.json` files over the **union** of their flags, and
fails when a move is stated by no README a consumer of the version in force reads.

It is deliberately hard to satisfy with words. A row read backwards is the opposite claim and does
not count; a sentence denying the move in the same words is not a table row and does not count; one
row wide enough to carry every token answers one move, not nine; and a table under a heading naming
the wrong pair answers nothing, because attributing a diff to versions it is not between is the
whole of what `2026-08-30` did wrong.
