//! Projection between the neutral values and the pinned Messages subset.
//!
//! Everything vendor-shaped lives here. The loop above and the values below name no Anthropic
//! field, which is what lets a second wire cost a second projection instead of a second loop.

use harness_wire::{
    Item, MAX_TOOL_ARGUMENT_BYTES, Sampling, StopReason, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest, Usage, WireError, WireErrorCode, WireId,
};
use serde_json::{Map, Value, json};

use crate::WIRE;

/// The character class and length this wire will publish a tool name in.
///
/// Read off the published tool-definition schema rather than guessed at, and stated here rather
/// than in `harness-wire`: it is *this* provider's rule. The class happens to match the Responses
/// wire's; the **length cap does not exist there at all**, and a neutral
/// [`harness_wire::ToolName`] that enforced either would be shaped by whichever vendor was
/// implemented first.
pub const TOOL_NAME_PATTERN: &str = "^[a-zA-Z0-9_-]{1,128}$";

/// Longest tool name this wire will publish.
pub const MAX_TOOL_NAME_BYTES: usize = 128;

/// Largest `temperature` this wire admits.
///
/// The neutral [`harness_wire::Sampling::validate`] admits `0.0..=2.0`, which is the *first*
/// wire's range and was never anything more general than that. This route tops out at 1.0, so a
/// value between the two ranges passes the neutral check and is refused here — by name, before a
/// round trip that would come back as a vendor error string nobody can act on.
pub const MAX_TEMPERATURE: f64 = 1.0;

/// This route's spelling of a tool choice, or [`None`] when the model decides.
fn tool_choice_to_wire(choice: &ToolChoice) -> Option<Value> {
    match choice {
        ToolChoice::Auto => None,
        ToolChoice::Required => Some(json!({"type": "any"})),
        ToolChoice::Named(name) => Some(json!({"type": "tool", "name": name.as_str()})),
    }
}

/// Projects one tool specification into a Messages tool definition.
pub fn tool_to_wire(tool: &ToolSpec) -> Value {
    json!({
        "name": tool.name.as_str(),
        "description": tool.description,
        "input_schema": tool.input_schema,
    })
}

/// Refuses a toolset this wire cannot publish, before anything is sent.
///
/// # Errors
///
/// Returns [`harness_wire::WireErrorCode::Protocol`] — not retriable, since sending it again sends
/// the same name.
pub fn check_tool_names(tools: &[ToolSpec]) -> Result<(), WireError> {
    for tool in tools {
        let name = tool.name.as_str();
        if name.len() > MAX_TOOL_NAME_BYTES {
            return Err(WireError::protocol(format!(
                "{WIRE} cannot publish the tool `{name}`: a tool name is at most \
                 {MAX_TOOL_NAME_BYTES} bytes on this wire and this one is {}",
                name.len()
            )));
        }
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(WireError::protocol(format!(
                "{WIRE} cannot publish the tool `{name}`: a tool name must match \
                 `{TOOL_NAME_PATTERN}` on this wire. Rename it — `{}` — and the toolset is \
                 publishable.",
                name.replace(['.', ' ', '/', ':'], "_")
            )));
        }
    }
    Ok(())
}

/// Refuses sampling this route will not take, before anything is sent.
///
/// # Errors
///
/// Returns [`harness_wire::WireErrorCode::Protocol`] naming the field and both ranges, so the
/// caller can see that the value was legal in the neutral layer and is not legal here.
pub fn check_sampling(sampling: &Sampling) -> Result<(), WireError> {
    if let Some(value) = sampling.temperature
        && value > MAX_TEMPERATURE
    {
        return Err(WireError::protocol(format!(
            "{WIRE} tops out at temperature {MAX_TEMPERATURE}; {value} is inside the neutral \
             range and outside this wire's"
        )));
    }
    Ok(())
}

/// Refuses a conversation this wire cannot carry, before anything is sent.
///
/// The Messages format is a list of **role-alternating messages** and the first must be the
/// person's. A neutral item list has no such rule, so a caller can hand this wire something the
/// Responses wire would have taken: a conversation that opens with an assistant item, or one that
/// is empty. Both come back from the provider as a 400 about `messages`, which does not say which
/// of the caller's items was wrong.
///
/// # Errors
///
/// Returns [`harness_wire::WireErrorCode::Protocol`].
pub fn check_conversation(items: &[Item]) -> Result<(), WireError> {
    let Some(first) = items.first() else {
        return Err(WireError::protocol(format!(
            "{WIRE} needs at least one item: this wire carries a conversation, not an instruction \
             on its own"
        )));
    };
    if role_of(first) != Role::User {
        return Err(WireError::protocol(format!(
            "{WIRE} needs the conversation to open with the person's own input; item 0 projects \
             to an assistant message"
        )));
    }
    Ok(())
}

