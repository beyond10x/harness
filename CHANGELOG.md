# Changelog

All notable changes to this component are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A turn's `delegate` calls now run side by side** (design 0002 § 2, milestone M4). A run that
  asked for three sub-tasks in one turn paid three whole child runs of latency back to back,
  because the loop resolved delegates strictly one at a time. Nothing about them required it: a
  delegate starts from an empty conversation, so no child can read what another produced and there
  is no ordering between them to preserve.

  **Neighbouring** `delegate` calls of one turn form a group, capped by the new
  `Delegation::max_parallel` (`--delegate-parallel <N>`, default 4, minimum 1). Neighbouring rather
  than gathered from the whole turn, exactly as a batch of pure tool calls is: a call between two
  delegates is a barrier, because the second child may be there to look at what that call did.

  Each child gets its **own** model port and tool port, from two new defaulted trait methods —
  `ModelPort::fork` and `ToolPort::fork`, both answering `None` (*cannot be run beside itself*)
  unless a port says otherwise. `MessagesClient` and `ResponsesClient` fork by sharing: one
  endpoint, one credential source, one connection pool, and — on the Responses wire — one request
  counter, so two siblings cannot mint the same `x-client-request-id`. `Flat` and `Verbs` fork by
  sharing the catalogue itself, so a fork publishes exactly what its parent publishes, entry for
  entry. A fork that published one tool more would be a way to widen a run by delegating.

  Three things are **not** forked, because there is one of each by nature: the approver (one
  person, asked one question at a time), the operator's hooks (*how many copies of my guard are
  running* must not depend on how many sub-tasks a model asked for) and the event sink (the record
  is one ordered stream). A child on a worker thread reaches all three by asking the run's own
  thread, which sits answering for exactly as long as any child is running. Each proxy **fails
  closed**: an approval nobody gave is a denial, and a hook that could not be consulted did not say
  yes. A child that panics comes back as a failed tool result naming what happened, and its
  siblings finish.

  **This is an optimisation and never a difference in what a run can do.** Where a port will not
  fork, where `--delegate-parallel 1` is set, or where the run's remaining token budget will not
  divide between the children, the same delegates run **in order** — the same children, the same
  gate, the same results in the same order. Order is in fact the more accurate accounting: each
  child is carved on what the one before it actually spent, where a group has to divide the
  remainder up front. Tokens are divided because they add up; the wall clock is **not**, because
  four children running at the same moment take one child's worth of it — the same figure a batch
  of tool calls is handed.

  A reader of the record tells the two apart with no new event: two `DelegateStarted` before either
  `DelegateFinished`, and the children's `Delegated` events interleaved, cannot happen in a run
  that delegated in order.

  The `delegate` tool's description now says several calls in one turn run at once — **only** on a
  run that can actually do it, because a model told something false about what its next turn costs
  is worse than one not told at all, and a model that is not told does not ask.

  Evidence is `provider_emulated`: a new `delegate-pair` scenario on **both** emulators, driven
  through the shipped binary, whose record shows two `delegate-started` before either
  `delegate-finished` — a bracketing a run that delegated in order cannot produce. Timing proves
  nothing here; a fast serial run and a slow parallel one look alike.

- **`contracts/cli/b10x-harness/2026-08-30.1`**, cut for `--delegate-parallel`. Strictly additive:
  every flag of `2026-08-30` keeps its spelling, its `takes_value`, its default, its conflicts and
  its requirements, and no subcommand changed. A consumer pinned to `2026-08-30` is correct against
  this binary. `2026-08-30` is released and reachable on `main`, so it was cut beside rather than
  edited (`AGENTS.md` invariant 13), and a second cut on the same date takes the `.1` suffix.

- **A tracked file carrying an absolute home directory now fails the gate.**
  `scripts/check-no-home-paths.py` judges the **index**, read with `git cat-file --batch` over
  `git ls-files -s`, because a commit records the index and not the working copy — content staged
  with a leak and tidied afterwards would otherwise be committed by a green check; the worktree copy
  is judged too, because `git commit -a` stages it. Every file is searched **as bytes**, with a
  second pass over the same bytes with NULs removed, so a path inside a committed `.pyc` or UTF-16
  text is visible: that `.pyc` was the one file of twenty the cross-repository audit found that no
  text grep would have caught.

  A home directory needs **no trailing separator** — `HOME=/home/<name>` publishes the account
  exactly as a subpath does — and the account class admits non-ASCII, so a contributor named
  `müller` is protected like the author; a candidate is then trimmed to the name at its head, so an
  elided path in a doc comment names nobody. One account, `you`, is a documentation placeholder in
  every file type; `user` and `username` are not, because real machines have them. Two
  planning-store paths are exempt with the reason in the script: the journal is append-only and
  committed, and editing it would forge the record. `--self-test` (52 cases) is a gate step of its
  own, because a check that passed everything would look exactly like a green one.

### Changed

- **A `delegate` call that names an agent this run does not have no longer emits a
  `DelegateStarted` for a child that never starts.** The name was resolved *after* the event was
  emitted, so the record carried a delegation that began and never finished — the one shape a
  reader cannot interpret. Everything a child needs is now worked out before anything is emitted
  and before any budget is carved, which is also what lets a group divide its remainder by the
  number of children that will actually run. The refusal the model reads is unchanged; what moved
  is that an unresolvable agent name is now reported instead of an exhausted budget when a call
  would have failed both checks.

### Fixed

- **A subscription token now reaches `claude-opus-5` and `claude-sonnet-5` on the Messages wire.**
  Both answered `429 rate_limit_error` on every request, at any hour, against an account measured
  at 8% of its five-hour window — and the refusal carried **no `anthropic-ratelimit-*` headers at
  all**, so nothing downstream could tell it apart from an exhausted quota. The transport did what
  it is supposed to do with a `RateLimited` that says it is retriable: four attempts, a back-off,
  and a reported rate limit that was not one.

  The cause is a condition the documented API has no field for. Under a token obtained on a
  person's behalf, this route serves a request only when `system` **opens with a fixed block**:

  ```
  "system": [
    {"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude."},
    {"type": "text", "text": "<the run's own instruction>", "cache_control": {...}}
  ]
  ```

  This harness sent one block — its own instruction — which is one of the shapes that is refused.
  Measured on 2026-08-30 against `https://api.anthropic.com/v1`: the match is exact and positional.
  Dropping the trailing full stop, adding a leading space, adding a trailing newline, or merging
  the instruction into the same block each answer `429`; extra blocks *after* it are served.
  `claude-haiku-4-5-20251001` is not gated at all, which is why the failure looked model-shaped and
  not request-shaped. The `anthropic-beta` value makes no difference.

  `harness_messages::SUBSCRIPTION_CLIENT_PREAMBLE` holds the string, and `request_body` now takes
  the credential presentation as an argument — on this route **how the credential is presented
  changes the body**, so the projection has to be told. A key issued to a program sends exactly
  what it sent before: one block, the instruction, breakpoint on it. The breakpoint stays on the
  last block under both, so the cached constant head still covers the instruction rather than only
  the preamble.

  Contract `contracts/provider-wires/anthropic-messages/2026-08-30.1` cuts a version for it and
  pins **two** request fixtures, one per presentation, both built by the one function in
  `crates/harness-messages/tests/contract.rs`. `2026-08-30` stays as released.

  **This is the harness naming itself as another vendor's client to that vendor.** It is recorded
  here rather than buried in a constant, and it is the operator's decision to send it, taken on
  2026-08-30 against their own subscription credential.

- **The CLI contract version in force, `2026-08-30.1`, now says what actually moved, and a test
  holds it to that.** `2026-08-30` claimed "strictly additive" while measuring itself against
  `2026-08-29.1` — two cuts back — and nine fields had moved against its real predecessor
  `2026-08-29.3`: `--model` and `--base-url` stopped being `required` and `--wire` lost its
  `"openai-responses"` default, on `run`, `chat` and `workflow run`. That directory is released and
  immutable (`AGENTS.md` invariant 13), so the correction is carried in `2026-08-30.1/README.md`,
  which a consumer of the current pin reads. `2026-08-30` was also dated the day after the commit
  that cut it (`719f6e3`, 2026-08-29 23:08); `2026-08-30.1` was cut on 2026-08-30. No flag,
  `takes_value` or default moved in this change — `ARGV_CONTRACT_VERSION` is unchanged and the
  command line is untouched.

  `the_version_in_force_names_every_field_that_moved_between_pinned_versions` diffs every
  consecutive pair of pinned `argv.json` files and fails when a moved field is named by no README a
  consumer of the version in force reads. It matches an **ordered** sequence scoped to the section
  naming that pair, so a document cannot satisfy it by reversing the direction of every row, by
  denying the change in so many words, by listing the tokens on one junk line, or by attributing the
  moves to a different pair — all four passed the first version of the guard. It iterates the
  **union** of flag names, so a renamed or removed flag is a departure rather than a silent skip:
  that is the failure the CLI contract exists for, `--substrate-embedded` having changed shape and
  refused a pinned consumer at clap. Version order is compared numerically on the `.N` suffix, so a
  tenth cut in one day does not sort before the ninth.

## [0.3.0] — 2026-08-30

*Three of the entries below — providers and profiles, workspace adoption, and the default model —
were written after this tag was cut, from `719f6e3`, `0c31438` and `f701e2e`. Each of those changes
shipped in this release without entering the changelog, against `AGENTS.md` § *Releases*. The
entries are new; nothing already recorded in this section was altered.*

### Added

- **A `codex` provider, and it renews its own credential.** `b10x-harness run` against a ChatGPT
  subscription is now `[default] provider = "codex"` — endpoint `https://chatgpt.com/backend-api/codex`,
  wire `openai-responses`, model `gpt-5.6-sol`, credential `~/.codex/auth.json` at
  `/tokens/access_token`. Every one of those values was read off the completed two-turn run of
  2026-08-30 (`story:chatgpt-codex-authorized-run`) rather than off a vendor's documentation.
  `openai` stays what it was: that entry bills an API key, this one bills a person's plan, and they
  are two providers because they are two things to be.

  Unlike every provider before it, `codex` carries **renewal facts**: a token endpoint, a client id
  and the pointer to the refresh token beside the access token. When the token a run is about to
  use is within fifteen minutes of expiring, the run presents that refresh token to
  `https://auth.openai.com/oauth/token`, takes the new one, and writes it back into
  `~/.codex/auth.json` before the first request. Staleness is decided by decoding the access
  token's own `exp` — the signature is not verified, because this is not authenticating anybody,
  it is asking the credential when it expects to stop working. A token whose expiry cannot be read
  is left alone rather than guessed at.

  **This is the harness writing to a file another program owns**, which is a larger softening than
  the defaulted credential path that preceded it, and it is bounded the same way — by being
  readable before it happens and stated after. `providers show codex` prints the file, the token
  endpoint, the client id and the refresh pointer before anything is spent. The write itself is
  atomic (written beside the original, parsed back to check it says what it should, then renamed
  over it) and byte-preserving (only the token values move; key order, indentation and keys this
  build has never heard of survive exactly — where that cannot be proven safe the document is
  re-serialised and the record says so). The file's own mode is carried across, so a store is
  never widened by being renewed.

  A credential the operator named themselves — `--oauth-token-file`, or an `oauth-token-file` in a
  `[providers.codex]` table — switches the renewal off: those pointers describe one vendor's
  document, and applying them to a file somebody named by hand would at best refuse and at worst
  rewrite something this build had no business touching. `claude` carries no renewal either, and
  that is deliberate rather than pending: its credential file holds a refresh token, but the
  authorization server and client that would accept it have not been read off anything here, and a
  guessed token endpoint sends a live refresh token to a server nobody verified.

- **`credential-renewed`, a new event, emitted before `started`.** The record of a run that
  rewrote somebody's credential store: the file, the provider whose renewal was used, when the new
  credential runs out, whether the refresh token on disk was retired, and whether the rewrite
  preserved every other byte. It carries **no part of the credential** — not a prefix, not a
  length, not a digest, because a digest of a token is an oracle for it — so the JSONL stream stays
  a thing you can forward to explain a run. Printed on stderr **even under `--quiet`**: quiet is a
  request for less progress noise, not for a side effect on your disk to go unmentioned. A run that
  renewed nothing emits nothing here; unlike the always-written lists on `started`, this is an act,
  and an act that did not happen has no empty form.

