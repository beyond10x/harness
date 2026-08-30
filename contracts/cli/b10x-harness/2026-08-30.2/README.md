# `b10x-harness` command line, version 2026-08-30.2

The exact argv surface this binary accepts. Immutable once released: a change to a flag's name, to
its short spelling, to whether it takes a value, to its default, to what it may not appear beside or
to what it may not appear without opens a **new dated version** rather than editing this one
(`AGENTS.md` invariant 13).

## What changed since `2026-08-30.1`, and what did not

**The binary did not move; this document did.** clap accepts exactly the command lines it accepted
under `2026-08-30.1` — no flag was added, renamed, removed or reshaped, and `subcommands` names the
same commands. Every one of the 27 rows below is this document being corrected to say what the
binary was already doing, so nothing here can break an invocation that already worked. What they
change is what a driver **generating** an invocation from this document will emit.

Five things were wrong.

**23 flags that eat no word named a placeholder for one.** `value_name` is defined below as the
placeholder in the usage line, and clap prints none for a bare flag: `b10x-harness run --help`
renders `--substrate-embedded` and `--delegate` with nothing after them, beside a
`-p, --profile <NAME>` that has one. The document nonetheless recorded
`"value_name": "SUBSTRATE_EMBEDDED"` against `"takes_value": false` — on the one flag this whole
contract exists because of. A driver reading `value_name` as *this flag takes this argument*
emitted `--substrate-embedded SUBSTRATE_EMBEDDED`, which is the exact word clap refused from the
consumer pinned to `0.1.0`. Under this version `takes_value: false` implies `value_name: null`,
for every flag on every command.

**`-p` was a flag a consumer could type and this document did not record.** clap accepts `-p` for
`--profile` on `run`, `chat`, `workflow run` and `profiles explain`, and prints it as
`-p, --profile <NAME>`. Every row was keyed by its long flag alone and no field held the short one,
so a consumer building an invocation from the pin could not know `-p` existed and nothing here would
have caught it being repointed or dropped. Rows now carry `short`, and it is pinned like any other
field: `null` where a flag has only its long spelling, and a change to it is a table row in the cut
that changes it, exactly as a change to `takes_value` is.

**clap's own `--help` and `--version` were in no row either.** The document was read off the command
definition **before clap builds it**, and clap inserts `-h, --help` on every command and
`-V, --version` on the root during that build. `b10x-harness -V` prints `b10x-harness 0.4.0` and
exits `0`; nothing here said so, and `--version` is what a driver reads to know which binary it
drove. The definition is now built before it is read, and 19 rows arrive with it: `--help` on the
root and on each of the seventeen commands, and `--version` on the root. They are arrivals — no
invocation that worked before can break on a flag that was always accepted and is only now written
down — so they take no row in the table below.

**And this document said the command line had no positional arguments.** It said it as a fact about
the binary — *"positional arguments are not recorded because this command line has none: every value
is named"* — and `b10x-harness profiles show` exits `1` with *"the following required arguments were
not provided: `<NAME>`"*. `providers show` is the same. A driver reading `2026-08-30.1` saw two rows
for `profiles show` — clap's `--help`, and nothing `required` — generated
`b10x-harness profiles show`, and was refused by clap before any harness code ran: the
`--substrate-embedded` failure on another axis, and it survived all six earlier cuts because the
document had no field to hold a positional. There is one now, `positionals`, and what it records is
an arrival like the flags above.

**And two fields said something about a flag left out that this binary decides after clap has
parsed.** `--wire` and `--session-dir` record `"default": null` while the binary applies
`openai-responses` and a state directory read from the environment; `--base-url`, `--model` and
`--session-dir` record `"required": false` while a run without them is refused by name. Both fields
are generated from clap's own definition (`AGENTS.md` invariant 14) and clap is not where any of it
is settled, so no row can hold the answer and no row moves for this. What the document does instead
is say so, in three new subsections of *What is not pinned* below: two tables with one row per
command and flag, and the reason `workflow run` is in neither of them. A driver that read `required`
and sent no endpoint got exit `1`; one that read `default` could not say which model API it was
about to speak, or that a transcript of every run was being filed on the operator's machine.