/// Which side of the conversation one item belongs to.
///
/// The whole of the structural difference from the first wire. There, every item is its own entry
/// in a flat `input` array and the role rides on the entry. Here the transcript is a list of
/// **messages**, each with one role and a list of content blocks, and a tool result is a *user*
/// block answering a *tool call* that was an *assistant* block. Getting this wrong does not fail
/// loudly: it produces a transcript the model reads as somebody else having said its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

fn role_of(item: &Item) -> Role {
    match item {
        // A tool result is the *person's* turn on this wire: the harness is answering the model.
        Item::UserText { .. } | Item::ToolResult { .. } => Role::User,
        Item::AssistantText { .. } | Item::ToolCall(_) | Item::Opaque { .. } => Role::Assistant,
    }
}

/// Projects one conversation item into one Messages content block.
fn item_to_block(item: &Item) -> Value {
    match item {
        Item::UserText { text } | Item::AssistantText { text } => {
            json!({"type": "text", "text": text})
        }
        Item::ToolCall(call) => json!({
            "type": "tool_use",
            "id": call.call_id.as_str(),
            "name": call.name.as_str(),
            // A structured object, not a string. The first wire carries the arguments as encoded
            // text and this one carries them as JSON; a projection that stringified them here
            // would be sending the model a tool call it never made.
            "input": call.arguments,
        }),
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => json!({
            "type": "tool_result",
            "tool_use_id": call_id.as_str(),
            "content": text_of(output),
            // This wire has somewhere to put the failure, so it goes there *and* stays in the
            // text: a model that reads only the content must still see that the effect did not
            // happen (AGENTS.md invariant 9).
            "is_error": failed,
        }),
        // Verbatim. Reinterpreting it would defeat the entire point of keeping it opaque.
        Item::Opaque { payload, .. } => payload.clone(),
    }
}

/// The text a tool result carries, with a failure never able to read as an empty success.
fn text_of(output: &Value) -> String {
    match output {
        // A string result is passed through unquoted; the model reads prose, not a quoted blob.
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_owned()),
    }
}

/// Groups the conversation into role-alternating messages.
///
/// Consecutive items on the same side become **one** message with several content blocks, which is
/// the shape the wire requires: two adjacent assistant messages are rejected, and splitting the
/// tool results of one turn across two user messages teaches the model to stop calling tools in
/// parallel.
///
/// Order inside a message is the order the items arrived in, and nothing is sorted. That is what
/// keeps a replayed thinking block where the model put it — first — without this function ever
/// having to know what a thinking block is.
pub fn items_to_messages(items: &[Item]) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut role: Option<Role> = None;
    let mut blocks: Vec<Value> = Vec::new();
    for item in items {
        let next = role_of(item);
        if role != Some(next) {
            if let Some(previous) = role.take() {
                messages.push(message(previous, std::mem::take(&mut blocks)));
            }
            role = Some(next);
        }
        blocks.push(item_to_block(item));
    }
    if let Some(last) = role {
        messages.push(message(last, blocks));
    }
    messages
}

/// Content blocks this file builds itself, and may therefore annotate.
///
/// A `cache_control` marker **modifies** the block it lands on. On a block carried through from the
/// model — a `thinking` block, whose signature the provider verifies against the block as it was
/// produced — that is AGENTS.md invariant 5 broken and a turn the route rejects. An allowlist
/// rather than a denylist, because the blocks that must not be touched are exactly the ones this
/// crate has undertaken not to look inside, and a new opaque shape must not become markable by
/// nobody noticing.
const MARKABLE_BLOCKS: [&str; 2] = ["text", "tool_result"];

/// Places the rolling cache breakpoint on the tail of the conversation.
///
/// # What it costs a person not to have one
///
/// The loop is stateless: turn *n* resends turn *n−1*'s transcript byte for byte and adds to it.
/// With a breakpoint on the constant head alone, every byte the conversation grew by is paid for at
/// the full input rate on every remaining turn, so a run's input cost is quadratic in its turns. A
/// measured 81-turn run watched its cache hit rate fall from 66% to 12.5% and spent 1.33M input
/// tokens to produce 10.5k of output.
///
/// A breakpoint on the **last** block of the **last** message makes each turn write the prefix it
/// just read, so the next turn reads that back instead of paying for it again: the growth is
/// charged once rather than once per remaining turn.
///
/// # Why one rolling marker is enough
///
/// The marker moves, so the previous turn's write sits a few blocks earlier in this turn's request
/// than this turn's marker does. This route looks for a hit at the breakpoint **and** at the blocks
/// shortly before it, which is what makes a single moving marker chain turn to turn. That is the
/// provider's documented behaviour and **not** something measured here; if a run ever shows writes
/// with no reads, a second marker held one turn behind is the fix.
///
/// Two breakpoints total, against a cap of four, which leaves room for that second one.
///
/// Nothing is placed at all when the last message ends in blocks this file did not build — see
/// [`MARKABLE_BLOCKS`]. A missing breakpoint costs money; a modified `thinking` block costs the
/// turn.
fn mark_rolling_breakpoint(messages: &mut [Value]) {
    let Some(last) = messages.last_mut() else {
        return;
    };
    let Some(blocks) = last.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    let markable = blocks.iter_mut().rev().find(|block| {
        block
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| MARKABLE_BLOCKS.contains(&kind))
    });
    if let Some(block) = markable
        && let Some(object) = block.as_object_mut()
    {
        object.insert("cache_control".to_owned(), json!({"type": "ephemeral"}));
    }
}