- **`harness-http` gained `JsonExchange`**: one plain POST with a JSON body and a JSON answer, for
  the request that is not a turn. It does not retry, deliberately — an authorization server that
  rotates a refresh token has already spent the old one by the time it answers, so a second attempt
  cannot succeed and a first attempt whose answer was lost has left the caller holding a credential
  the server no longer honours. `harness-credential` takes it rather than a second HTTP client: a
  binary that handles credentials should have one transport to audit.

- **A turn can be held to one tool, and the answer nudge holds one.** `TurnRequest::tool_choice`
  is a new neutral value — `auto`, `required`, or a named tool — projected by both wires in their
  own spellings (`{"type": "tool", "name": …}` and `{"type": "any"}` on the Messages route;
  `{"type": "function", "name": …}` and `"required"` on Responses) and **absent for `auto`**,
  because the model choosing is each provider's own default. A turn held to a tool it does not
  publish is refused before it is sent.

  The loop sends it on exactly one turn: the one the answer nudge opens (`--output-schema`). A run
  that ended in prose has already read the tool's description and the nudge's sentence, so the
  second ask is the provider's constraint rather than a third sentence. It is **not** sent from the
  first turn — that would be a run that answers before doing its work — and on the Messages route
  it sits outside the cache breakpoints, so one turn per run is also what keeps it cheap.

  Measured, not assumed: `ROADMAP.md` Phase 7 said the prose rate would decide whether
  provider-native constrained decoding was worth a contract version. The seventh paid native walk
  (2026-08-30, Haiku 4.5, metaharness `native-eval.hUbOP5`) ended in prose on **three of four**
  attempts at one section under the nudge alone. New contract versions:
  `anthropic-messages/2026-08-30` and `openai-responses/2026-08-30`; the versions before them stay
  pinned as released, and neither stream fixture changed.

- **Providers and profiles, so the flags that never vary live in a file.** `run` takes about fifty
  options; a useful invocation was twelve of them and a confined one sixteen, and most never varied
  — the endpoint, wire, model and credential are the same for every run against a provider, and the
  confinement flags are the same for every run of a kind of work. With `~/.config/b10x/harness.toml`
  holding `[default] provider = "claude"`, twelve becomes none:
  `b10x-harness run --workspace . --input "…"`.

  **The line between the two mechanisms is permission.** A *provider* says where to talk — endpoint,
  wire, default model, where the credential is read from — and none of it grants a run anything, so
  the collection ships compiled in, `claude` and `openai`, overridable field by field. A *profile*
  says what may happen — `write`, an approval ceiling, an allow-list of programs, a write scope —
  and **nothing of that shape is compiled in**: there is no permission bundle inside the binary, and
  every rule a run obeys sits in a file that can be read, diffed and versioned. `--profile <NAME>`
  applies one, repeatably, in the order given.

  **`write` is one key, and it is off.** Absent or false, the run gets four read-only tools whatever
  else the table says, and a profile declaring programs without `write` is refused at startup rather
  than left for the model to discover by being refused mid-run. Turning writing on does not turn the
  approval gate off: `write = true` with no ceiling still meets `DenyAll` (invariant 12).
  `write-scope` defaults to denying `.git/**` — running against a real checkout is what this makes
  ordinary, and a model rewriting history there must not depend on a key somebody remembered.

  Precedence is the shape of the data rather than a rule on top: built-in provider, then
  `[providers.x]` **merged field by field**, then `[default]`, then each `-p` in order **replacing
  whole keys**, then a typed flag — which wins because resolution only ever fills what clap left
  empty, so no `ValueSource` bookkeeping can drift from it. A provider merges because changing the
  model must not drop the endpoint; a profile replaces because one profile's allow-list beside
  another's ceiling is a set of rules nobody wrote. A typed `--base-url` opts out of the provider
  entirely, since half-applying a bundle points one vendor's dialect at a server that never heard
  of it.

  **The condition is in the record, not only in the file.** `session.started` gains `profiles` —
  name, source and a digest of each table — and `credential_source`, both always serialised on the
  `withheld` rule. That matters most for the credential: a provider naming
  `~/.claude/.credentials.json` is still refused outright by `resolve_credential`, on the grounds
  that a harness quietly picking up a key is one whose runs cannot be explained afterwards, so the
  purpose is met by the record instead. A run reports `credential_source: "provider:claude"` rather
  than `"named"`, and `providers show` prints the path before a token is spent. Something is
  defaulted; nothing is silent. Read any of it without running it: `providers list|show` and
  `profiles list|show|explain|init`, where `explain` prints the argv a `-p` expands to and which
  profile set each key.

- **`contracts/cli/b10x-harness/2026-08-30`**, cut for `--profile` and the `providers` and
  `profiles` verbs. Strictly additive: `--base-url`, `--model` and `--wire` become optional, which
  is what lets a provider supply them, and every existing invocation means what it did.

### Changed

- **A workflow step runs under the write scope its own node declares.** `protocol workflow flow
  --map` has always written each step's first-match-wins scope into the projected node's `run:`
  block as a `scope:` list of `<glob>=<word>` — the same grammar `--write-scope` takes. Nothing
  read it: the toolset was built once per run in `prepare`, so every step of a walk ran under the
  run's scope and the document's own was decorative. The eighth paid native walk (metaharness
  `native-eval.ew4lFi`, 2026-08-30) proved what that costs: the eval's deliberate-denial step, whose
  node denies `.engineering/**` precisely so a refusal can be observed, wrote `revision: 99` into a
  planning store on disk, and the only thing that caught it was the store's own after-the-fact
  validator. *Prevented* and *detected* are not the same guarantee, and the document said prevented.

  The node's scope is now laid over the run's for the length of the step and taken off again
  however the step ends, so the same map denies the same write on both arms. It is refused **in the
  tool, before the write**, and comes back as a failed `ToolOutcome` the model is told about
  (invariants 9 and 12) — never a run-ending error and never an audit afterwards. The step is also
  told its node's rules beside its prompt, in the same words the standing instruction uses for the
  run's, so no paid turn is spent discovering the document's own rule by being refused.

  **A node can only narrow.** Both layers are asked and the first refusal wins, so a node that says
  `allowed` where `--write-scope` says `denied` changes nothing at all — there is no arrangement of
  rules a generated document can write that gives back what the operator's command line took away.
  A node that declares no `scope` runs under the run's, exactly as before: a document that says
  nothing does not silently narrow a run. The list is applied in the order the map wrote it and
  nothing sorts it, because re-ordering it is what changes its meaning.

  A `scope` this build cannot read — not a list, holding something that is not a string, or naming
  a word that is not `allowed`, `partial-only` or `denied` — refuses the document **by node path**
  when it is read, and `workflow plan` refuses it for free with the same words. It never falls
  through to the run's scope: a document that states a boundary and a walk that quietly ran without
  it is the failure the key exists to close.

  The tool list and `--allow-program` are still the run's for every step; a published toolset per
  group remains open (design 0003 § 6).

- **`--substrate-embedded` adopts a directory the operator named, so a confined run can be pointed
  at a real project.** Adoption refused any workspace whose directory was not named `ws_…`, so
  writing meant pointing a run at a scratch copy of a project rather than the project itself: edits
  landed somewhere that then had to be synchronised back, and the confined half of this harness was
  unusable against a checkout somebody actually works in. Now:

  ```
  cd ~/src/my-project
  b10x-harness run -p write --input "…"
  ```

  **The prefix was never the containment**, which is why this is a change and not a boundary being
  relaxed. `ws_` belongs to substrate's resource-**id** scheme — a workspace's directory name *is*
  its id, so the two cannot disagree. What stops a name escaping is `openat2` beneath the pinned
  root descriptor with symlinks refused, plus the name being a single path component; neither moved.
  The rule lived in three places rather than one — `substrate-host`'s `validate_root_name`,
  `harness-substrate`'s `workspace_adopt` and `harness-cli`'s pre-flight, the last two carrying
  their own copy so a refusal names the flag instead of surfacing a driver's flat `path-escape` from
  inside — and all three now accept one path component of alphanumerics, `_` and `-`, refusing `.`,
  `..`, anything with a separator, and a leading `-`, which would read as a flag where the name
  reaches an argv. The hyphen is new: `engineering-protocols` was not previously a legal name.

  **Capsules move to `$XDG_STATE_HOME/b10x-harness/capsules`, and that is not a tidiness fix.**
  `HostConfig::minimum` put the capsule directory beside the workspaces it serves, which was right
  while the root was a scratch directory this process made. Now that the root is the parent of
  somebody's checkout, a run in `~/src/my-project` would have created `~/src/.substrate-capsules` —
  writing into an operator's own tree, uninvited, to hold something that is this harness's business
  and not the project's.

  The substrate pin moves `tag = "0.2.1"` to `tag = "0.2.2"`, which is the revision that drops the
  requirement from adoption. **Half of adoption, and the other half is named rather than left to be
  found:** substrate still gates `exec.start` on a `ws_` prefix over the **socket** path, so a
  directory adopted through `--substrate <socket>` can be read and written but cannot run an exec.
  The embedded driver does not import that crate, so the path `--substrate-embedded` uses is whole.

- **`claude`'s default model is `opus`, and the default is written as an alias.** The capable model
  rather than the cheapest one, chosen knowing it costs materially more per run than `haiku`; anyone
  wanting the other trade has `[providers.claude] model = "haiku"` — one line — or `--model haiku`,
  which is none. The field holds `opus` rather than `claude-opus-5` because a dated identifier goes
  stale on the vendor's next release and takes every run that never named a model with it, as a
  `404` from the far side that nothing here can explain. `haiku`, `sonnet`, `opus` and `fable`
  resolve through the provider, so the alias table is the single place that answers which one is
  current; a name the table does not know passes through untouched, so a model released after this
  binary is still reachable by its exact identifier. `session.started.model` records what it
  resolved to, because an alias is a convenience at the command line and never in the evidence.

### Fixed

- **A section's session now names every attempt above it, so a re-entered ancestor overwrites
  nothing.** `workflow run` named a session `<flow-run-id>.<path>.<attempt>` with the section's
  **own** attempt and nothing else, which is unique only while nothing above it goes round twice.
  When the root retreats — or a retreat group does — every section under it runs its attempt 1 a
  second time under the same name, and the second file lands on top of the first. The seventh paid
  native walk (2026-08-30, metaharness `native-eval.hUbOP5`) filed `…root.receive.1` twice and
  `…root.specify.1` twice: the `specify` attempt whose validator exited `1` — the one worth reading —
  was gone from disk, and only the event record still said it had happened.

  The id now carries the attempt of **every open scope on the way down**:
  `…root.2.implement-to-review.3.verify.1` is the first attempt of `verify`, inside the third attempt
  of `implement-to-review`, inside the second attempt of the root. It still sorts by flow run, it
  still names the section, and a walk now leaves as many files as it ran sections — `sessions` lists
  every attempt that happened, retreats included. Anything reading these names by hand (a listing, a
  glob) sees a longer id for a nested section; nothing parses them.

- **A section name carrying a `.` is refused by the notation.** `FlowError::DottedName`, at the point
  a group's names are first read. The dot is what a path is joined with — `root.shape.specify` — so a
  name holding one made two different sections read as the same path, and would have made two
  different attempt chains read as one session file. It was never legal in spirit and was never
  checked.

- **A model alias now resolves wherever a model is named, not only where one was typed.**
  `[providers.claude] model = "sonnet"` — the shape `website/docs/guides/profiles.md` documents —
  reached the endpoint as the literal string `sonnet`, and the provider's own default had the same
  hole, which is why writing that default as an alias would have shipped broken had the defect not
  been found with it. There is one expansion point now, and every position goes through it: a typed
  `--model`, `[default] model`, a profile's, a `[providers.x]` override's, and the provider's own
  default. `providers show` and `profiles explain` print the resolved identifier too, so what a
  reader sees is what the request will carry.

## [0.2.0] — 2026-08-30

### Added