| command | flag | field | in `2026-08-30.1` | in `2026-08-30.2` |
| --- | --- | --- | --- | --- |
| `chat` | `--delegate` | `value_name` | `"DELEGATE"` | `null` |
| `chat` | `--json` | `value_name` | `"JSON"` | `null` |
| `chat` | `--no-project-instructions` | `value_name` | `"NO_PROJECT_INSTRUCTIONS"` | `null` |
| `chat` | `--no-session` | `value_name` | `"NO_SESSION"` | `null` |
| `chat` | `--profile` | `short` | `null` | `"-p"` |
| `chat` | `--quiet` | `value_name` | `"QUIET"` | `null` |
| `chat` | `--substrate-embedded` | `value_name` | `"SUBSTRATE_EMBEDDED"` | `null` |
| `chat` | `--yes` | `value_name` | `"YES"` | `null` |
| `profiles explain` | `--profile` | `short` | `null` | `"-p"` |
| `run` | `--delegate` | `value_name` | `"DELEGATE"` | `null` |
| `run` | `--json` | `value_name` | `"JSON"` | `null` |
| `run` | `--no-project-instructions` | `value_name` | `"NO_PROJECT_INSTRUCTIONS"` | `null` |
| `run` | `--no-session` | `value_name` | `"NO_SESSION"` | `null` |
| `run` | `--profile` | `short` | `null` | `"-p"` |
| `run` | `--quiet` | `value_name` | `"QUIET"` | `null` |
| `run` | `--substrate-embedded` | `value_name` | `"SUBSTRATE_EMBEDDED"` | `null` |
| `run` | `--yes` | `value_name` | `"YES"` | `null` |
| `tools` | `--substrate-embedded` | `value_name` | `"SUBSTRATE_EMBEDDED"` | `null` |
| `workflow plan` | `--json` | `value_name` | `"JSON"` | `null` |
| `workflow run` | `--delegate` | `value_name` | `"DELEGATE"` | `null` |
| `workflow run` | `--json` | `value_name` | `"JSON"` | `null` |
| `workflow run` | `--no-project-instructions` | `value_name` | `"NO_PROJECT_INSTRUCTIONS"` | `null` |
| `workflow run` | `--no-session` | `value_name` | `"NO_SESSION"` | `null` |
| `workflow run` | `--profile` | `short` | `null` | `"-p"` |
| `workflow run` | `--quiet` | `value_name` | `"QUIET"` | `null` |
| `workflow run` | `--substrate-embedded` | `value_name` | `"SUBSTRATE_EMBEDDED"` | `null` |
| `workflow run` | `--yes` | `value_name` | `"YES"` | `null` |

The 23 `value_name` rows are the document dropping a placeholder it should never have carried; the
four `short` rows are the document starting to carry a spelling clap has accepted all along. Neither
is a change to what clap will accept. Anything that generated a command line off `value_name`
without reading `takes_value` beside it was emitting a word this binary refuses, and has to be
re-read against this version.

## What `2026-08-30` got wrong, and what a consumer of `2026-08-29.3` has to do

`2026-08-30` is released, so it cannot be edited (`AGENTS.md` invariant 13) and this is the only
document in the chain that can carry the correction. Read it here even if you were pinned two
versions back — the version in force is the one you look at, so this is where it has to be.

Three things about it are wrong.

**Its date is a day ahead of its cut.** It was committed by `719f6e3` at `2026-08-29 23:08`, so a
directory named `2026-08-30` puts a date on a pinned artefact that is not the day it was made. The
day already held `2026-08-29`, `.1`, `.2` and `.3`; the honest name was `2026-08-29.4`.
`2026-08-30.1` was cut by `c5bb2ed` at `2026-08-30 10:10`, and this directory on `2026-08-30`
after it; both are dated the day they were made.

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
| `arguments[…][].short` | the same flag's one-letter spelling, `-` and all, or `null` where it has none |
| `arguments[…][].takes_value` | **whether the flag eats the next word.** The one that broke a consumer |
| `arguments[…][].value_name` | the placeholder in the usage line, and `null` whenever `takes_value` is `false` |
| `arguments[…][].default` | the value used when the flag is absent, or `null` |
| `arguments[…][].required` | whether omitting it is a parse error |
| `arguments[…][].conflicts_with` | every flag it may not appear beside, **both directions** |
| `arguments[…][].requires` | every flag it may not appear **without** |
| `positionals` | per command, the same command set `arguments` names — the words typed after the verb, **in the order they are typed** |
| `positionals[…][].name` | the placeholder, as the usage line spells it inside `<>` or `[]` |
| `positionals[…][].required` | whether omitting the word is a parse error — `<NAME>` in the usage line, against `[NAME]` |
| `positionals[…][].multiple` | whether more than one word lands in it |

**clap's own arguments are rows like any other**, because they are command lines a consumer types:
every command carries `-h, --help`, the root carries `-V, --version`, and a driver reads
`b10x-harness --version` to record which binary it drove. They are read off the command line clap
has **built** — an unbuilt definition holds neither, which is how six versions of this document
came to omit them.