fn message(role: Role, content: Vec<Value>) -> Value {
    json!({
        "role": match role {
            Role::User => "user",
            Role::Assistant => "assistant",
        },
        "content": Value::Array(content),
    })
}

/// Decodes one Messages content block.
///
/// A block this wire does not model is kept as [`Item::Opaque`] and reported through `warn`. It is
/// never dropped: a dropped item is a hole in the conversation the next turn cannot see.
///
/// # Errors
///
/// Returns [`harness_wire::WireErrorCode::Protocol`] for a tool call this crate cannot turn into a
/// callable request, and [`harness_wire::WireErrorCode::TooLarge`] past a declared bound.
pub fn block_to_item(
    wire: &WireId,
    value: &Value,
    warn: &mut dyn FnMut(String, String),
) -> Result<Item, WireError> {
    match value.get("type").and_then(Value::as_str) {
        Some("text") => Ok(Item::assistant(
            value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )),
        Some("tool_use") => tool_use_to_item(value),
        // **The reason invariant 5 exists on this wire.** A thinking block carries a signature the
        // provider verifies; it is replayed byte for byte or not at all, and it is meaningless to
        // any other wire. `redacted_thinking` is the same thing with the text withheld, and is
        // handled identically for exactly that reason — the payload is never read either way.
        Some("thinking" | "redacted_thinking") => Ok(Item::Opaque {
            wire: wire.clone(),
            payload: value.clone(),
        }),
        other => {
            let kind = other.unwrap_or("<absent>").to_owned();
            warn(
                "unknown-output-item".to_owned(),
                format!("content block of type `{kind}` was preserved but not interpreted"),
            );
            Ok(Item::Opaque {
                wire: wire.clone(),
                payload: value.clone(),
            })
        }
    }
}

fn tool_use_to_item(value: &Value) -> Result<Item, WireError> {
    let call_id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::protocol("a tool call arrived without an `id`"))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::protocol(format!("tool call `{call_id}` has no name")))?;
    let arguments = value.get("input").cloned().unwrap_or_else(|| json!({}));
    if harness_wire::exceeds(&arguments, MAX_TOOL_ARGUMENT_BYTES) {
        return Err(WireError::too_large(format!(
            "arguments of call `{call_id}` are over the {MAX_TOOL_ARGUMENT_BYTES} byte bound"
        )));
    }
    // A tool that expects an object must never be handed a bare scalar the model streamed. The
    // first wire catches this while parsing the argument text; here the value arrives already
    // decoded, so the check has to be explicit or it does not happen at all.
    if !arguments.is_object() {
        return Err(WireError::protocol(format!(
            "arguments of call `{call_id}` are not a JSON object"
        )));
    }
    Ok(Item::ToolCall(ToolCall {
        call_id: harness_wire::CallId::new(call_id)
            .map_err(|error| WireError::protocol(format!("call id `{call_id}`: {error}")))?,
        name: ToolName::new(name)
            .map_err(|error| WireError::protocol(format!("tool name `{name}`: {error}")))?,
        arguments,
    }))
}

/// Reads reported token counts. Absent or unreadable usage stays absent.
///
/// # The one place this wire has to do arithmetic on a provider's numbers
///
/// This route reports its three input figures **disjointly**: `input_tokens` excludes both cache
/// classes. [`harness_wire::Usage`] documents the opposite — its `input_tokens` is the whole, and
/// the cache figures are parts of it, which is what the only consumer computes an uncached count
/// from. So the three are summed here, once, where the difference is visible. Left unsummed, every
/// cached turn would report fewer input tokens than it was charged for and price itself low.
pub fn usage_from_message(message: &Value, model: &str) -> Option<Usage> {
    let usage = message.get("usage")?.as_object()?;
    let input = usage.get("input_tokens")?.as_u64()?;
    let output = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // Absent stays absent: a route that reports no cache-write figure has not said there was none.
    let created = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64);
    Some(Usage {
        model: message
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_owned(),
        input_tokens: input
            .saturating_add(cached)
            .saturating_add(created.unwrap_or(0)),
        output_tokens: output,
        cached_input_tokens: cached,
        cache_creation_input_tokens: created,
    })
}