- **A workflow's `command` step is one call through the gate, not a model turn** (design 0003
  § 6, M2). A step whose `run` says `kind: command` names a program the document runs — the
  projection's verifiers — and `workflow run` now runs its `command` argv as one `run` call
  through the same stages a model's call meets, in the same order: published or withheld, the
  approver, the operator's `before-call` hook, the tool, `after-call`. No request is sent for it.
  The call and its result are filed into the section's conversation as a model's would be, and
  the record carries `tool-requested`, `tool-completed` and any refusal's `warning` under the same
  names. Exit `0` is a passed step; a non-zero exit, a timeout, a program the run does not
  publish, a person's *no* or a hook's block is a failed step with a `step-command` warning saying
  which. A `command` step whose `command` is missing, empty or not a list of strings is an error
  and not a turn. The exit is read from either shape the `run` tool answers in — the local port's
  `exit: <code>` and the confined port's `exit: <execution record>` with `exit.exit: {code,
  signal}` — because the sixth paid native walk read a validator that exited `0` in the sandbox as
  *no exit code*. In `harness-loop`, `AgentLoop::call` is the new public entry: one call outside
  any turn, through `invoke`'s stages, leaving the events a turn's call leaves.

- **`harness-flow`'s fixture `adp-default.projected.yaml` is refreshed from
  `protocol workflow flow`** (engineering-protocols `870894d`): every state is now a section — a
  group named for the state, holding `<state>-1`… — so a governor asked at group boundaries is
  asked at every state; the retreat `implement-to-review` is a group of those groups. A walk of it
  files eight sessions, one per state, and neither the root nor the retreat holds a step of its
  own any more.

- **`--skills-dir` and `--plugin-dir` — the operator's own instructions, in the on-disk format
  Claude Code already writes.** A skill library written for that harness had to be rewritten to
  reach a run here, so an evaluation comparing the two arms compared who had rewritten their
  instructions rather than comparing the harnesses. Both flags are on `run` and on `tools`, and
  both are repeatable. `--skills-dir <DIR>` reads `<DIR>/<name>/SKILL.md` — YAML frontmatter
  carrying `name` and `description`, the document after. `--plugin-dir <DIR>` reads
  `<DIR>/skills/` the same way and qualifies each skill `<plugin>:<skill>` from the `name` in
  `<DIR>/.claude-plugin/plugin.json`: exactly `--skills-dir <DIR>/skills` plus that qualification,
  named separately so this harness and the vendor's take the same flag with the same argument. The
  qualification is not cosmetic — two plugins may both ship a `planning`, and a run that followed
  whichever was read first would be following instructions nobody chose.

  **Reading a vendor's file format is not becoming a client of a vendor's protocol**, which is the
  distinction `README.md` now draws where it goes on refusing an MCP client, and it is why skills
  could be added while an MCP client stays refused (invariant 1 does not move). A format has no
  reach: nothing opens a socket, nothing gives a third party a say in what a run may do, and the
  bytes are read once, before the first request, out of a directory the operator named.

  **The descriptions reach the model and the bodies do not.** One line per skill in the standing
  instruction — the half of the request a prompt cache holds — and the body arrives only when the
  model calls the new `skill` tool by name. This loop is stateless and replays the whole
  conversation every turn (invariant 4), so a body placed in the instruction is billed on turn one
  and again on turn forty, on every run, including every run that never wanted that skill.
  `--context` is the flag for the files a run genuinely needs throughout; a library of skills is
  the other case. Progressive disclosure is also what Claude Code does with these same documents,
  which is what makes the two arms readable against each other at all: a run handed every body
  eagerly and a run that loaded one on demand are not the same experiment.
  The tool's input schema enumerates the available names as a JSON Schema `enum`, so a name this
  run does not have is refused by the provider before it is sent, instead of costing a turn to find
  out.

  **The parser refuses rather than guesses.** No YAML dependency was taken — the workspace rule
  that kept `hyper` out of `harness-substrate`. What that costs is stated rather than hidden: it
  reads `key: value` at the top level of a frontmatter block and understands nothing else, and a
  document using a key this build does not read refuses the run **by name** rather than being
  half-read. A key that was skipped is a rule its author wrote that the run would not have applied,
  and nobody reading the record afterwards would know which rule was missing.

  `LoopEvent::Started` gains `skills` and `agents`, **both always serialised, empty included** —
  the rule `withheld` was fixed to earlier the same day. Skip-when-empty makes *this run had none*
  and *this build does not say* the same record to a reader outside the process, and "the model was
  never offered the guidance" and "we cannot tell whether it was" are different findings about a
  run. Names only: what a skill says is what the `skill` tool answers with, and a body in a session
  record would be in every reader's face on every run. `b10x-harness tools` answers a `skills` list
  under the same rule, and the terminal renderer puts the names on the opening line, because a run
  given skills and a run given none are different experiments. `agents` rides beside it under the
  same rule, and a plugin carrying only one of the two is accepted and contributes the one.

- **`--agents-dir <DIR>`, and `--plugin-dir`'s `agents/` half: named sub-agents in the vendor's
  format.** `<DIR>/<name>.md`, frontmatter `name`, `description` and an optional `tools` list, the
  body after as that agent's own standing instruction. A delegate call may now name one —
  `delegate(task, agent)` — and the names are a schema `enum` for the reason the `skill` tool's
  are, so a wrong one is refused before it is sent. A run with no agents publishes no `agent` key
  at all, so the option does not exist rather than existing and always failing.

  **A declared toolset can only narrow, never widen, and that is the whole of the security
  claim.** `delegate.rs` has always said delegation widens nothing — the child does exactly what
  the parent's catalogue admits, entry for entry — and an agent's `tools:` is intersected with
  what the *parent was admitted*, not with the port's whole list, so a child of an already
  narrowed run cannot climb back out by naming an agent. What the agent asked for and did not get
  is a `withheld` record in the child's own session, naming the tool: an agent whose author
  granted it something this machine never admitted is a fact about the run, and an absence would
  read as one that never wanted it.

  The narrowing is enforced at **one** chokepoint, and it had to be moved there. Filtering only
  the published toolset left the tool reachable by name — the model has the name from its own
  instructions and does not have to guess — so a hidden tool was still callable. A permission
  boundary that only hides is not one. It now filters what is published *and* refuses the call,
  by the same rule that already refuses a tool the run never published, so the two cannot
  disagree. A test asserts exactly that, and is what found it.

  What is **not** narrowed: an entry reached through a verb. Under the `verbs` surface the call
  names `tool_invoke` and the entry is an argument, so this admits the verb and the entry is
  decided inside the port. Named agents are a flat-surface feature until that is answered, and
  the code says so where a reader will meet it.

  Vendor tool names are mapped in the CLI layer so the loop stays vendor-free — `Read`→`file_read`,
  `Grep`→`search`, `Glob`→`find`, `Bash`→`run`, `Write`→`file_write`, `Edit`→`file_edit`,
  `LS`→`dir_list`. A name outside that table refuses the document, for the reason an unknown
  frontmatter key does: a permission its author granted and this build quietly dropped is one the
  run would not have, with nothing saying so. `tools: []` is refused too — an empty list means the
  parent's whole catalogue, so an author writing *no tools* would otherwise get everything, which
  is the one misreading here that hands out power.

  **Measured:** the driven evaluation's native arm went from 30 pass / 1 fail to **30 pass / 0
  fail**, `EVAL_EXIT=0`, with `the-skill-was-offered` now reporting `skill
  engineering-protocols:planning is among 2 offered at event 0`. Before this the arm was launched
  without `--plugin-dir` at all, and the model was left to discover the subject CLI's own `skill
  load` verb by itself. Contract version **2026-08-29.3** (`contracts/cli/b10x-harness/`), strictly
  additive — flags added, nothing renamed or removed.

- **`b10x-harness workflow` — the loop walks a workflow itself, and the governor stays a program
  outside it.** `crates/harness-flow` could plan and walk a graph and nothing bound it: every
  `StepRunner` lived in its own `tests.rs`, and the only way a workflow reached this loop was an
  external driver spawning the binary once per step. The loop never saw the graph, every step
  started cold, and a retreat paid for its context again. `workflow plan --flow <FILE>` validates a
  document and prints what runs in what order while contacting nothing — no endpoint, no credential,
  like `tools`. `workflow run --flow <FILE> --input <TEXT>` walks it, flattening the same options
  `run` and `chat` take, plus `--max-attempts <N>` for a document that carries no bounds of its own.
  The document format is `harness-flow`'s own `Flow`, deserialised as it is: nothing was added to
  the notation for the verb, and YAML and JSON reading landed there as `Flow::from_yaml` and
  `Flow::from_json` so the command line never parses a document itself.

  **A step is one turn, and it reports through `answer`.** The runner derives that step's output
  schema — `outcome` of `passed` or `failed`, an optional `note`, and `gives` keyed by the names the
  enclosing group promised — so the model never sees a schema file, and `--output-schema` is **not a
  flag of `workflow run`**: there is nothing for a file to shape, so it is not declared rather than
  accepted and overridden. `gives` is what the walk already checks a handoff against, so a group
  that promised `specification_id` and never answered with it fails by the notation's own rule —
  once, without a retreat, because a section that came out clean and still did not produce what its
  document declared buys the same answer again on a second attempt. Only a section that came out
  clean hands anything to its siblings: a result nobody accepted must not be what the rest of the
  walk is built on. `--max-attempts` overrides every `repeat.max` in the document, the root's
  included. A budget stop or a second prose ending is a failed step; a `LoopError` is
  nobody's failed step and aborts the flow, because a walk that filed a network blip as `failed`
  would misreport the plan.

  **One session per `(scope, attempt)`**, id `<flow-run-id>.<path>.<attempt>`, filed with what that
  scope cost as it closes. A group is the context scope: its steps continue one conversation, and a
  step in another group starts from the finished siblings' handoffs and nothing else — a sibling
  cannot depend on a step inside a group, so it must not see that step's transcript either. A
  retreat re-enters the whole section from those handoffs. `--resume` is refused: a flow names its
  own sessions, and resuming a *flow* has no cursor yet.

  **A fourth hook point, `transition`.** `--hooks` learns `on: "transition"`, asked before a section
  is entered and again after it leaves, under the three existing rules — declared and never
  discovered, narrowing only, an argv and never a shell. In the notation this is
  `StepRunner::entering` and `StepRunner::leaving` returning `Gate::Proceed` or `Gate::Refused`,
  defaulted so the walk can be told *no* at a boundary without knowing what a hook is: a refused
  entry skips the section as failed, a refused exit from a clean attempt forces a retreat until the
  document's bound, and a refused exit from a failed one changes nothing. Both emit
  `FlowEvent::TransitionRefused` before the consequence. A hook that cannot answer is read as a
  refusal at both moments — a governor that could not answer did not say yes, exactly as
  `before-call`. The governor itself stays outside: guards and evidence are the protocol engine's,
  this component embeds nothing above it, and a run whose transitions nobody answered is ordered
  rather than governed and its record says so.

  The argv pin is cut as a new version, **2026-08-29.2** (`contracts/cli/b10x-harness/`), strictly
  additive over `.1` — `workflow plan|run`, `--flow`, `--input`, `--max-attempts`, nothing renamed
  or removed; `.1` stays as released (invariant 13). It is regenerated from clap, and it is
  the only contract this touches: `harness-wire` and the app server are unchanged, and `harness-loop`
  gains one word and no behaviour — `HookPoint::Transition`, so a boundary consultation files the
  same `HookRan` event every other point files. There is no port method for it, nothing in the loop
  asks at it, and no new wire item: the loop still does not know it is inside a flow. Every claim
  above is `provider_emulated`; the design is `docs/design/0003-workflow-runner.md`, and what it
  leaves for a later milestone — resuming a flow, a toolset per group, parallel layers, command
  steps — is listed there and in the workflows guide.

- **A program the run may not start is a named refusal in the record, not an error like any other.**
  The `run` tool refusing a program outside the declared set was a failed tool result whose only
  distinguishing mark was its sentence — on the record it read as `tool-completed`, `failed: true`,
  the same shape as a compile error or a missing file. An evaluation asking *did the surface refuse
  what is outside it?* had to grep prose for it, and across the metaharness seam, where every tool
  result's content is `null`, there was no prose left to grep: the row read `0 refusal(s)` for a run
  where the refusal plainly happened.

  Both providers now answer `Refusal::ProgramNotDeclared { program, declared }` beside the sentence
  (`harness_wire::Refusal`, carried on `ToolOutcome::refusal`), and the loop emits
  `warning [program-refused]` immediately before the `tool-completed` it explains — the same order
  `unpublished-tool` uses. The words are unchanged and are now written in exactly one place
  (`Refusal::message`), so what the model reads, what the conversation holds and what the record
  carries cannot drift apart. Nothing else changes: the call still fails, and the model still learns
  the effect did not happen.

  A warning crosses the `metaharness.event/1` converter generically, so this needed no converter
  code. The unnamed failures stay unnamed on purpose — if every failed call carried a name, *the run
  would not do this* would be as unreadable as it was before.

- **`--driver <PATH>` — a program on this host that a confined `run` can actually start.**
  Allow-listing a program by absolute host path admitted its *name*, never its *bytes*: the sandbox
  reaches `/usr`, `/bin`, `/lib`, `/lib64` and the workspace and nothing else, so every exec of a
  path outside those died at `ENOENT`, which a model reads as *the command is wrong* rather than
  *the program is not here*. A driven run whose only sanctioned route was its own CLI could not
  take it and wrote the store's files directly instead.

  Naming a program here hard-links exactly that one file into a private directory — never the
  directory it was built in, which holds every other binary and every dependency — and mounts that
  directory read-only at `/toolchain/driver`, adding the mounted path to the allow-list so one
  declaration is the whole declaration. Read-only on purpose: a run that can rewrite the program
  recording its evidence has no evidence. The stage is named by the digest of what is in it, so a
  rebuilt program is a new stage rather than a silently reused one, and a hard link keeps the exact
  bytes the run was launched against even if a build lands mid-run.

  substrate mounts a declared root and reports it but computes no digest over one, so `tools` now
  answers `driver: {program, sha256}` — without it, "this run pinned the build its evidence is
  recorded against" is a claim nothing supports. Composes with `--toolchain`: substrate admits four
  roots, and a run can want a compiler and the program that drives it.

  Contract version **2026-08-29.1** (`contracts/cli/b10x-harness/`), strictly additive — one new
  flag, nothing renamed or removed. Cut rather than edited because `2026-08-29` is released
  (invariant 13); same-day cuts take a `.N` suffix, which is the first time that has been needed.

- **Public documentation website.** A Docusaurus site under `website/` now gives Harness a
  public-facing quickstart, an explanation of the loop and its safety boundary, operating guides
  for sessions, confinement, structured output, delegation and hooks, command-line and wire
  reference, and an explicit pre-v1 limitations page. Its landing page follows the visual language
  of the other beyond10x project sites; broken links, anchors and TypeScript fail the Pages build.

- **Structured output, sub-agents and hooks** — the three of finding #13's five gaps that are this
  component's to own (`docs/design/0002-sub-agents-structured-output-hooks.md`; the MCP client and
  multimodal input stay out, with the reason in `README.md`). All three are opt-in per run, none
  touches `harness-wire`, and every one of them meets the approval gate exactly as a catalogue entry
  does: nothing reaches a tool without the gate, nothing widens what a turn admits, nothing refuses
  silently.
  - **`--output-schema <FILE>`.** The schema is published as a tool named `answer` that the model
    calls to finish, and its arguments are the answer — wire-neutral, no contract change, and what a
    delegate's structured report will be built on; provider-native constrained decoding behind the
    same value is a labelled later milestone. **Stdout is that JSON and nothing else**, so the
    command composes with `jq`; it is written once, when the run completes, so an answer a `stop`
    hook withdrew never reaches it. Under `--json` stdout is the event record instead, with no
    bare answer line: the answer is the **last** `answered` event before a `finished` whose
    `stop.kind` is `completed` — a `stop` hook can withdraw an earlier one, so a driver taking the
    first takes a refused value. A model that ends in prose is told once to call `answer`; if
    it still does not, the run stops `unstructured` and exits 2 — never a success status over
    prose. An `answer` beside any other call in one turn ends the run and refuses the others as
    *made in the same turn as `answer`, which must be called alone* — which is what the tool's
    description promised, and a sentence that stays true when the answer itself is refused and the
    run goes on. A `stop` hook that sends an answered run back to work restores its nudge, so a
    second prose ending is asked once more instead of exiting `unstructured`.
    The nudge is warned as `answer-nudged`; a `{"accepted": true}` result goes into the
    conversation so the run stays replayable; `LoopEvent::Answered` puts the value in the record;
    the loop validates nothing against the schema. The session stores the answer beside the text.
    `chat` does not take it — a conversation has no single end.
  - **`--delegate`** (`--delegate-turns N`, default 20). A tool named `delegate`: a second
    `AgentLoop` runs to completion inside the tool call over a **fresh** conversation, with the same
    tools, the same approver, the same hooks, the same cancellation token and the **remainder of
    the parent's budget** — a delegate spends the run's budget, never its own, and the parent's
    ceilings bind on the sum — on every exit path, a child that failed on the wire included. The
    parent reads one result, `{stop, turns, text}`, failed when the child did not complete; every event the child emits arrives wrapped in `delegated` so a reader
    cannot mistake its text for the answer, and a terminal renders them indented. Depth one: a
    delegate cannot delegate, and it publishes no `answer` either. `--delegate-turns 0` is a parse
    error (exit 2), refused where the parent's own `--max-turns 0` is refused — before the first
    request — rather than as a failed tool result on every delegation. A port that already publishes
    `answer` or `delegate` refuses the run by name (`LoopError::Config`) **before the first
    request**, rather than being found out by a wire rejecting a duplicate tool on turn one.
  - **`--hooks <FILE>`.** The operator's own programs, run as an argv — never a shell — at three
    moments: `before-call`, after the approver said yes, where exit 2 refuses the call and a hook
    that could not run refuses it too; `after-call`, where a note is appended to the result the
    model reads; `stop`, where exit 2 keeps the run working with the reason as the next user item,
    at most three times — and never at the end of a delegate, whose ending is not the run's. A
    hook can refuse what the gate allowed and can allow nothing the gate refused; `answer` and
    `delegate` are calls like any other to a hook. Named on the command line and **never discovered in the workspace**: a hook found in a
    repository would be a program the repository runs on the operator's machine. A run with hooks
    attached batches nothing, so a hook fires exactly once per call, and every firing is a
    `hook-ran` event naming the point and the decision. The refusal names the entry that would
    have run — *"`run` (called through `tool_invoke`) was blocked by a hook: …"* — the same way an
    approval refusal does. An `after-call` hook's exit 2 or failure becomes a note rather than
    silence — and a failure is recorded as `hook-ran` with `decision: failed`, never `proceed`, so
    the record shows a guard that crashed (`HookPort::after_call` returns `AfterCall { note,
    decision }`). `after-call` does not fire for a call that never ran — an unpublished tool, an
    argument over the bound, a call the approver refused — those are in `tool-completed` and
    `approval-resolved`. A note that pushes a result over the result bound refuses the result by
    name. On
    the command line: exit `0` proceeds, `2` blocks with the reason from `{"reason"}` on stdout or
    else stderr, any other status, a program that cannot start, more than 16 KiB on stdout or 60 s
    of running — pipes included, so a grandchild holding them cannot stall the run — fails by
    name; the child never inherits the variable this run's own credential was named in; a `stop` hook declaring `tools` is refused at load, because nothing
    would ever match it. A hooks file this build cannot read, and a schema that is not an object
    schema, refuse the run before the first request like every other run that never started. The
    argv pin `contracts/cli/b10x-harness/2026-08-29` is re-pinned in place — it is unreleased, and
    invariant 13's immutability starts at release — and now records `requires` per flag beside
    `conflicts_with`, so a consumer can see that `--delegate-turns` needs `--delegate`.

- **The model is handed the tools themselves.** `--surface flat` — the **default** on `run`,
  `chat` and `tools` — publishes every catalogue entry as its own tool with its own input schema,
  so the provider can refuse a misspelled field before the call is billed and no turn is spent
  finding out what exists. Three live runs measured the cost of the alternative: **33–44% of every
  tool call was `tool_search` or `tool_describe`**, and `tool_invoke.arguments` was an untyped
  object nothing could validate. The neutral names the three verbs existed to protect are the
  entry names themselves (`file_read`, `file_write`, …), which `harness_tools::operation_of` maps
  for a reader of a finished run, so nothing downstream loses vocabulary. `--surface verbs` is
  unchanged and fully served: metaharness offers it over MCP, and an arm comparing the two
  surfaces asks for it by name. The standing instruction follows the surface — under `flat` it
  names the entries in one line and leaves the schemas in `tools`, where the provider reads them
  and a prompt cache holds them.

- **Sessions on disk, and `--resume`.** A run that dies on turn 20 no longer takes the first
  nineteen with it. `AgentLoop::run_in(&mut items, &mut spend, input, sink)` runs over a
  conversation and a `RunLedger` the caller owns and writes both back on **every** exit path,
  including the two that are errors — `LoopError` carries neither items nor usage, which is why
  nothing could be saved before. The command line files
  it: `transcript::Session` writes the whole conversation, its usage and its cost to
  `$XDG_STATE_HOME/b10x-harness/sessions/<id>.json`, atomically, in a directory created `0700`,
  outside the repository. Items are stored verbatim, opaque reasoning items included, so a
  following run replays what the model already thought instead of paying for it again. No
  credential is written, and no instruction text: the instruction is derived from this run's
  catalogue and files, and replaying under a stale one would give a run nobody could reproduce
  from its flags. New flags: `--session-dir <path>`, `--resume <id|latest>`, `--no-session` (for
  an evaluation arm that must leave nothing on the machine). A session recorded on the other wire
  is refused **before the first request**, by name, with the flag that fixes it — the loop would
  refuse the opaque items anyway, and saying it here costs nothing; a different workspace is a
  warning, because reading a second checkout is a legitimate thing to do.

- **`b10x-harness sessions`** lists what there is to resume — identifier, UTC timestamp, model,
  turns — newest first.

- **`b10x-harness chat`**, the smallest thing that removes *one question, one answer, exit*. Every
  line of standard input is one more turn on the same conversation, the session is written after
  each of them, and `exit` or the end of the input stops. The same flags as `run` without
  `--input`. No line editing, no history, no completion: a shell has all three, and a harness that
  grew them would own a terminal library forever.

- **A person can approve one write and refuse the next.** `--approve <auto|prompt|deny|all>`,
  default `auto`. `approve::Terminal` asks over `/dev/tty`, so the question arrives even when
  stdin and stdout are pipes, and the prompt names the entry the call resolved to — `file_write`
  with its path and byte count, `file_edit` with the first lines of both sides, `run` with its
  argv — never the verb it travelled through. `y` approves once, `a` stops asking about that entry
  for this process only, `n` and an empty line refuse; nothing answering refuses every further
  call, said once. `auto` asks when there is a terminal and stdin and stderr are one, and
  otherwise prints a single line saying calls above the ceiling will be refused rather than
  leaving it to be discovered from a refusal. `prompt` refuses the run when there is no terminal —
  a run that asked for a person and silently refused everything looks like a harness whose tools
  do not work. `--yes` is unchanged and is the same as `--approve all`. **The library's default
  approver is still `DenyAll`** (invariant 12); what changed is the command line's choice.

- **The model is told where it is.** With no `--instructions-file`, the standing instruction now
  carries an environment block — the absolute workspace path, the OS and architecture, today's UTC
  date, and the git branch, read from `.git/HEAD` and following a `.git` file to a linked
  worktree, **never by spawning `git`** — and the project's own instruction file, `AGENTS.md`
  before `CLAUDE.md` because the neutral one is the maintained one. Anything past 32 KiB is cut at
  a line boundary and the instruction says in words which part of how many bytes was carried.
  `--no-project-instructions` leaves the project's words out as an experiment control; the
  environment block is always there.

- **`find`, a seventh catalogue entry.** Name a glob and get every matching file in one call,
  instead of one `dir_list` per directory level: `*.rs` is that file name at any depth,
  `crates/**/*.rs` is the whole workspace-relative path. The same walk as `search` — build output
  and version control skipped, depth 12, containment re-checked per entry — capped at 500 paths
  with `truncated` when it binds.

- **`search` takes `regex`, `glob` and `context`.** A regular expression that does not compile is
  refused in the regex crate's own words rather than quietly matching nothing; `context` (0–5)
  answers the lines either side of each match under `before`/`after`, each with its own number.

- **Pure tool calls of one turn run side by side.** A turn that asks for six independent reads no
  longer pays six round trips of tool latency: consecutive calls that are published, inside every
  bound, and whose invoked envelope neither mutates nor asks a person are handed to the port as
  one batch (`ToolPort::call_batch`, one thread per call in `Catalogue::invoke_batch`). A write
  between two reads ends the group; a group of one goes down the single-call path unchanged. A
  port that answers a different number of outcomes than it was given calls is **not trusted with
  any of them** — the loop says so by name (`batch-miscounted`) and runs every call itself.

- **A long think is no longer a silent minute.** `response.reasoning_summary_text.delta` on the
  Responses wire and `thinking_delta` on the Messages wire become `StreamEvent::ReasoningDelta` and
  reach a reader on stderr as they arrive. Shown and let go: nothing here is replayed, and what
  carries reasoning across a tool round trip is still the opaque item the turn ends with. The
  Responses summary's `.done` and `part` markers stay silent because each repeats text already
  streamed, and a thinking block's signature is never shown at all.

- **`contracts/provider-wires/anthropic-messages/2026-08-29b`: the Anthropic conversation is
  cached, not just its head.** A run's transcript is resent whole every turn, so with one cache
  breakpoint on the constant head every byte the conversation grew by was paid at full rate on
  every remaining turn — a measured 81-turn run watched its hit rate fall from 66% to 12.5% and
  spent 1.33M input tokens to produce 10.5k of output. A second, **rolling** breakpoint now marks
  the last block of the last message, so each turn writes the prefix it just read and the next
  turn reads it back. Two breakpoints against a documented cap of four, and never on a replayed
  `thinking` block: the provider's signature covers those bytes, so marking one would be a
  rejected turn (invariant 5). **`2026-08-29` stays as released and is superseded by `2026-08-29b`
  wherever this changelog names it.**

- **`contracts/cli/b10x-harness/2026-08-29`: the argv surface is a contract now.**
  `--substrate-embedded` changed from taking a value to being bare — the right change — and a
  consumer pinned to `0.1.0` went on passing a value, which clap refused before any harness code
  ran. The wire contracts pin what goes to a model and the profile contract pins what a bridge
  client sees; the command line is a **third** interface with consumers of its own. `argv.json` is
  generated from clap's own definition (every long flag, whether it takes a value, its value name,
  its default, whether it is required, and every flag it conflicts with, in both directions) and
  checked from both sides: `scripts/check-cli-contract.py` against the manifest digest, and a Rust
  test against clap, failing with a diff that says to cut a new version.

- **A run that never starts leaves a terminal record.** A driver launched this binary with a flag
  that had changed shape; clap wrote its usage and exited **2** before any harness code ran, and
  the driver — which reads the `--json` record and the exit status — saw a status it already had a
  meaning for and an empty stream. Two hours went into working that out. Now every refusal that
  happens before the loop starts — a refused command line, a credential, a workspace, a
  confinement, a session on the wrong wire — writes one line, `{"kind":"refused","reason":…}`, on
  stdout under `--json` and exits **1**; on this command line `2` means *the run stopped for a
  named reason*, which is a run that happened. `b10x-harness events` maps it onto the
  `session.ended` record the stream already has, `subtype: "refused"`, with the reason in
  `stop_reason`.

- **A second wire: `anthropic-messages`, over `POST {base}/messages`.** Streaming SSE, request
  projection, tool-call decode, usage, stop reasons, cancellation and typed status mapping — the
  same loop, unchanged, behind a second projection. `b10x-harness run --wire anthropic-messages`
  selects it, defaulting to the wire this harness shipped with so every existing invocation still
  means what it did. The wire is a branch in exactly one function; below it the loop holds a
  `ModelPort` and cannot tell which projection it got.

  What the projection actually had to do, none of which the first wire needed: group a flat item
  list into **role-alternating messages** with content blocks, put a tool result in the *user*
  message that answers a `tool_use` block in an assistant one, carry tool arguments as a JSON
  **object** rather than as encoded text, send `effort` under `output_config` rather than under
  `reasoning`, and supply `max_tokens` — which this route **requires**, so absence cannot be
  preserved and resolves to a number the endpoint declares.

- **`thinking` and `redacted_thinking` blocks are opaque items.** Assembled from their
  `thinking_delta` and `signature_delta` fragments, kept whole, and replayed byte for byte **and in
  place** — nothing reorders content blocks, which is what keeps a thinking block first in its
  message without this code having to know why that matters. The reasoning text is never emitted to
  a reader: opaque means opaque. Replaying one into the Responses wire, or a `reasoning` item into
  this one, is a typed refusal naming both wires rather than a silent drop (invariant 5); both
  directions are now tested at the client, not only at the type.

- **`contracts/provider-wires/anthropic-messages/2026-08-29`**, checked from both directions like
  every other pin. It adds two halves the first wire has no equivalent of: the
  `content_block_delta` sub-types — on this route the interesting variation is *inside* one outer
  event name, so pinning the outer names alone would pin almost nothing — and the **header names
  each credential kind travels under**, checked against the same function the client calls to build
  them.

- **`harness-credential`, and a `BearerSource` for a subscription token.**
  `SubscriptionToken` reads a token from a file or an environment variable the caller **names**,
  optionally at a caller-named JSON pointer, and re-reads it on **every** call — so a token an owner
  outside this process renews is followed without restarting the run. There is no default path, no
  vendor directory it looks in, and no fallback when the named source is missing: a source that
  searched on failure would be an ambient credential fallback whichever way it was spelled. New
  flags: `--oauth-token-file`, `--oauth-token-env`, `--oauth-token-pointer`, mutually exclusive with
  the API-key flags.

  Its own crate rather than part of a wire, because **nothing about it is vendor-shaped**: the two
  subscription routes this harness cares about hang off two different wires, and putting the source
  in one would make the other depend on it to reuse it. What *is* vendor-shaped is how the fetched
  credential is presented, and that stays in the wire crate.

- **Both wires pass the same loop suite.** `harness-messages`'s provider-emulated suite is
  `harness-responses`'s, case for case, over a second deterministic local endpoint with the same
  scenario names — and `the_two_wires_serve_the_same_scenarios` compares the two emulators' own
  declarations, so a case added to one and not the other fails the gate instead of being noticed a
  release later.
- **`--approve-up-to <risk>`** on `run`: raises the loop's unattended ceiling (`low` by default)
  so a `file_write` (`medium`) or a `run` (`high`) goes through without asking, while everything
  above it still asks and — with no approver attached — is refused. A `file_edit` asks whatever
  the ceiling, because it is non-idempotent, and still needs `--yes`; the two flags do not
  combine, since `--yes` approves everything.

  *Superseded 2026-08-29: idempotency no longer asks. `Envelope::needs_approval` is `risk >
  ceiling` and nothing else, and `file_edit` is `medium` like `file_write` — so
  `--approve-up-to medium` lets both through and neither needs `--yes`. See § Changed below.*
- **A CI gate**, `.github/workflows/gate.yml`: `scripts/gate.sh` on `stable`, and a build on the
  declared `rust-version`. It needs the `B10X_BOT_APP_ID` and `B10X_BOT_PRIVATE_KEY` repository
  secrets to read the private substrate dependency, provisioned by atlas's `bot-ci-secrets.sh`.

### Changed

- **An approver's denial crosses the evaluation converter as a warning, not as a seam decision.**
  `b10x-harness events` mapped `approval-resolved` to `tool.decided`, and every `DecidedBy` that
  event has — `Embedder`, `Frame`, `Deadline`, `Adapter` — names a *metaharness-side* decider. This
  loop's approver is none of them: it is the run's own gate, inside the harness, and describing it
  as a seam decision put the driven arm's treatment on top of the arm that measures the opposite
  claim. A denial is now `warning` / `approval-denied`, naming the call; a granted approval emits
  nothing, because the request and its result already say the call proceeded. The metaharness-side
  b10x seam maps it the same way, so one run described through the two paths does not differ.
  `LoopEvent::ApprovalResolved` is unchanged and still carries no reason, so neither does the
  warning.

- **`file_read` stops counting lines after 16 MiB.** Counting `lines.total` had become a full
  sequential scan of the file on every read, which a deadline cannot reach into; past the bound
  `lines.total` is `null` and `lines_counted_to` says where the scan stopped. `bytes` is still the
  file's own size.
- **A batch runs at most 8 calls at a time** instead of one OS thread per call — a turn asking for
  two hundred reads is two hundred reads, not two hundred threads.
- **`search` compiles a regular expression under a 1 MiB size and DFA limit**, refusing one over it
  in the crate's own words, and echoes `context` when it capped it at 5.
- **Both wires stop calling an error retriable once they have made four attempts**, whatever was
  emitted. A turn that failed three times cold and then broke mid-stream used to buy the loop
  another three rounds of four — sixteen requests and half a minute to learn one thing.
- **The transport half of both wires is one crate now, `crates/harness-http`** — a new name in the
  crate list, which is why an internal move gets an entry here. **No behaviour change.** Bounded
  SSE framing, the retry rule, the back-off, the witnessed sink that makes the retry rule safe, the
  status mapping and the blocking client with its two timeouts moved out of `harness-responses` and
  `harness-messages`, which had held byte-identical copies since the second wire was written; each
  wire is now its projection, its URL and its headers over `harness_http::HttpTransport`, and
  neither depends on `reqwest` at all. `harness-wire` is untouched, and no vendor name, field name
  or header name appears in the new crate.
  **What proves nothing moved:** the two pinned contract suites, `scripts/check-provider-wires.py`
  and both `provider_emulated` suites pass unchanged — not one fixture, manifest or case was
  edited. One real difference between the two copies was found and is now explicit rather than
  implicit: the first route ends its stream with `data: [DONE]` and the second has no sentinel at
  all, so `Framing` is a per-wire setting and `crates/harness-messages/tests/transport.rs` fails if
  the wires ever disagree about anything else. The status tables were already identical — 529 was
  covered by the 5xx range on both sides, and only the comments differed.
- **Bridge mode compacts on the context window too.** `ServerConfig::context_window` carries
  `--context-window` into every bridged thread's `LoopConfig`, the same as `run` and `chat`.

- **Idempotency no longer asks for approval; risk alone does.** `--approve-up-to high` let a `run`
  and a whole-file `file_write` through unasked and refused every `file_edit`, because a second
  clause asked about every non-idempotent mutation whatever the ceiling — a retry question written
  into an approval gate. An unattended run was being pushed toward rewriting files whole when the
  narrower edit was the safer act. `Envelope::needs_approval` is now `risk > ceiling`, and
  `file_edit` and `file_write` are both `Medium`. `Idempotency` is still declared, for a scheduler
  that re-runs a scope to read.

- **Any part of any file is readable, and what comes back is numbered.** `file_read` takes
  `offset` and `limit` in lines and answers numbered lines in `cat -n` shape — the numbers are
  what let a model quote exact text back to `file_edit` — plus `lines: {from, to, total}`, so a
  window is never mistaken for a whole file. A line over 2,000 characters is cut and its number
  listed in `truncated_lines`, never silently. A window that starts past the end of the file is
  refused with the number of lines there are; the confined path refuses by name saying which line
  its byte ceiling reached.

- **A test suite's verdict survives a long `run`.** Output over the 64 KiB cap kept the **first**
  64 KiB and dropped the rest — which is the compiler's progress and never `test result: FAILED`.
  Both ends are now kept with `\n… N bytes omitted here …\n` between them, and the result reports
  `omitted_bytes`.

- **`harness_tools::Operations` is a breaking change for an out-of-tree implementor.** metaharness
  embeds this crate to serve the same catalogue over MCP, so it is named here rather than left to
  be discovered at a build: `file_read(path, ReadWindow)` and `search(pattern, path,
  &SearchOptions)` take the new argument shapes, `find(...)` is a new method with a **defaulted
  refusal** so an implementor that does not answer it refuses by name instead of failing to
  compile, and the trait is now `Operations: Send + Sync` — required by `Catalogue::invoke_batch`,
  which gives each call of a batch a thread.

- **The model may call a catalogue entry by its bare name.** 10 of 82 tool calls on one live run
  were `file_read{path}` rather than `tool_invoke{name:"file_read"}`, each refused as unpublished
  and each a dead turn. Under `--surface verbs` the published list is still the three verbs; a
  bare name is routed to the entry and warned about (`unpublished-tool-routed`) so the waste stays
  measurable. Routing widens nothing: the entry was already reachable through the verb, and it
  meets the same approval gate, the same argument bound and the same result bound. The metaharness
  converter reads a routed call as the act it performed, under either surface.

- **Compaction can see the context window, and `--context-window` now drives it.** Given a window
  in tokens it fires at 80% — measured by the provider's own last reported input count, or by a
  bytes÷4 estimate where nothing reported — and frees down to 50%. Where eliding old tool output
  cannot reach the target, because the weight is in user or assistant text or in opaque reasoning
  items, the harness spends one extra turn asking the model to summarise the earlier part of the
  run and replaces it with a single marked item; the task itself, the newest results and every
  call-with-its-result survive. That turn is charged to the run's tokens, budget and bill like any
  other and does not count against `max_turns`; a summary that fails on the wire is a warning
  (`summary-failed`), not the end of the run. Without a declared window the fixed 192 KiB byte
  rule is unchanged. `b10x-harness run` and `chat` pass `--context-window` into `LoopConfig`, so
  the flag that had only ever bounded the request now also decides when the conversation is
  compacted.

- **A failure that may not repeat now says so.** A stream that stops mid-frame or closes before its
  terminal event is a dropped connection, not a peer speaking a different protocol, and is
  reported as retriable; `408` joins `429` and `5xx`; on the Responses wire a `server_error` or
  `rate_limit_exceeded` **in the stream** is the provider's own state rather than a refusal of this
  request. A malformed event, a bound, and anything refused on this request's own terms stay
  final — retrying those spends a run's budget to be told the same thing four times. The wire
  itself still never resends once a person has read part of an answer; the loop above it, which
  owns the transcript, decides.

- **`b10x-harness tools` states which surface it answered for**, and under `flat` the published
  list and the catalogue name the same entries.

- **`harness_wire::Usage` gained `cache_creation_input_tokens`,** an `Option`. The second route
  bills cache *writes* as their own class; dropping the figure would make a cache-writing turn
  indistinguishable from one that wrote nothing. It is optional because a route that never mentions
  cache writes has **not** said there were none (invariant 7). Nothing prices it separately — the
  rate card has no cache-write field — so it is counted inside `input_tokens` and priced at the
  input rate, which understates such a turn; carrying the figure is what makes that visible.

  `Usage` now also documents the invariant it had only ever implied: **`input_tokens` is the whole
  and the cache figures are parts of it.** The second route reports its three input figures
  *disjointly*, so its projection sums them — left unsummed, every cached turn would have reported
  fewer input tokens than it was charged for and priced itself low.

- **`harness_wire::BearerSource` gained `kind`,** defaulted to `CredentialKind::ApiKey`. One
  endpoint, two routes, the same secret under **different header names** — so which kind a
  credential is stopped being derivable from the wire alone and became a property of the source.
  The kind is neutral; the header names (`x-api-key`, `authorization`, `anthropic-beta`) stay in the
  wire crate, which is where every vendor-shaped byte belongs (invariant 3).

- **The approval gate now fires.** The loop asked its approver only for a tool whose spec said
  `Approval::Required`, and no tool this harness ships says so — so `DenyAll`, which AGENTS.md
  calls the review gate, decided nothing and `--yes` changed nothing. The loop now derives the
  question from what the **call** does: `ToolPort::invoked` answers the spec of the catalogue
  entry a `tool_invoke` names (not the verb's own, which must declare every effect any entry can
  have), and `Envelope::needs_approval` is judged against `LoopConfig::unattended_ceiling`,
  default `Risk::Low`. The same spec is what the approver is handed, what the `ApprovalRequired`
  event names and what the refusal says — `file_write`, never `tool_invoke` — and the refusal
  names the verb too, so the model keeps using it for the reads behind it; `DenyAll` says that a
  retry cannot help, and the standing instruction says not to. Consequence for a person: a
  `b10x-harness run` with a write-capable or exec-capable catalogue and no `--yes` now **refuses
  every write and every `run`** and tells the model so; `--yes` approves them. A `file_edit`
  (non-idempotent) asks whatever the ceiling. Bridge mode is unchanged: the client is the gate
  there.

  *Superseded 2026-08-29: idempotency no longer asks; risk alone does. A `file_edit` is `medium`
  and asks exactly when a `file_write` does. The rest of this entry stands. See § Changed below.*
- **`--substrate-embedded` is a flag, not an option.** It demanded a value it then ignored; the
  README showed it bare and no test exercised it. It is now `bool` on `run` and `tools`, and an
  end-to-end test drives the embedded path.
- **Confinement the operator named and the machine cannot provide refuses the run by name.**
  `--substrate-embedded` over a directory not named `ws_…`, an embedded driver that does not open,
  or `--substrate <socket>` with no usable daemon behind it used to fall back to the read-only
  catalogue **silently** — the operator asked for write+exec, got a read-only run, and the model
  reported the task done. Each case now exits 1 with the reason. The embedded driver is opened
  once per run instead of twice.
- **The socket path works, and it was run.** Verified on 2026-08-29 against a daemon built from
  the pinned substrate revision (`f1cfc1c`) in a delegated user scope: `workspace_create`,
  `file_write`, `file_read`, a confined `/bin/echo` through `run` and a twelve-second
  `/bin/sleep` through `run` (`tests/live.rs`, ignored by default, `B10X_SUBSTRATE_SOCKET`).
  Four things stood between the client and that daemon, none of them the daemon's:
  - **`op` was missing, and then it was the wrong thing.** Every mutating body carried `input`
    alone, which the decoder refuses before it reads the input; `op` is a **caller-minted
    operation id** (`common.json#/$defs/operation-id`, 16–128 of `[A-Za-z0-9_-]`, an idempotency
    key the daemon reserves against the request's hash), not the operation's name — sending
    `"workspace.create"` there was refused for the `.`. The client mints one per mutation from
    time, process and a sequence.
  - **A read needs its query.** `GET …/files/{path}` without `?mode=file&offset=0&limit_bytes=…`
    is refused at `query`; the ceiling asked for is the daemon's own `workspace.read-limit-bytes`.
  - **The exec's output was never fetched.** The start answers the exec resource under `result`
    with its `id`; the client looked for `exec_id`, fell through to answering the start document,
    and the model would have got an exit code and no output. Both streams are now read
    (`…/output?stream=…&offset=0&limit_bytes=…`) and projected into the shape the embedded path
    answers: `stdout`, `stderr`, `stdout_truncated`, `output_complete`, `exit`.
  - **A program longer than ten seconds was reported unreachable.** `wait: true` holds the
    connection open until the exit, and the transport's read timeout was the probe's ten seconds;
    an exec now waits its own `timeout_ms` plus that.
  Before any of that, `Client::exec` posted `{workspace_id, argv}` and nothing else — no
  `sandbox`, no limits — so whether it ran unconfined was the daemon's choice. It now posts a body
  serialised from `substrate-wire`'s own `ExecStartInput` (`require: true`, `network: "none"`,
  the same limits the embedded path uses), built by one shared function so the two paths cannot
  drift, and refuses by name when the daemon states no capability snapshot. The snapshot is asked
  for **once per client** and held, and the CLI probes and serves with one client, so publication
  and admission read one document.