`short` and `value_name` are the two halves of what a driver may **type**. `-p` is `--profile` on
`run`, `chat`, `workflow run` and `profiles explain` — a command line a consumer can type today, so
it is pinned here and losing it is a change this document has to record rather than one a consumer
finds out by being refused. `value_name` is the placeholder clap prints beside a flag that eats a
word, `--profile <NAME>`, and it prints none beside a bare one: a flag cannot both have
`takes_value: false` and name a placeholder. Generating `--substrate-embedded SUBSTRATE_EMBEDDED`
off a placeholder is the exact command line clap refused from the consumer pinned to `0.1.0`.

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

Positional arguments are in `positionals` and not in `arguments`, because the two are found
differently: a flag by its name, a positional by its place. So that list is **not** sorted — it is
in the order the words are typed, and sorting it would describe a command line nobody can type —
and it is keyed by command exactly as `arguments` is, empty for the sixteen commands that take no
word. Two take one, and both refuse to run without it: `profiles show <NAME>` and
`providers show <NAME>`.

## What is not pinned

The help text, the summaries, the order clap prints things in, and the exit statuses — those last
are stated in `README.md` and are `0` answered, `2` stopped for a named reason, `1` could not run.

**clap's generated `help` subcommand, and every path under it** — `help`, `help run`,
`profiles help explain`, and even `help help` — is not pinned either, and appears in neither
`subcommands`, `arguments` nor `positionals`. Typing `help <command>` prints the same help text
the flag does and does nothing else, and that text is the first thing this section declines to pin;
enumerating the tree would put 33 more paths in the list a driver enumerates as this product's
verbs, `help help` among them. The flags clap generates are a different question and **are**
pinned, in a row each — see *What is pinned* above.

### Flags a run demands that clap does not

`"required": false` is true of clap on every row below — omitting the flag is not a parse error —
and it is not true of the run. **One row per command and flag**, because a sentence naming them all
at once absolves them all at once, and the value in the third column is what happens without it:

| command | flag | without it | when |
| --- | --- | --- | --- |
| `chat` | `--base-url` | `refused by name` | always, unless a `[default]` provider in `$XDG_CONFIG_HOME/b10x/harness.toml` supplies it |
| `chat` | `--model` | `refused by name` | as above |
| `chat` | `--session-dir` | `refused by name` | on a machine with neither `XDG_STATE_HOME` nor `HOME`, unless the run is told to file nothing |
| `run` | `--base-url` | `refused by name` | always, unless a `[default]` provider in `$XDG_CONFIG_HOME/b10x/harness.toml` supplies it |
| `run` | `--model` | `refused by name` | as above |
| `run` | `--session-dir` | `refused by name` | on a machine with neither `XDG_STATE_HOME` nor `HOME`, unless the run is told to file nothing |

The endpoint and the model may come from a provider instead; the session directory may come from
the environment, and where it cannot the run is refused before the first request rather than
inventing a place to write a transcript. A consumer that read `"required": false` as *may be left
out* emitted `b10x-harness run --input …` and got exit `1` with nothing sent — and, in a container
with a cleared environment, got the same after supplying the endpoint and the model.

The requirement is not recorded on the row because it is not a property of the command line: the
same invocation is refused on one machine and runs on another, according to a config file and an
environment the command line does not name. `"required"` is defined above as *whether omitting it
is a parse error*, and that question still has the answer the row gives.

`app-server` takes `--base-url` and `--model` and clap requires them there, so its rows already say
what they mean; this table is not about it.

### The third run command this table leaves out

`workflow run` flattens the same options and records the same six rows, and it is deliberately in
neither table above nor below. It does not behave the way they describe.

A command line built from this document alone — its two `"required": true` rows and nothing else —
does not reach a refusal by name on it. `workflow::dispatch` never resolves the profile, so the
endpoint is still absent when the run reads it, and the process aborts on that with **exit `101`**,
which is not one of the three statuses *What is not pinned* names above. The message is a panic; it
names no flag, so a consumer cannot repair the invocation from it.

That is a defect in the binary, tracked as `story:workflow-run-panics-and-drops-its-profile`, and
this document does not paper over it: a row promising a refusal by name there would be a second
false statement laid on top of the first, and it is a pinned one. When the binary is fixed the rows
belong in both tables, and
`crates/harness-cli/tests/argv_pin_consumer.rs`'s `the_escape_table_names_the_flags_this_binary_demands_and_clap_does_not`
will say so: it measures every command the tables could cover and fails on one that is missing.

### Defaults this binary applies after clap