/// Derives why the turn stopped from the reported stop reason.
///
/// A reason this crate does not model is carried through under its own name rather than flattened
/// into "finished": `pause_turn` and `refusal` are both turns that did **not** end the way
/// `end_turn` did, and a caller told otherwise would report a refused run as a completed one.
pub fn stop_reason(reported: Option<&str>, has_tool_calls: bool) -> StopReason {
    match reported {
        Some("max_tokens") => StopReason::MaxOutputTokens,
        Some("tool_use") => StopReason::ToolCalls,
        Some("end_turn") | None => {
            if has_tool_calls {
                StopReason::ToolCalls
            } else {
                StopReason::EndTurn
            }
        }
        Some(other) => StopReason::Incomplete {
            reason: other.to_owned(),
        },
    }
}

/// Reads the provider's own error object into a typed refusal.
pub fn stream_error(event: &Value) -> WireError {
    let error = event.get("error");
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("the provider failed the message without a message");
    let kind = error
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    // **Which failures may not repeat.** An overloaded route is the far side asking for less
    // traffic, not a refusal of this request: sending it again later is exactly the right move,
    // and `Refused` would end the run instead. An `api_error` is the far side failing on its own
    // account, which is the same argument.
    //
    // Everything else this route documents is about *this request* and is refused identically a
    // second later — `invalid_request_error`, `authentication_error`, `permission_error`,
    // `not_found_error`, `request_too_large` — so they fall through and are not retried. They are
    // listed here rather than matched because the answer for each is the fall-through answer, and
    // an arm that only restated it would be a second place to keep in step.
    let (code, retriable) = match kind {
        "overloaded_error" => (WireErrorCode::RateLimited, true),
        "api_error" => (WireErrorCode::Transport, true),
        _ => (WireErrorCode::Refused, false),
    };
    WireError::new(
        code,
        format!("{WIRE} failed the message (`{kind}`): {message}"),
        retriable,
    )
}

/// Builds the request body for one turn.
///
/// Takes the whole [`TurnRequest`] rather than its fields one by one: the neutral value is what a
/// caller already holds, and a projection that took them separately grew an argument every time
/// the wire learned a field.
///
/// `max_output_tokens` is the exception and is passed beside it — a plain `u64` and not an option,
/// unlike the first wire's. This route **requires** `max_tokens`, so absence cannot be preserved
/// on it; the caller resolves what to send before calling, and where that number came from is a
/// decision written down at the call site rather than invented here.
pub fn request_body(request: &TurnRequest, max_output_tokens: u64) -> Value {
    let TurnRequest {
        model,
        instructions,
        items,
        tools,
        sampling,
        tool_choice,
        // Resolved by the caller: see this function's own note above. The turn's own value is
        // read there and never here, so there is one place that decides what this route is sent.
        max_output_tokens: _,
    } = request;
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(model));
    body.insert("max_tokens".to_owned(), json!(max_output_tokens));
    // **The standing instruction is a block list, not a string, so it can carry a cache
    // breakpoint.** The render order this route caches over is `tools` then `system` then
    // `messages`, so a breakpoint at the end of `system` covers the whole constant head of every
    // turn of the run. That matters more here than a tidier string would: the loop is stateless
    // and replays its conversation, so the head is re-sent on every turn and is paid for at the
    // full input rate without one.
    body.insert(
        "system".to_owned(),
        json!([{
            "type": "text",
            "text": instructions,
            "cache_control": {"type": "ephemeral"},
        }]),
    );
    let mut messages = items_to_messages(items);
    mark_rolling_breakpoint(&mut messages);
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert(
        "tools".to_owned(),
        Value::Array(tools.iter().map(tool_to_wire).collect()),
    );
    body.insert("stream".to_owned(), json!(true));
    // Absent stays absent. A field nobody set is one the provider decides, and writing its default
    // in here would turn that provider's choice into ours without anybody having made it.
    if let Some(temperature) = sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
    }
    // Nested under `output_config`, not under `reasoning` as on the first wire, and not flat.
    // Two wires, two spellings of one neutral field, and neither is guessable from the other.
    if let Some(effort) = &sampling.reasoning_effort {
        body.insert("output_config".to_owned(), json!({"effort": effort}));
    }
    // Absent for `Auto`: the model choosing is this route's own default, and sending `auto` would
    // be us deciding it. This route spells every choice as an object — `any` is its word for *some
    // tool*, `tool` names one — where the first wire spells two of the three as bare strings.
    //
    // **It goes after `tools` and `system` in the body and is not covered by their cache
    // breakpoint.** This route caches `tools`, then `system`, then `messages`; a turn that changes
    // this field is a turn whose prefix the route may not serve from cache. The loop sends it on
    // one turn per run at most (`AgentLoop::held_to_the_answer`), which is what makes that
    // acceptable — a run holding every turn to a tool would pay full rate for all of them.
    if let Some(choice) = tool_choice_to_wire(tool_choice) {
        body.insert("tool_choice".to_owned(), choice);
    }
    Value::Object(body)
}