- **The wall-clock deadline is checked between the tool calls of one turn**, not only between
  turns, so a turn of several slow calls stops at the first one past the deadline instead of
  running all of them. And what is left on the clock is handed into each call —
  `ToolPort::call_within` → `Catalogue::invoke_within` → `Operations::run_within` — so a `run`
  is bounded by the smaller of its provider's own ceiling (600 s unconfined, 900 s confined) and
  the time the run has left, and its result says `timeout_ms` so a kill at the deadline is not
  read as the program's slowness. Before this a one-minute budget could not stop a fifteen-minute
  `cargo test`. It also has tests.
- **A scoped run's paths are relative, and a write is judged by where it lands.**
  `Scope::refusal` normalises `./`, `.` and `..` lexically before matching and refuses an
  absolute path when any rule is declared — a denied `target/**` used to be bypassed by
  `./target/x`, `crates/../target/x` or an absolute spelling. A rule's own glob is normalised the
  same way: `./target/**=denied` used to match nothing, silently, and an absolute or climbing
  rule is now refused when it is read. The catalogue also asks the provider where the path
  **lands** (`Operations::lands`, which `LocalOperations` answers by resolving links) and puts that
  spelling through the scope too, so a link inside the workspace (`ok/link -> target/x`) or a
  path that leaves and re-enters it (`../<workspace>/target/y`) no longer steps past a `denied`
  rule. `**/` now matches zero directories too, so `**/*.md` covers `README.md` — which also means
  `docs/**/generated.md=denied` now names `docs/generated.md`.