`"default": null` is true of clap on every row below — clap holds none — and the binary applies one
after the parse. The third column is the value, in a cell of its own, because a row that named the
flag and then said something else about it would answer nothing:

| command | flag | value when it is absent |
| --- | --- | --- |
| `chat` | `--session-dir` | `$XDG_STATE_HOME/b10x-harness/sessions` |
| `chat` | `--wire` | `openai-responses` |
| `run` | `--session-dir` | `$XDG_STATE_HOME/b10x-harness/sessions` |
| `run` | `--wire` | `openai-responses` |
| `sessions` | `--session-dir` | `$XDG_STATE_HOME/b10x-harness/sessions` |
| `workflow run` | `--session-dir` | `$XDG_STATE_HOME/b10x-harness/sessions` |
| `workflow run` | `--wire` | `openai-responses` |

`--wire` is defaulted **last**, so that a provider may set the wire and a typed flag may still beat
it; `openai-responses` is the wire this harness shipped with, so an invocation predating the flag
means what it did before. `--session-dir` falls back to `$HOME/.local/state/b10x-harness/sessions`
where `XDG_STATE_HOME` is unset, and to the refusal in the table above where both are. Neither value
is in clap's definition, and neither can be: a provider or a profile may supply the wire, and the
session directory is read out of the environment at the moment the run starts.

So `null` on those rows says *clap has no default*, not *there is no default* — and the second
reading is the expensive one. A driver that took `--session-dir`'s `null` to mean nothing happens
has a harness writing a transcript of every run into the operator's state directory, indefinitely,
at a path no field of this document names.

`sessions` is in the table for the same reason `run` is: `b10x-harness sessions` with no
`--session-dir` reads the same directory, and a driver listing what a machine has run needs to know
which one.

## Checked from both directions

| half | where |
| --- | --- |
| the manifest digest matches the file | `scripts/check-cli-contract.py` |
| this binary's clap definition produces exactly these bytes | `crates/harness-cli/src/contract.rs`, `the_pinned_argv_contract_is_what_this_binary_defines` |
| what this README says moved is what actually moved | `crates/harness-cli/src/contract.rs`, `the_version_in_force_names_every_field_that_moved_between_pinned_versions` |
| a flag that eats no word names no placeholder | `crates/harness-cli/src/contract.rs`, `a_flag_that_eats_no_word_records_no_placeholder_for_one`, and `scripts/check-cli-contract.py` |
| every short flag clap accepts, on the **built** command line, is a row here or is named in *What is not pinned* | `crates/harness-cli/src/contract.rs`, `a_short_flag_a_consumer_can_type_is_pinned_or_named_as_unpinned` |
| a flag that loses its short spelling is a move a README has to state | `crates/harness-cli/src/contract.rs`, `a_flag_that_loses_its_short_spelling_is_a_move` |
| a positional the binary requires is recorded, and one that becomes required is a move | `crates/harness-cli/src/contract.rs`, `a_positional_the_binary_requires_is_pinned` and `a_positional_that_becomes_required_or_vanishes_is_a_move`, and `scripts/check-cli-contract.py` |
| a bare flag holds no default this document silently drops | `crates/harness-cli/src/contract.rs`, `a_flag_that_eats_no_word_holds_no_default_but_claps_own` |
| the escape tables name exactly the flags this binary demands, measured by supplying what its refusals name until it stops refusing | `crates/harness-cli/tests/argv_pin_consumer.rs`, `the_escape_table_names_the_flags_this_binary_demands_and_clap_does_not` |
| a command line built from this document alone reaches the endpoint | `crates/harness-cli/tests/argv_pin_consumer.rs`, `an_invocation_built_from_the_document_alone_reaches_the_endpoint` |
| every value this binary uses for an absent flag is a row here carrying that value | `crates/harness-cli/tests/argv_pin_consumer.rs`, `every_default_this_binary_applies_after_clap_is_a_row_carrying_its_value` |
| an escape stated as prose rather than as one table row per claim answers nothing | `crates/harness-cli/src/contract.rs`, `rows_missing`, which the move table above is also read with |
| every rule the checker holds fires on a document that breaks it | `scripts/check-cli-contract.py --self-test`, its own gate step |

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
5. Carry the *What `2026-08-30` got wrong* section forward. **Every** cut carries it, not only
   the next one: the check diffs every consecutive pair of pinned versions, and
   `2026-08-29.3` → `2026-08-30` stays a pair for as long as both directories exist, which is
   for good (invariant 13). It is only in this document because the version it corrects is
   immutable, and a correction a reader of the pin in force cannot see is not a correction.
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