#[cfg(test)]
mod tests {
    /// One turn, from the pieces a test cares about. `max_output_tokens` is resolved by the caller
    /// on this route and is passed to the projection beside the turn, not inside it.
    fn turn(
        model: &str,
        instructions: &str,
        items: &[Item],
        tools: &[ToolSpec],
        sampling: &Sampling,
        tool_choice: &ToolChoice,
    ) -> TurnRequest {
        TurnRequest {
            model: model.to_owned(),
            instructions: instructions.to_owned(),
            items: items.to_vec(),
            tools: tools.to_vec(),
            max_output_tokens: None,
            sampling: sampling.clone(),
            tool_choice: tool_choice.clone(),
        }
    }

    use super::*;
    use harness_wire::{Approval, CallId, Envelope, ToolOutcome};

    fn wire() -> WireId {
        WireId::new(WIRE).expect("the wire id is valid")
    }

    fn no_warn() -> impl FnMut(String, String) {
        |_, _| panic!("this case must not warn")
    }

    fn spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: ToolName::new(name).expect("a printable identifier"),
            description: "d".to_owned(),
            input_schema: json!({"type": "object"}),
            approval: Approval::NotRequired,
            envelope: Envelope::default(),
        }
    }

    #[test]
    fn a_tool_name_this_wire_cannot_publish_is_refused_before_it_is_sent() {
        let error = check_tool_names(&[spec("workspace.read")]).expect_err("refused");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(!error.retriable, "the same name would fail again");
        assert!(
            error.message.contains("workspace.read"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("workspace_read"),
            "names the fix: {}",
            error.message
        );
        check_tool_names(&[spec("workspace_read"), spec("with-hyphen"), spec("A9")])
            .expect("the publishable class passes");
    }

    #[test]
    fn a_tool_name_over_this_wires_length_cap_is_refused_by_name() {
        // The cap the first wire does not have. A neutral `ToolName` admits 256 bytes, so a name
        // between the two limits passes every check above this one.
        let long = "t".repeat(MAX_TOOL_NAME_BYTES + 1);
        let error = check_tool_names(&[spec(&long)]).expect_err("an over-long name refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.message.contains("129"), "{}", error.message);
        check_tool_names(&[spec(&"t".repeat(MAX_TOOL_NAME_BYTES))]).expect("the cap itself passes");
    }

    #[test]
    fn a_temperature_the_neutral_layer_admits_and_this_wire_does_not_is_refused_here() {
        let sampling = Sampling {
            temperature: Some(1.5),
            ..Sampling::default()
        };
        // Legal in the neutral layer, which is the whole point of checking again.
        sampling.validate().expect("the neutral range admits 1.5");
        let error = check_sampling(&sampling).expect_err("this wire does not");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.message.contains("1.5"), "{}", error.message);
        check_sampling(&Sampling {
            temperature: Some(1.0),
            ..Sampling::default()
        })
        .expect("the cap itself passes");
    }

    #[test]
    fn a_conversation_that_does_not_open_with_the_person_is_refused_by_index() {
        let error = check_conversation(&[Item::assistant("hi")]).expect_err("refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.message.contains("item 0"), "{}", error.message);
        assert_eq!(
            check_conversation(&[])
                .expect_err("an empty conversation refuses")
                .code,
            WireErrorCode::Protocol
        );
        check_conversation(&[Item::user("hi")]).expect("a person's input opens it");
    }

    #[test]
    fn consecutive_items_on_one_side_become_one_message() {
        let call_id = CallId::new("toolu_1").expect("valid");
        let items = vec![
            Item::user("read the readme"),
            Item::Opaque {
                wire: wire(),
                payload: json!({"type": "thinking", "thinking": "…", "signature": "SIG"}),
            },
            Item::assistant("Reading it."),
            Item::ToolCall(ToolCall {
                call_id: call_id.clone(),
                name: ToolName::new("workspace_read").expect("valid"),
                arguments: json!({"path": "README.md"}),
            }),
            Item::result(call_id, ToolOutcome::ok(json!("hello harness"))),
        ];
        let messages = items_to_messages(&items);
        assert_eq!(messages.len(), 3, "{messages:#?}");
        assert_eq!(messages[0]["role"], json!("user"));
        assert_eq!(messages[1]["role"], json!("assistant"));
        // The thinking block stays where the model put it — first — because nothing here sorts.
        assert_eq!(messages[1]["content"][0]["type"], json!("thinking"));
        assert_eq!(messages[1]["content"][1]["type"], json!("text"));
        assert_eq!(messages[1]["content"][2]["type"], json!("tool_use"));
        // A tool result is the person's turn on this wire, not the model's.
        assert_eq!(messages[2]["role"], json!("user"));
        assert_eq!(messages[2]["content"][0]["type"], json!("tool_result"));
        assert_eq!(messages[2]["content"][0]["tool_use_id"], json!("toolu_1"));
    }

    #[test]
    fn tool_arguments_travel_as_an_object_rather_than_as_encoded_text() {
        let block = item_to_block(&Item::ToolCall(ToolCall {
            call_id: CallId::new("toolu_1").expect("valid"),
            name: ToolName::new("workspace_read").expect("valid"),
            arguments: json!({"path": "README.md"}),
        }));
        assert_eq!(block["input"], json!({"path": "README.md"}));
        assert!(block["input"].is_object(), "not a string: {block}");
    }

    #[test]
    fn a_failed_result_is_never_projected_as_a_successful_one() {
        let block = item_to_block(&Item::result(
            CallId::new("toolu_1").expect("valid"),
            ToolOutcome::failed("not granted"),
        ));
        assert_eq!(block["is_error"], json!(true));
        assert_eq!(block["content"], json!("not granted"));

        // The empty case is the one that actually bites: a result whose text is empty must not
        // read as an empty success, so the flag is the thing that carries it.
        let empty = item_to_block(&Item::ToolResult {
            call_id: CallId::new("toolu_2").expect("valid"),
            output: json!(""),
            failed: true,
        });
        assert_eq!(empty["is_error"], json!(true));
    }

    #[test]
    fn opaque_items_are_replayed_byte_for_byte() {
        let payload = json!({"type": "thinking", "thinking": "…", "signature": "SIG"});
        assert_eq!(
            item_to_block(&Item::Opaque {
                wire: wire(),
                payload: payload.clone(),
            }),
            payload
        );
    }

    #[test]
    fn both_thinking_block_kinds_survive_as_opaque() {
        for payload in [
            json!({"type": "thinking", "thinking": "…", "signature": "SIG"}),
            json!({"type": "redacted_thinking", "data": "OPAQUE"}),
        ] {
            assert_eq!(
                block_to_item(&wire(), &payload, &mut no_warn()).expect("decodes"),
                Item::Opaque {
                    wire: wire(),
                    payload
                }
            );
        }
    }

    #[test]
    fn an_unknown_content_block_warns_and_is_preserved() {
        let value = json!({"type": "server_tool_use", "id": "srvtoolu_1"});
        let mut warnings = Vec::new();
        let item = block_to_item(&wire(), &value, &mut |code, message| {
            warnings.push((code, message));
        })
        .expect("unknown blocks are preserved");
        assert_eq!(
            item,
            Item::Opaque {
                wire: wire(),
                payload: value
            }
        );
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "unknown-output-item");
    }

    #[test]
    fn a_tool_call_whose_arguments_are_not_an_object_never_reaches_a_tool() {
        let value = json!({"type": "tool_use", "id": "toolu_1", "name": "t", "input": "oops"});
        let error =
            block_to_item(&wire(), &value, &mut no_warn()).expect_err("a scalar input refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.message.contains("toolu_1"), "{}", error.message);
    }

    #[test]
    fn absent_usage_stays_absent_and_never_becomes_zero() {
        assert_eq!(usage_from_message(&json!({}), "m"), None);
        assert_eq!(usage_from_message(&json!({"usage": null}), "m"), None);
    }

    #[test]
    fn disjoint_input_figures_are_summed_into_the_neutral_total() {
        // The provider says 11 fresh, 4 read from cache, 6 written to it. The neutral contract is
        // that `input_tokens` is the whole and the cache figures are parts, so the total is 21 —
        // reporting 11 would price every cached turn low.
        let message = json!({
            "model": "served-model",
            "usage": {
                "input_tokens": 11,
                "cache_read_input_tokens": 4,
                "cache_creation_input_tokens": 6,
                "output_tokens": 8,
            },
        });
        assert_eq!(
            usage_from_message(&message, "requested-model"),
            Some(Usage {
                model: "served-model".to_owned(),
                input_tokens: 21,
                output_tokens: 8,
                cached_input_tokens: 4,
                cache_creation_input_tokens: Some(6),
            })
        );
    }

    #[test]
    fn an_unreported_cache_write_stays_absent() {
        let message = json!({"usage": {"input_tokens": 5, "output_tokens": 1}});
        let usage = usage_from_message(&message, "m").expect("usage was reported");
        assert_eq!(usage.cache_creation_input_tokens, None);
        assert_eq!(usage.input_tokens, 5);
    }

    #[test]
    fn stop_reasons_distinguish_a_budget_cut_a_tool_call_and_a_refusal() {
        assert_eq!(stop_reason(Some("end_turn"), false), StopReason::EndTurn);
        assert_eq!(stop_reason(Some("end_turn"), true), StopReason::ToolCalls);
        assert_eq!(stop_reason(Some("tool_use"), true), StopReason::ToolCalls);
        assert_eq!(
            stop_reason(Some("max_tokens"), false),
            StopReason::MaxOutputTokens
        );
        // Carried under its own name. A refusal reported as `EndTurn` is a refused run a caller
        // would write down as completed.
        for reason in ["refusal", "pause_turn", "stop_sequence"] {
            assert_eq!(
                stop_reason(Some(reason), false),
                StopReason::Incomplete {
                    reason: reason.to_owned()
                }
            );
        }
    }

    #[test]
    fn an_overloaded_route_is_retriable_and_a_refusal_is_not() {
        let overloaded = stream_error(&json!({
            "type": "error",
            "error": {"type": "overloaded_error", "message": "busy"},
        }));
        assert_eq!(overloaded.code, WireErrorCode::RateLimited);
        assert!(overloaded.retriable, "asking again later is the right move");

        let api = stream_error(&json!({
            "type": "error",
            "error": {"type": "api_error", "message": "upstream exploded"},
        }));
        assert_eq!(api.code, WireErrorCode::Transport);
        assert!(api.retriable, "the far side failed on its own account");

        let refused = stream_error(&json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "boom"},
        }));
        assert_eq!(refused.code, WireErrorCode::Refused);
        assert!(!refused.retriable);
        assert!(refused.message.contains("boom"), "{}", refused.message);
    }

    #[test]
    fn every_error_type_this_route_documents_about_the_request_itself_is_final() {
        // Named one by one because the cost of getting this wrong is asymmetric: a retriable
        // classification here spends a run's budget four times over to be refused identically,
        // and the failure looks like slowness rather than like a mistake.
        for kind in [
            "invalid_request_error",
            "authentication_error",
            "permission_error",
            "not_found_error",
            "request_too_large",
        ] {
            let error = stream_error(&json!({
                "type": "error",
                "error": {"type": kind, "message": "m"},
            }));
            assert_eq!(error.code, WireErrorCode::Refused, "{kind}");
            assert!(!error.retriable, "{kind}");
            assert!(error.message.contains(kind), "{}", error.message);
        }
    }

    #[test]
    fn the_request_body_is_stateless_and_carries_a_cache_breakpoint() {
        let body = request_body(
            &turn(
                "m",
                "be useful",
                &[Item::user("hi")],
                &[spec("t")],
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
            4096,
        );
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["max_tokens"], json!(4096));
        assert_eq!(body["system"][0]["text"], json!("be useful"));
        assert_eq!(
            body["system"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert_eq!(body["tools"][0]["name"], json!("t"));
        // No conversation identifier and nothing retained: the transcript is ours and is replayed
        // whole on every turn (AGENTS.md invariant 4).
        for field in ["conversation_id", "previous_message_id", "store"] {
            assert!(body.get(field).is_none(), "{field} leaked into {body}");
        }
    }

    #[test]
    fn the_growing_tail_of_the_conversation_carries_the_rolling_breakpoint() {
        // Where the marker lands is the whole of the change: on the **last** content block of the
        // **last** message, which on this loop is the user message carrying either the person's
        // input or the tool results just appended.
        let body = request_body(
            &turn(
                "m",
                "be useful",
                &[
                    Item::user("read the readme"),
                    Item::ToolCall(ToolCall {
                        call_id: CallId::new("toolu_1").expect("valid"),
                        name: ToolName::new("workspace_read").expect("valid"),
                        arguments: json!({"path": "README.md"}),
                    }),
                    Item::result(
                        CallId::new("toolu_1").expect("valid"),
                        ToolOutcome::ok(json!({"text": "hello harness"})),
                    ),
                ],
                &[spec("workspace_read")],
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
            4096,
        );
        let messages = body["messages"].as_array().expect("an array");
        let last = messages.last().expect("a last message");
        assert_eq!(last["role"], json!("user"), "the tail is the person's turn");
        let blocks = last["content"].as_array().expect("an array");
        assert_eq!(
            blocks.last().expect("a last block")["cache_control"],
            json!({"type": "ephemeral"})
        );
        // Only the tail, and only once: the earlier user message keeps none, or the marker would
        // stop moving and the cache would stop chaining.
        assert!(
            messages[0]["content"][0].get("cache_control").is_none(),
            "{body}"
        );
        // The constant head keeps its own, and the two are the only ones. The route admits four.
        assert_eq!(
            body["system"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert_eq!(
            body.to_string().matches("cache_control").count(),
            2,
            "at most four breakpoints are admitted and two is what this sends: {body}"
        );
    }

    #[test]
    fn a_replayed_thinking_block_is_never_the_thing_that_gets_marked() {
        // Invariant 5, from the direction that would be easy to break by accident: the signature
        // covers the block as the model produced it, so a `cache_control` key added to one is a
        // turn the route rejects. Here the model spoke last and its message ends in a thinking
        // block, so the marker must fall on the text block before it — or nowhere.
        let body = request_body(
            &turn(
                "m",
                "",
                &[
                    Item::user("hi"),
                    Item::assistant("thinking about it"),
                    Item::Opaque {
                        wire: wire(),
                        payload: json!({
                            "type": "thinking",
                            "thinking": "OPAQUE",
                            "signature": "SIG",
                        }),
                    },
                ],
                &[],
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
            1024,
        );
        let last = body["messages"]
            .as_array()
            .expect("an array")
            .last()
            .expect("a message");
        let blocks = last["content"].as_array().expect("an array");
        let thinking = blocks
            .iter()
            .find(|block| block["type"] == json!("thinking"))
            .expect("the thinking block is replayed");
        assert_eq!(
            *thinking,
            json!({"type": "thinking", "thinking": "OPAQUE", "signature": "SIG"}),
            "a replayed block is sent byte for byte or not at all"
        );
        assert_eq!(
            blocks[0]["cache_control"],
            json!({"type": "ephemeral"}),
            "the marker falls back to the last block this projection built: {body}"
        );
    }

    #[test]
    fn a_conversation_with_nothing_markable_in_its_tail_simply_carries_no_rolling_marker() {
        // A missing breakpoint costs money; a modified opaque block costs the turn. So this is the
        // side to fail on, and it must not panic or mark something it does not understand.
        let body = request_body(
            &turn(
                "m",
                "",
                &[
                    Item::user("hi"),
                    Item::Opaque {
                        wire: wire(),
                        payload: json!({"type": "redacted_thinking", "data": "OPAQUE"}),
                    },
                ],
                &[],
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
            1024,
        );
        let last = body["messages"]
            .as_array()
            .expect("an array")
            .last()
            .expect("a message");
        assert!(
            last["content"]
                .as_array()
                .expect("an array")
                .iter()
                .all(|block| block.get("cache_control").is_none()),
            "{body}"
        );
    }

    #[test]
    fn sampling_nobody_set_is_absent_rather_than_defaulted() {
        let body = request_body(
            &turn(
                "m",
                "",
                &[Item::user("hi")],
                &[],
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
            1024,
        );
        for field in ["temperature", "top_p", "output_config"] {
            assert!(body.get(field).is_none(), "{field} leaked into {body}");
        }
    }

    #[test]
    fn each_sampling_field_travels_under_its_own_wire_name() {
        let body = request_body(
            &turn(
                "m",
                "",
                &[Item::user("hi")],
                &[],
                &Sampling {
                    temperature: Some(0.2),
                    top_p: Some(0.95),
                    reasoning_effort: Some("medium".to_owned()),
                },
                &ToolChoice::Auto,
            ),
            1024,
        );
        assert_eq!(body["temperature"], json!(0.2));
        assert_eq!(body["top_p"], json!(0.95));
        // Under `output_config` here and under `reasoning` on the first wire. A projection that
        // reused the other spelling would be silently ignored by this route.
        assert_eq!(body["output_config"], json!({"effort": "medium"}));
        assert!(body.get("reasoning").is_none(), "{body}");
    }

    /// This route spells every choice as an object, where the first wire spells two of three as
    /// bare strings. Two wires, two spellings of one neutral field, neither guessable from the
    /// other — and `auto` is absent on both, because that is the provider's own default.
    #[test]
    fn a_tool_choice_travels_in_this_routes_own_spelling_and_auto_is_absent() {
        use harness_wire::ToolChoice;

        let with = |choice: &ToolChoice| {
            request_body(
                &turn(
                    "m",
                    "",
                    &[],
                    &[spec("answer")],
                    &Sampling::default(),
                    choice,
                ),
                4096,
            )
        };

        let auto = with(&ToolChoice::Auto);
        assert!(auto.get("tool_choice").is_none(), "{auto}");
        assert_eq!(
            with(&ToolChoice::Required)["tool_choice"],
            json!({"type": "any"})
        );
        assert_eq!(
            with(&ToolChoice::Named(ToolName::new("answer").expect("valid")))["tool_choice"],
            json!({"type": "tool", "name": "answer"})
        );
    }
}