- `x-client-request-id` is per request; `session-id` and `prompt_cache_key` stay per run. Retry
  back-off sleeps in slices and stops on Ctrl-C. `serde_yaml` (deprecated) → `serde_yaml_ng` in
  `harness-flow`'s tests; `chacha20 0.10.1` (yanked) → `0.10.2`.

- **substrate is pinned by git revision, not reached by path.** `harness-substrate` depended on
  `../../../substrate/crates/*` — a sibling checkout, so the gate was green against whatever tree
  happened to be there and `--locked` could lock none of it. It now names `beyond10x/substrate` at
  revision `f1cfc1c` (`0.2.0` plus the brand sweep; the tag itself still carries the former brand
  in a wire hash domain). Fetching goes through the system `git` (`.cargo/config.toml`,
  `net.git-fetch-with-cli`) because the repository is private. AGENTS.md invariant 2 now says what
  the code does: no dependency on anything that could embed this, one pinned dependency below it.

### Fixed

- **A machine with no config directory is told what to type, instead of panicking.** With neither
  `HOME` nor `XDG_CONFIG_HOME` set there is no `harness.toml` for a profile to live in, and
  `apply_profiles` returned early for that case — past the refusal at the end of it that names the
  missing endpoint and model. `RunOptions::model` unwraps on the promise that this function fills
  the model or refuses the run, so the promise became a panic: exit **101**, a fourth status on a
  command line documenting three, with a backtrace invitation where an instruction belonged. The
  refusal is now `require_endpoint_and_model`, called on both paths out of `apply_profiles`, and it
  tells this caller the truth rather than pointing at a config file that cannot exist on their
  machine. Found by running the binary in that environment, not by reading it.

