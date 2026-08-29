# `b10x-harness` command line, version 2026-08-29.1

The exact argv surface this binary accepts. Immutable once released: a change to a flag's name, to
whether it takes a value, to its default, to what it may not appear beside or to what it may not
appear without opens a **new dated version** rather than editing this one (`AGENTS.md`
invariant 13).

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
| `subcommands` | every subcommand, in name order |
| `arguments` | per command — the root included, under its own name — one row per long flag |
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

Neither is sufficient alone: a checker alone pins a document nothing produces, and a Rust test
alone pins a document nothing else can verify was not quietly edited alongside the code
(`AGENTS.md` invariant 14).

## Cutting the next version

1. Copy this directory to today's date.
2. Regenerate `argv.json` from `contract::argv()` and re-pin `manifest.json`.
3. Point `ARGV_CONTRACT_VERSION` at it.
4. Enter what changed in `CHANGELOG.md`, naming any flag whose `takes_value` moved — that is the
   change a consumer cannot survive silently.