- **A run that was refused the one tool it needed now says so, instead of looking like a run that
  never asked.** Publication is by absence: a tool this machine cannot confine is not published, so
  the model never plans around it. That is right, and it was also the whole failure — six catalogue
  entries where seven were declared read exactly like six that were declared, with no error, no
  warning and no fact anywhere in the record. A driven session whose only legal route was running a
  program was handed no tool that could start one, hand-wrote the files instead, and the failure was
  read as the model's for weeks. It was the machine's: substrate reports the exec facts only where
  its own cgroup probe passed, and that probe reads the **probing process's** `/proc/self/cgroup`, so
  the same `b10x-harness tools` answers seven entries under `systemd-run --user --scope` and six from
  a login shell, whose `session-M.scope` is a sibling of the manager scope a delegated root lives
  under.

  A **declared** program set the machine will not admit now produces a `Withheld { tool, reason }`
  record naming the predicate that decided as the machine stated it — `exec.argv-only` absent or
  `false`, `exec.cgroup-limits` short of `cpu`/`memory`/`processes`, or no capability facts at all —
  and every reason a stated fact produced carries one line pointing at the caller's own cgroup rather
  than at substrate's configuration. `workspace.guarded-io` absent with a confinement named is
  covered the same way, taking `file_write` and `file_edit` with it. It reaches every place the run's
  shape is stated: one `note:` line on stderr before the first turn (past `--quiet`, like a warning,
  because the run that needed it most was unattended), a `withheld` field on the `started` event
  under `--json`, and a `withheld` array in `b10x-harness tools` — the command the defect was found
  with — beside the same line on its stderr.

  Additive throughout and no gate moved: the tool is still absent from what the model sees, no call
  can name it, and `LoopEvent::Started`'s new field is skipped when empty, so the record of a run
  that was refused nothing is byte-identical to the one it wrote before. **A run that declared no
  programs states nothing**, because absence stays absence and a read-only run is owed no sentence
  about a tool it never wanted.

- **And it now survives the crossing into `metaharness.event/1`, where it was dying one line
  later.** The fact reached the `--json` record and stopped there: `b10x-harness events` writes the
  stream every arm of the evaluation is judged from, and its `session.started` carried
  `offered_tools` and `available_operations` and nothing that could say a tool had been *denied* —
  so the matrix, which is the thing the record exists to feed, saw the same six-of-seven silence
  the terminal had. `session.started` now carries **`withheld`**, the same `{tool, reason}` pairs
  under the same names. **`[]` when the record names none, never `null`:** the loop skips its own
  field when empty, so an absent key is either *nothing was withheld* or *a record older than the
  field* — and this converter has already answered that question one line up, where it stamps
  `harness_version` with its **own** `CARGO_PKG_VERSION` rather than with anything the record said.
  Having claimed the record as this build's it must answer as this build would, and this build
  writes the field whenever the loop reports one. A `null` would report that nobody looked, about a
  converter that did. The argv contract does not move — `--json` is a shape, not a flag — and the
  consumer's half is `metaharness`' own change under its `[Unreleased]`.

- **A run that failed now files what it spent, not only what it said.** A wire failure on turn
  twenty handed the shell its nineteen turns of conversation and none of their figures: the usage
  and cost of every turn that did happen scrolled past on stderr and then died with the process,
  and the session file — the only record left afterwards — showed the whole conversation at zero
  turns and no cost, so `b10x-harness sessions` listed a run that had been billed for nineteen
  turns as `0 turn(s)`. `AgentLoop::run_in` now takes a `RunLedger` beside the items — `usage`,
  `cost_micro_usd`, `turns` — and writes it on **every** exit path exactly as it writes the
  conversation back; `run` keeps its signature and `LoopError` keeps its three payload-free
  variants, so only a caller that lends a conversation pays for the extra argument.
  `transcript::Session::spent` folds it into the session in the failed arm of both `run` and
  `chat`. A run nobody could price still adds no cost rather than a zero. This is the rule
  `RunState::absorb_child` already applies to a delegate — a child that broke on turn four still
  bought three turns — reaching the top-level run, where the shell rather than the loop is the
  thing holding the record. The join is proved end to end by a new scenario both emulators serve,
  `fails-after-turn` — one whole turn with usage and a tool call, then a request answered `400`,
  which the retry rule treats as final so nothing waits out a back-off — against which
  `crates/harness-cli/tests/end_to_end.rs` asserts that a `run` on either wire and a `chat` line
  exit 1, leave a record that carries the bought turn and never a `finished`, and file a session
  holding two turns, the answered turn's usage and its cost, while a run nobody could price
  (`unauthorized` under a rate card) leaves that session unpriced rather than at zero.

- **A compaction can no longer fold a tool call away from its result**, or a reasoning item away
  from the call that follows it: the summary's fold boundary now falls only between whole turn
  groups. Both shapes were provider 400s on the turn after the compaction.
- **The summary turn's own request is one plain-text user item** with no tools, no tool blocks and
  no opaque items, instead of a replay of the folded conversation. On the `anthropic-messages` wire
  that replay was rejected twice over — an assistant-first message, and tool blocks with no `tools`
  — so every compaction there paid for a doomed turn.
- **`max_input_tokens` and `max_cost` are checked immediately after a compaction.** A summary
  turn's spend was absorbed but never tested against the ceilings, so a run overshot by a summary
  turn plus a full conversation turn.
- **A confined `file_read` no longer answers the read route's byte-ceiling prefix as though it
  were the whole file**: past the ceiling `lines.total` and `bytes` are `null`, `truncated` is
  `true`, and `route_ceiling_bytes` and a `note` say the lines past it are unreachable on that
  path. A `file_edit` of such a file is refused rather than writing the prefix back and deleting
  everything after it.
- **A tool call whose thread panics inside `Catalogue::invoke_batch` is a refusal naming the
  entry**; it no longer takes every sibling's answer with it.
- **`find` and `search` answer `depth_bound_reached`**, and both entries' descriptions name the
  directories they skip and the depth they stop at, so a bound is never read as an empty tree.
- **A CRLF file reads identically through the local and the confined provider**; a trailing `\r`
  quoted back to `file_edit` used to match nothing.
- **`find` refuses an empty `glob` by name** instead of answering an empty list.
- **The `batch-miscounted` warning says the port had already run the calls**, so each one in the
  group happens a second time — pure reads, and still stated.

- **A network blip twenty turns into a long run no longer throws the run away.** A turn whose
  stream broke after it had started speaking is attempted again — up to three times, pausing 0.5 s,
  1 s then 2 s, honouring Ctrl-C and the wall-clock budget inside the pause. Whatever streamed for
  that turn is announced as discardable (`turn-retried`) so a renderer can tell a person to
  disregard it, and the renderer prints exactly that. The wire still refuses to retry once it has
  emitted, because only the loop knows the conversation is unchanged by a failed turn; a failure
  the wire calls final is still final, and an error it already tried four times before the first
  byte goes up as final too, so a gateway that is down costs four requests and not sixteen.

- **A provider `error` event stopped losing the provider's own words.** Its `code` and `message`
  sit at the top level, not under `error`, and were read from the wrong place — every one of them
  arrived as `unknown` with no message, which also meant no retriable classification could ever
  fire.

- **`file_write` could escape the workspace through a dangling symlink.** `LocalOperations`
  tested presence with `exists()`, which follows links, so a link inside the workspace whose
  target did not exist yet looked absent; the write then followed the link and created the file
  outside. Reproduced, and reachable through `LocalOperations::unconfined`, which metaharness's
  MCP server uses. Presence is now `symlink_metadata`, a link that leads nowhere is refused, and a
  target that is itself a link is refused. Unconfined `run` no longer inherits this process's
  environment — only `PATH`, `HOME`, `LANG`, `LC_ALL`, `TERM`, `TMPDIR` and the toolchain paths
  (`CARGO_HOME`, `RUSTUP_HOME`, `RUSTUP_TOOLCHAIN`, `CARGO_TARGET_DIR`, `SSL_CERT_FILE`,
  `SSL_CERT_DIR`) reach the child, so a credential held for the harness cannot reach a program
  the model chose the arguments for; `LocalOperations::inheriting` names more, by name. A value
  that is not UTF-8 is passed as it is rather than dropped.
- **`ConfinedOperations::run` refuses an empty argv by name** rather than panicking on `argv[0]`;
  it is a public trait method and an embedder can reach it without the catalogue's check.
- **The loop's deadline tests use a 200 ms budget and 300 ms calls**, not 40 and 60, so one
  scheduling stall on a shared CI runner cannot fail them.
- `file_read` reads at most `max_bytes` from disk rather than the whole file; a truncation lands
  on a character boundary; `search` says `line_truncated: true` when it cut a matched line;
  `dir_list` reports a symlink as `symlink`. A non-string `argv` item is refused rather than
  dropped (`["cargo", 5, "test"]` no longer runs `cargo test`). Two workspaces opened with one
  lease in one process no longer share an id. The contract checkers report a corrupted fixture by
  name instead of a traceback — including a trace entry whose `frame`, or a stream event, is valid
  JSON but not an object.
- **Documentation that had drifted from the tree.** `README.md` named a `scripts/check-brand.sh`
  that moved to atlas; `STATUS.md` was dated 2026-08-21, counted 189 tests (324 pass, 1 ignored),
  omitted the `2026-08-22` wire pin and named the profile directory wrongly; design 0001 said
  nothing in it was implemented after most of it shipped in 0.1.0. AGENTS.md now records that
  bridge mode's approver is the client's, not `DenyAll`, and why.
- **`--context` is pinned on both sides** (`crates/harness-cli/tests/context.rs`). A declared
  context or hooks file that is absent refuses the run — exit `1`, `{"kind": "refused"}` under
  `--json` — with no request sent and no session filed, and a context file that is present reaches
  the standing instruction labelled by its own path. Both refusals were documented and untested, so
  nothing would have caught either decaying into a warning.

### Known gaps

- **The transport half of the two wires is duplicated, and that is this change's real finding.**
  Bounded SSE framing, the retry rule, the witnessed sink that makes the retry rule safe, the
  back-off and the status mapping were copied from `harness-responses` unchanged, because none of it
  is vendor-shaped — it is *transport*-shaped, and the first wire could not tell the difference
  while it was the only one. A `harness-http` beneath both is what that argues for. It was
  deliberately not done here so that this change is the evidence rather than a guess acting on
  itself.
- **Nothing renews a subscription token.** The Anthropic route has now been contacted — a
  three-turn tool-using run against `https://api.anthropic.com/v1` on 2026-08-29, with a
  deliberately invalid token to the same endpoint answering `401` so the 200 is the credential's
  and not the endpoint's indifference (`STATUS.md` § *Subscription auth*). The ChatGPT/Codex route
  still has not been. Nothing here holds a refresh token or calls an authorization server, so a
  token nobody renews expires and the run fails by name.
- **Sub-agents, hooks, an MCP client, multimodal input and structured output are still not owned
  here** (`README.md` § *Not owned here*). Named because a comparison against other harnesses
  ranked them as the remaining gap; each is a decision about what this component owns rather than
  a defect in it, and the decision is pending.
- **The `verbs` surface's discovery cost is measured; the flat surface's is not.** 33–44% of tool
  calls went on discovery behind three verbs, across three live runs. What publishing flat costs
  or saves on a real provider — schema validation refusals, prompt-cache behaviour with seven tool
  definitions instead of three — is an experiment nobody has run yet, and both surfaces stay
  reachable from a flag so that it can be.

## [0.1.0] — 2026-08-24

First tagged release. The entries below cover everything since the component was established;
the commit history carries the full reasoning per change.

### Fixed

- **Compaction reaches its target instead of firing every turn.** The floor on what a compaction
  may elide was a count — the newest six tool results were never touched — and six results can
  outweigh the whole target, so compaction fired on consecutive turns and each rewrite voided the
  prompt cache for a full-rate replay. The floor is now bytes (`KEPT_RESULT_BYTES`, 48 kB) and
  compaction elides to a low-water mark (`COMPACTED_TARGET_BYTES`, 96 kB) instead of stopping the
  moment it fits. Measured on a live run: one compaction instead of four, cost −17%, cache hit rate
  78% → 86%.
- **A confined read is bounded, and says when it was.** The substrate-backed `file_read` ignored
  `max_bytes` and always answered `truncated: false`; the note claiming the truncation could not be
  reported was wrong. It now bounds at 64 kB — the same figure the unconfined provider uses — and
  reports the real size and `truncated: true`.
- **A turn the far side never answered is retried**, instead of ending the run before any text
  arrived.
- **The `2026-08-22` provider-wire manifest is re-pinned to its own fixture.** The workspace tool
  rename changed `turn-stream.sse` without moving the manifest digest, so the contract check
  refused bytes the Rust contract test already required.
- **A tool name this wire cannot publish is refused before the request, and the workspace toolset is
  renamed.** The first live run this component has ever had — `https://chatgpt.com/backend-api/codex`
  under a ChatGPT subscription credential, 2026-08-23 — answered turn 1 with

  ```text
  400 Invalid 'tools[0].name': string does not match pattern.
      Expected a string that matches the pattern '^[a-zA-Z0-9_-]+$'.
  ```

  The published toolset was `workspace.list` / `workspace.read` / `workspace.grep`, and had been
  since the crate was written. Nothing caught it because the only endpoint that had ever seen a
  request was the emulated one, and an emulator written from the same source as the projection
  cannot disagree with it about what a provider will take. This is the class of defect
  `STATUS.md` predicted with *"all evidence is `provider_emulated`; it proves nothing about how a
  real provider behaves"* — the prediction was right on the first attempt.

  The tools are now `workspace_list`, `workspace_read` and `workspace_grep`, and
  `harness-responses` gained `check_tool_names`, called beside `validate` and `check_opaque_items`
  in `turn`. It refuses a toolset this wire cannot carry **locally**, naming the offending tool, the
  pattern, and the name that would work.

  **The rule is in the wire, not in `harness-wire`.** `ToolName` still admits any printable ASCII
  identifier, and a test pins that it admits a dot. The pattern is one provider's, verified against
  one provider; putting it in the neutral crate would shape the neutral layer to a single vendor and
  forbid a name the Messages wire may well accept. A dedicated test in `harness-wire` exists to stop
  a later reader tidying it back in.

### Added

- **A workflow notation the loop runs natively** (`harness-flow`): a DAG of sub-trees, a group as
  a context scope with what crosses it written down, a retreat as a group that repeats — because a
  DAG has no back-edge — and plan/walk over a real projected workflow, with the verdict split from
  the tallies. A workflow renders as committed prose instructions.
- **Confined tools, published only where the machine can confine them.** `file_write`, `file_edit`
  and `run` exist behind substrate's own contract: what this machine can confine is read from
  substrate's facts, an embedded driver rides behind the same trait as the socket, and publication
  follows — three tools with no backend, five with an embedded driver, six inside a delegated
  cgroup. `--substrate-embedded` and `--cgroup-root` on `run` and `tools`; one tree, so the
  workspace a run reads is the workspace it writes.
- **A declared toolchain** (`--toolchain rust`), so a confined run can build and not only
  interpret: exec limits sized for a build, the exec identity substrate admits, and a pin that the
  declared toolchain carries no operator credential into the child.
- **Three verbs over one catalogue**: `tool_search`, `tool_describe` and `tool_invoke` over
  entries named by neutral operations; a call names which file it touched, and the run's own
  record — the event stream every arm is judged from — reports what it cost.
- **A run declares where it may write, and the toolset holds it**: `--write-scope
  <glob>=<allowed|partial-only|denied>` (ordered, first match wins, unnamed paths unrestricted),
  `--context <file>` preloaded into the standing instruction (an absent file refuses the run), and
  `--scope-announce stated|silent` — `silent` is the experiment control that shows the toolset,
  not the prose, is what holds the rule.
- **Prompt caching on the Responses wire**: send `prompt_cache_key`, key it on the conversation,
  say who is calling, and carry the standing instruction at the head of `input` where the cache
  can see it; the catalogue is stated once in the instructions instead of asked for, call by call.
- **A conversation bound instead of a length cliff**: the loop elides old tool-result payloads
  when the replayed conversation passes its bound, and the warning carries the figures.
- Carry sampling on a turn. `TurnRequest` gains an optional `Sampling` — temperature, top_p and a
  reasoning effort — which `LoopConfig` sets once and the loop sends on every turn, because a
  stateless loop replays the whole conversation and a value carried only on the first request would
  apply only to the first request. `b10x-harness run` exposes `--temperature`, `--top-p` and
  `--reasoning-effort`.

  A field nobody set is **absent**, not defaulted. Writing a provider's own default here would take
  a decision that provider is entitled to make and change, make it ours, and make it invisible: a
  request carrying `temperature: 1.0` looks identical to one somebody chose. Values outside their
  range are refused before the request is sent, because the round trip otherwise costs a turn and
  returns a vendor error string nobody can act on.

  Because the request field set changed and a pinned wire version is immutable, this opens
  `contracts/provider-wires/openai-responses/2026-08-22/`. The response side did not move, so the
  stream fixture is byte-identical to the previous version's. `effort` is nested under `reasoning`;
  a flat `reasoning_effort` is accepted by the transport and ignored by the provider, and a test
  pins the nesting for that reason.

  This contract says the fields are **sent**, not that any endpoint acts on them. The self-hosted
  gateway fixes thinking and effort when it launches a pod, so a per-request effort reaches it and
  changes nothing. Which endpoint honours what is `runtime/agent`'s route registry's question.

- Establish `runtime/harness`, B10x's own agent loop, as a component separate from the Codex
  and Claude bridges in `runtime/agent`. It carries no bridge and depends on nothing else in the
  monorepo; the arrow points inward, so a future consumer embeds it rather than the reverse. The
  split is accepted by architecture ADR 0052.
- Add `harness-wire`: neutral conversation items, tool specifications, turn requests and outcomes,
  reported usage, stream events, size bounds, and the three ports the rest of the component is
  built on — `ModelPort`, `ToolPort` and `BearerSource`. It performs no I/O, reads no clock, holds
  no credential and names no vendor field, which is what lets a second wire cost a projection
  rather than a second loop. A provider item the component does not model is carried as an opaque
  value tagged with the wire that produced it; replaying one into a different wire is a typed
  refusal rather than a silent drop, and carrying reasoning items verbatim is what keeps a
  stateless loop as capable as a provider-threaded one.
- Add `harness-responses`: the `openai-responses` wire over `POST {base}/responses` in streaming
  mode. Bounded SSE reading refuses an oversized event, an oversized stream, an unparseable
  payload, and a stream that ends mid-event — a truncation is never read as a completion. Requests
  are stateless (`store: false`, the whole conversation replayed) and ask for encrypted reasoning
  content. HTTP statuses map to actionable codes: a rejected key is a non-retriable
  `Unauthorized`, a starting gateway is a retriable `Transport`. Arguments that are not JSON never
  reach a tool. Unreported usage stays absent rather than becoming zero.
- Add `harness-loop`: turn assembly, tool round trips, approvals and budgets. Because the loop is
  owned here it can count `max_turns`, input and output token totals, and a wall-clock deadline,
  so those bounds are enforced rather than hoped for; a spend ceiling is refused by name before the
  first request, since a gateway relays bytes and reports no price. An approval is an ordinary
  blocking call, so a decision cannot arrive after the effect, and the default approver denies. A
  call the run never published, a denied approval, an oversized argument set and an oversized
  result all return to the model as failed outcomes, so it learns the effect did not happen; the
  oversized payload is kept out of the replayed conversation so one bad call cannot poison every
  later turn.
- Add `b10x-harness`, the command-line shell, with a bounded read-only workspace toolset —
  `workspace.list`, `workspace.read`, `workspace.grep` — that refuses any path resolving outside the
  workspace, including through a symlink, and reports its own truncation rather than implying a
  partial answer is whole. Credentials come from an explicitly named file or environment variable
  with no ambient fallback. Ctrl-C ends the run rather than the process, cancelling both the loop
  and the response body being read. Exit status distinguishes an answer, a named stop, and a
  harness that could not run.
- Pin the wire in `contracts/provider-wires/openai-responses/2026-08-21`: the exact request the
  harness sends and the exact stream it accepts. Both halves are checked —
  `scripts/check-provider-wires.py` verifies the manifest against its fixtures, and a Rust contract
  test verifies the harness actually produces those bytes.
- Prove the composition against a real socket. A standard-library local Responses endpoint drives
  fifteen provider-emulated cases through the real client and the real loop, and seven end-to-end
  cases through the built binary over a real workspace. This is `provider_emulated` evidence and is
  never promoted to a claim about a real provider; no live run has happened.
- Register the component in the monorepo gate, `scripts/check-local.sh`.
- Add bridge mode: `b10x-harness app-server` serves B10x's own loop over the pinned
  Codex app-server JSON-RPC format on stdio, under the client's operation-tools profile. The real
  bridge has not driven it and no gate compares the two inventories; all evidence is this
  component's own client. `runtime/agent` already drives a process speaking that
  format and the command it spawns is arbitrary, so this reuses that entire bridge — its conformance
  suite, its governed execution lane, its process reaping — with no new bridge code and no
  dependency in either direction. A protocol is the seam; a shared crate would have been a coupling.
  Tools arrive from the client as `dynamicTools` on `thread/start` and are called back over the
  wire, which makes the bridged tool port the second implementation of the same `ToolPort` the
  embedded shell uses: in-process a tool call is a function call, here a round trip, and the loop
  cannot tell. `thread/resume` and `turn/steer` are refused by name rather than answered with a
  silent success, because a client told a thread resumed or a turn was steered would carry on
  believing something happened that did not. A run stopped by a budget is reported `failed`, not
  `completed`, and a failed or interrupted turn delivers no answer alongside its terminal frame.
- Make cancellation reach the layer that is actually blocked. One shared token now spans the loop,
  the tool sequence and the HTTP response body being read, replacing the per-layer flags; in bridge
  mode the reading thread sets it the instant a `turn/interrupt` frame is decoded, and the server
  acknowledges it between streamed events. A turn spends almost all its time blocked on the model,
  so an acknowledgement that waited for the main thread to return to the wire would arrive after the
  turn it was meant to stop had already finished.
- Treat a cancelled model read as a terminal outcome rather than an error, in every shell. A person
  who presses Ctrl-C was previously told the model wire had refused; the run now ends as cancelled,
  keeping the work it did complete and the usage it reported.
- Pin the served JSON-RPC subset in `contracts/app-server-profile/codex-app-server-stdio-v2/`, with
  a complete connection trace and a manifest. `scripts/check-app-server-profile.py` proves every
  frame is a declared method, every request is answered and every declared method is exercised; a
  Rust contract test proves the server's own constants match the manifest. The method inventory is a
  deliberate copy of the client's rather than an import — copying is what keeps the components
  independent, and nothing here can check it against the original, so a Codex version bump is a
  review obligation rather than a gated one.

### Fixed

Found by an independent review of the two slices above, before any of it was released.

- Stop bridge mode aborting the process whenever a client sent anything while a turn was running.
  Writing a notification held a borrow of the connection that draining control frames then took
  again, so a pipelined `turn/start` + `turn/interrupt` — the exact sequence an interrupt is for —
  killed the server with no terminal frame at all. A bridge saw only a dead pipe. Covered now by
  two regression tests that reach the interleaving through different frames.
- Declare the profile the client actually offers for tool calling. Bridge mode announced
  `codex-app-server-stdio-v2`, which is the client's *stable* profile: it registers no dynamic
  tools, refuses `item/tool/call` as an out-of-profile method, and cannot classify a
  `dynamicToolCall` item. The server would have looked compatible and failed at the first tool
  call. It now declares the operation-tools profile, requires the client to negotiate
  `experimentalApi` before accepting a tool registration, and refuses that registration by name
  otherwise rather than stranding the turn later.
- Give each turn its own cancellation token. A single token cleared at the start of every turn
  raced the reading thread: an interrupt decoded just before the clear was erased, and the turn it
  was meant to stop ran to completion while the client held an acknowledgement. `Cancel::reset` is
  removed, so the shape cannot come back.
- Report an interrupt that was actually requested as `interrupted` even when the connection drops
  afterwards. A later write failure used to overwrite it, reporting a person's own cancellation as
  a fault.
- Stop `workspace.grep` reading outside the workspace. The supplied path was checked, but the walk
  then followed a symlink inside the workspace and returned outside files under a workspace-relative
  name. Every entry is now re-checked after canonicalization. The previous symlink test covered
  `read` only.
- Preserve a failed tool result at the Responses wire. The projection dropped the `failed` flag, so
  a bridged client answering `{"success": false, "contentItems": []}` reached the model as an empty
  *successful* call — the exact failure mode the loop's refusal design exists to prevent.
- Cap both framers at the read rather than after it. A peer that never sent a newline chose how much
  memory this process allocated: measured 606 MiB before the 32 MiB stream bound was consulted.
- Bound the wait for a tool answer. A client that never answered `item/tool/call` held a turn open
  forever.
- End the turn when the client is gone. A broken pipe stopped the writing but let the loop keep
  calling the model, spending inference for a reader that would never see it.
- Answer every tool call the model made, even ones a cancellation skipped. A `function_call` left
  without its output makes the conversation unreplayable, so a cancelled run could not be resumed.
- Align the two bridge bounds with the client's: tool answers 64 KiB → 256 KiB, frames 4 MiB →
  8 MiB. The smaller values refused traffic the client is entitled to send, and the tool bound's own
  doc comment claimed it already matched.
- Use saturating arithmetic for the two token sums that were not, which panicked in debug and
  reported `totalTokens: 0` in release on a hostile usage report.
- Require the profile contract to exercise every declared *client* method, not only server ones.
  `turn/interrupt` was declared and untraced, which is how a crash on the interrupt path reached a
  green gate.
- Correct the documentation the review falsified: the `harness-wire` test count (33 → 26, the
  headline was right and the table was not), "the gate keeps the copy honest" (nothing compares the
  copy to the original), "`harness-wire` holds no credential" (it defines `StaticBearer`, which
  holds one for as long as its caller does), and the provider-wire README crediting the Python
  checker with checks the Rust test performs and with fields nothing checks at all.
