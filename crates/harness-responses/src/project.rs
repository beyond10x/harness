//! Projection between the neutral values and the pinned Responses subset.
//!
//! Everything vendor-shaped lives here. The loop above and the values below name no `OpenAI` field,
//! which is what lets a second wire cost a second projection instead of a second loop.

use harness_wire::{
    Item, MAX_TOOL_ARGUMENT_BYTES, StopReason, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest, Usage, WireError, WireId,
};
use serde_json::{Map, Value, json};

use crate::WIRE;

/// Projects one conversation item into a Responses `input` entry.
pub fn item_to_input(item: &Item) -> Value {
    match item {
        Item::UserText { text } => json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": text}],
        }),
        Item::AssistantText { text } => json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}],
        }),
        Item::ToolCall(call) => json!({
            "type": "function_call",
            "call_id": call.call_id.as_str(),
            "name": call.name.as_str(),
            "arguments": compact(&call.arguments),
        }),
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => json!({
            "type": "function_call_output",
            "call_id": call_id.as_str(),
            // The wire carries only text here, so the failure has to travel inside it. Dropping
            // the flag would hand the model an empty string it reads as a successful call.
            "output": if *failed {
                json!({"ok": false, "error": output}).to_string()
            } else {
                compact(output)
            },
        }),
        // Verbatim. Reinterpreting it would defeat the entire point of keeping it opaque.
        Item::Opaque { payload, .. } => payload.clone(),
    }
}

pub fn tool_to_wire(tool: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "name": tool.name.as_str(),
        "description": tool.description,
        "parameters": tool.input_schema,
        "strict": false,
    })
}

/// The character class this wire will publish a tool name in.
///
/// Read off a live 400 rather than a specification, and stated here rather than guessed at
/// elsewhere: on 2026-08-23 `https://chatgpt.com/backend-api/codex/responses` answered a toolset
/// named `workspace.list` / `.read` / `.grep` with
///
/// ```text
/// Invalid 'tools[0].name': string does not match pattern.
/// Expected a string that matches the pattern '^[a-zA-Z0-9_-]+$'.
/// ```
pub const TOOL_NAME_PATTERN: &str = "^[a-zA-Z0-9_-]+$";

/// Refuses a toolset this wire cannot publish, before anything is sent.
///
/// **Here and not in `harness-wire`.** The pattern is this provider's, verified against this
/// provider and no other; a neutral [`harness_wire::ToolName`] that enforced it would be shaped by
/// one vendor and would forbid a name a later wire may accept. What the neutral layer owes is that
/// the name is a printable identifier — what *this* wire owes is to say so locally instead of
/// letting the model's whole first turn cost a round trip and come back a 400.
///
/// The refusal names the tool and the pattern, because the caller's next move is to rename it and
/// an error that says only *bad request* does not tell them to.
///
/// # Errors
///
/// Returns [`harness_wire::WireErrorCode::Protocol`] — not retriable, since sending it again sends
/// the same name.
pub fn check_tool_names(tools: &[ToolSpec]) -> Result<(), WireError> {
    for tool in tools {
        let name = tool.name.as_str();
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

/// Decodes one Responses output item.
///
/// An item this wire does not model is kept as [`Item::Opaque`] and reported through `warn`. It is
/// never dropped: a dropped item is a hole in the conversation the next turn cannot see.
///
/// # Errors
///
/// Returns [`harness_wire::WireErrorCode::Protocol`] for a function call this crate cannot turn
/// into a callable request, and [`harness_wire::WireErrorCode::TooLarge`] past a declared bound.
pub fn output_item_to_item(
    wire: &WireId,
    value: &Value,
    warn: &mut dyn FnMut(String, String),
) -> Result<Item, WireError> {
    match value.get("type").and_then(Value::as_str) {
        Some("message") => Ok(Item::assistant(message_text(value))),
        Some("function_call") => function_call_to_item(value),
        Some("reasoning") => Ok(Item::Opaque {
            wire: wire.clone(),
            payload: value.clone(),
        }),
        other => {
            let kind = other.unwrap_or("<absent>").to_owned();
            warn(
                "unknown-output-item".to_owned(),
                format!("output item of type `{kind}` was preserved but not interpreted"),
            );
            Ok(Item::Opaque {
                wire: wire.clone(),
                payload: value.clone(),
            })
        }
    }
}

fn message_text(value: &Value) -> String {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter(|part| {
                    matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("output_text" | "text")
                    )
                })
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn function_call_to_item(value: &Value) -> Result<Item, WireError> {
    let call_id = value
        .get("call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::protocol("a function call arrived without a `call_id`"))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::protocol(format!("function call `{call_id}` has no name")))?;
    let raw = value
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            WireError::protocol(format!("function call `{call_id}` has no `arguments`"))
        })?;
    if raw.len() > MAX_TOOL_ARGUMENT_BYTES {
        return Err(WireError::too_large(format!(
            "arguments of call `{call_id}` are over the {MAX_TOOL_ARGUMENT_BYTES} byte bound"
        )));
    }
    // A half-parsed argument blob must never reach a tool: the tool would act on a value the model
    // did not send.
    let arguments: Value = serde_json::from_str(raw).map_err(|error| {
        WireError::protocol(format!(
            "arguments of call `{call_id}` are not JSON: {error}"
        ))
    })?;
    if !arguments.is_object() {
        return Err(WireError::protocol(format!(
            "arguments of call `{call_id}` are not a JSON object"
        )));
    }
    Ok(Item::ToolCall(ToolCall {
        call_id: id(call_id, "call id")?,
        name: ToolName::new(name)
            .map_err(|error| WireError::protocol(format!("tool name `{name}`: {error}")))?,
        arguments,
    }))
}

fn id(value: &str, kind: &str) -> Result<harness_wire::CallId, WireError> {
    harness_wire::CallId::new(value)
        .map_err(|error| WireError::protocol(format!("{kind} `{value}`: {error}")))
}

/// Reads reported token counts. Absent or unreadable usage stays absent.
pub fn usage_from_response(response: &Value, model: &str) -> Option<Usage> {
    let usage = response.get("usage")?.as_object()?;
    let input = usage.get("input_tokens")?.as_u64()?;
    let output = usage.get("output_tokens")?.as_u64()?;
    let cached = usage
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(Usage {
        model: response
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_owned(),
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cached,
        // Absent, not zero: this wire's provider reports no cache-write figure at all, and a zero
        // here would be this crate claiming on its behalf that it wrote nothing to its cache.
        cache_creation_input_tokens: None,
    })
}

/// Derives why the turn stopped from the terminal response object.
pub fn stop_reason(response: &Value, has_tool_calls: bool) -> StopReason {
    match response.get("status").and_then(Value::as_str) {
        Some("incomplete") => {
            let reason = response
                .get("incomplete_details")
                .and_then(|details| details.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if reason == "max_output_tokens" {
                StopReason::MaxOutputTokens
            } else {
                StopReason::Incomplete {
                    reason: reason.to_owned(),
                }
            }
        }
        _ if has_tool_calls => StopReason::ToolCalls,
        _ => StopReason::EndTurn,
    }
}

/// Reads the provider's own error object into a typed refusal.
pub fn response_error(response: &Value) -> WireError {
    // Two shapes reach here. `response.failed` carries a `response` object whose `error` field is
    // the reason; a top-level `error` event **is** the reason and carries `code` and `message`
    // itself. Reading only the nested one turned every `error` event into `unknown` with no
    // message, which is the provider's own explanation being dropped on the floor.
    let error = match response.get("error") {
        Some(nested) if nested.is_object() => nested,
        _ => response,
    };
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the provider failed the response without a message");
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let (code_class, retriable) = match code {
        // The far side failed on its own account, not on this request's: the same bytes sent
        // again may well be answered. A run has already paid for every turn before this one, and
        // ending it on somebody else's fault is the most expensive way to fail.
        "server_error" => (harness_wire::WireErrorCode::Transport, true),
        // Not a refusal of the request but a request for less traffic, which is what the back-off
        // above is for.
        "rate_limit_exceeded" => (harness_wire::WireErrorCode::RateLimited, true),
        // Everything else is this request being refused on its own terms — an unusable prompt, an
        // image this model cannot read, a bad parameter. Only these two are widened because only
        // these two are unambiguously the far side's own state; a code guessed into the retriable
        // set would spend a run's budget four times over to be refused identically.
        _ => (harness_wire::WireErrorCode::Refused, false),
    };
    WireError::new(
        code_class,
        format!("{WIRE} failed the response (`{code}`): {message}"),
        retriable,
    )
}

fn compact(value: &Value) -> String {
    // A string result is passed through unquoted; the model reads prose, not a quoted blob.
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_owned()),
    }
}

/// Builds the request body for one turn.
///
/// Takes the whole [`TurnRequest`] rather than its fields one by one: the neutral value is what a
/// caller already holds, every field here comes from it, and a projection that took them
/// separately grew an argument every time the wire learned a field — it was eight by the time
/// `tool_choice` arrived. `session` stays its own argument because it is **not** in the turn: it
/// is this route's prompt-cache key, minted per conversation by the client.
pub fn request_body(session: &str, request: &TurnRequest) -> Value {
    let TurnRequest {
        model,
        instructions,
        items,
        tools,
        max_output_tokens,
        sampling,
        tool_choice,
    } = request;
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(model));
    // **The standing instruction goes at the head of `input`, not in `instructions`.**
    //
    // Both are documented and the model reads them the same way — `developer` is the Responses
    // API's own replacement for a system message. What differs is the cache. A run whose whole
    // constant head sat in `instructions` measured 15–18% cached while `codex`, on the same route
    // and the same account, measured 85%; the one structural difference left, after the prefix was
    // proven stable and the head was lifted over the 1,024-token minimum, is that codex sends no
    // top-level `instructions` at all and puts its entire prompt inside `input`.
    //
    // So this follows the observation. It is also the shape that makes the head *count*: `input`
    // now begins with a thousand tokens that are identical on every turn of the run.
    let mut input = vec![json!({
        "type": "message",
        "role": "developer",
        "content": [{"type": "input_text", "text": instructions}],
    })];
    input.extend(items.iter().map(item_to_input));
    body.insert("input".to_owned(), Value::Array(input));
    body.insert(
        "tools".to_owned(),
        Value::Array(tools.iter().map(tool_to_wire).collect()),
    );
    body.insert("stream".to_owned(), json!(true));
    // Nothing is retained provider-side: the conversation is ours, replayed whole every turn.
    body.insert("store".to_owned(), json!(false));
    // Without this the model loses its own reasoning across every tool round trip under
    // `store: false`, and re-derives the plan on each call.
    body.insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
    // **The routing hint for prompt caching, in the shape the endpoint actually honours.**
    //
    // This loop is stateless: every turn replays the whole conversation, so turn n resends turn
    // n-1's prefix byte for byte. Without caching the cost of a run is quadratic in its turns, and
    // that term dominates everything else.
    //
    // The key is the **conversation**, not the prefix. A prefix digest was tried first and looked
    // more principled — two runs of one configuration share a prefix, so why not share a cache —
    // and it measured `cached_tokens: 0` on every turn of a live run. The endpoint was then
    // observed serving `codex` itself, on the same route and the same account, at **85% cached**;
    // what codex sends is the session's own id, matching the `session-id` it identifies the
    // conversation with. So this follows the observation rather than the argument.
    body.insert("prompt_cache_key".to_owned(), json!(session));
    if let Some(limit) = max_output_tokens {
        body.insert("max_output_tokens".to_owned(), json!(limit));
    }
    // Absent stays absent. A field nobody set is one the provider decides, and writing its default
    // in here would turn that provider's choice into ours without anybody having made it.
    if let Some(temperature) = sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
    }
    if let Some(effort) = &sampling.reasoning_effort {
        body.insert("reasoning".to_owned(), json!({"effort": effort}));
    }
    // Absent for `Auto`, for the reason above: the model choosing is this route's own default, and
    // sending `"auto"` would be us deciding it. `required` is this route's word for *some tool*;
    // a named tool is an object, and its `name` is the same name `tools` published.
    if let Some(choice) = tool_choice_to_wire(tool_choice) {
        body.insert("tool_choice".to_owned(), choice);
    }
    Value::Object(body)
}

/// This route's spelling of a tool choice, or [`None`] when the model decides.
fn tool_choice_to_wire(choice: &ToolChoice) -> Option<Value> {
    match choice {
        ToolChoice::Auto => None,
        ToolChoice::Required => Some(json!("required")),
        ToolChoice::Named(name) => Some(json!({"type": "function", "name": name.as_str()})),
    }
}

#[cfg(test)]
mod tests {
    use harness_wire::Sampling;

    /// One turn, from the pieces a test cares about.
    ///
    /// The projection takes the whole neutral value now, and a test that built one inline would
    /// bury what it is actually asserting under seven fields it does not care about.
    fn turn(
        model: &str,
        instructions: &str,
        items: &[Item],
        tools: &[ToolSpec],
        max_output_tokens: Option<u64>,
        sampling: &Sampling,
        tool_choice: &ToolChoice,
    ) -> TurnRequest {
        TurnRequest {
            model: model.to_owned(),
            instructions: instructions.to_owned(),
            items: items.to_vec(),
            tools: tools.to_vec(),
            max_output_tokens,
            sampling: sampling.clone(),
            tool_choice: tool_choice.clone(),
        }
    }

    #[test]
    fn a_tool_name_this_wire_cannot_publish_is_refused_before_it_is_sent() {
        use harness_wire::{ToolName, ToolSpec, WireErrorCode};
        use serde_json::json;

        let spec = |name: &str| ToolSpec {
            name: ToolName::new(name).expect("a printable identifier"),
            description: "d".to_owned(),
            envelope: Envelope::default(),
            input_schema: json!({"type": "object"}),
            approval: harness_wire::Approval::NotRequired,
        };

        // The live 400 of 2026-08-23, turned into a local refusal.
        let error = super::check_tool_names(&[spec("workspace.read")]).expect_err("refused");
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

        // And the class it will publish.
        super::check_tool_names(&[spec("workspace_read"), spec("with-hyphen"), spec("A9")])
            .expect("the publishable class passes");
    }

    use super::*;
    use harness_wire::Envelope;
    use harness_wire::{Approval, CallId, ToolOutcome};

    fn wire() -> WireId {
        WireId::new(WIRE).expect("the wire id is valid")
    }

    fn no_warn() -> impl FnMut(String, String) {
        |_, _| panic!("this case must not warn")
    }

    #[test]
    fn user_and_assistant_text_project_to_the_documented_shapes() {
        assert_eq!(
            item_to_input(&Item::user("hi")),
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]})
        );
        assert_eq!(
            item_to_input(&Item::assistant("ok")),
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]})
        );
    }

    #[test]
    fn a_tool_call_and_its_result_correlate_on_call_id() {
        let call = ToolCall {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new("workspace_read").expect("valid"),
            arguments: json!({"path": "README.md"}),
        };
        assert_eq!(
            item_to_input(&Item::ToolCall(call)),
            json!({
                "type": "function_call",
                "call_id": "call-1",
                "name": "workspace_read",
                "arguments": "{\"path\":\"README.md\"}",
            })
        );
        assert_eq!(
            item_to_input(&Item::result(
                CallId::new("call-1").expect("valid"),
                ToolOutcome::ok(json!({"bytes": 3})),
            )),
            json!({
                "type": "function_call_output",
                "call_id": "call-1",
                "output": "{\"bytes\":3}",
            })
        );
    }

    #[test]
    fn a_failed_result_is_never_projected_as_a_successful_one() {
        let item = Item::result(
            CallId::new("call-1").expect("valid"),
            ToolOutcome::failed("not granted"),
        );
        let output = item_to_input(&item)["output"]
            .as_str()
            .expect("the output is text")
            .to_owned();
        assert!(output.contains("\"ok\":false"), "{output}");
        assert!(output.contains("not granted"), "{output}");

        // The empty case is the one that actually bites: a bridged client answering
        // `{"success": false, "contentItems": []}` must not read as an empty success.
        let empty = Item::ToolResult {
            call_id: CallId::new("call-2").expect("valid"),
            output: json!(""),
            failed: true,
        };
        let output = item_to_input(&empty)["output"]
            .as_str()
            .expect("the output is text")
            .to_owned();
        assert_ne!(output, "");
        assert!(output.contains("\"ok\":false"), "{output}");
    }

    #[test]
    fn a_string_result_is_passed_through_unquoted() {
        let item = Item::result(
            CallId::new("call-1").expect("valid"),
            ToolOutcome::ok(json!("plain text")),
        );
        assert_eq!(item_to_input(&item)["output"], json!("plain text"));
    }

    #[test]
    fn opaque_items_are_replayed_byte_for_byte() {
        let payload = json!({"type":"reasoning","encrypted_content":"opaque-blob","id":"rs_1"});
        let item = Item::Opaque {
            wire: wire(),
            payload: payload.clone(),
        };
        assert_eq!(item_to_input(&item), payload);
    }

    #[test]
    fn a_reasoning_output_item_survives_as_opaque() {
        let value = json!({"type":"reasoning","id":"rs_1","encrypted_content":"blob"});
        let item = output_item_to_item(&wire(), &value, &mut no_warn()).expect("decodes");
        assert_eq!(
            item,
            Item::Opaque {
                wire: wire(),
                payload: value
            }
        );
    }

    #[test]
    fn a_message_output_item_concatenates_its_text_parts() {
        let value = json!({
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "output_text", "text": "Hel"},
                {"type": "output_text", "text": "lo"},
            ],
        });
        assert_eq!(
            output_item_to_item(&wire(), &value, &mut no_warn()).expect("decodes"),
            Item::assistant("Hello")
        );
    }

    #[test]
    fn a_function_call_decodes_its_arguments() {
        let value = json!({
            "type": "function_call",
            "call_id": "call-1",
            "name": "workspace_read",
            "arguments": "{\"path\":\"README.md\"}",
        });
        let Item::ToolCall(call) =
            output_item_to_item(&wire(), &value, &mut no_warn()).expect("decodes")
        else {
            panic!("a function call decodes to a tool call");
        };
        assert_eq!(call.arguments, json!({"path": "README.md"}));
    }

    #[test]
    fn malformed_arguments_never_reach_a_tool() {
        let value = json!({
            "type": "function_call",
            "call_id": "call-1",
            "name": "workspace_read",
            "arguments": "{not json",
        });
        let error = output_item_to_item(&wire(), &value, &mut no_warn())
            .expect_err("malformed arguments refuse");
        assert_eq!(error.code, harness_wire::WireErrorCode::Protocol);
        assert!(error.message.contains("call-1"), "{}", error.message);
    }

    #[test]
    fn empty_or_missing_arguments_refuse_instead_of_inventing_an_empty_object() {
        let value = json!({
            "type": "function_call",
            "call_id": "call-1",
            "name": "now",
            "arguments": "",
        });
        assert!(output_item_to_item(&wire(), &value, &mut no_warn()).is_err());
        let mut missing = value;
        missing.as_object_mut().expect("object").remove("arguments");
        assert!(output_item_to_item(&wire(), &missing, &mut no_warn()).is_err());
    }

    #[test]
    fn an_unknown_output_item_warns_and_is_preserved() {
        let value = json!({"type": "web_search_call", "id": "ws_1"});
        let mut warnings = Vec::new();
        let item = output_item_to_item(&wire(), &value, &mut |code, message| {
            warnings.push((code, message));
        })
        .expect("unknown items are preserved");
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
    fn absent_usage_stays_absent_and_never_becomes_zero() {
        assert_eq!(
            usage_from_response(&json!({"status": "completed"}), "m"),
            None
        );
        assert_eq!(
            usage_from_response(&json!({"status": "completed", "usage": null}), "m"),
            None
        );
    }

    #[test]
    fn reported_usage_is_read_with_its_cached_detail() {
        let response = json!({
            "model": "served-model",
            "usage": {
                "input_tokens": 11,
                "input_tokens_details": {"cached_tokens": 4},
                "output_tokens": 8,
            },
        });
        assert_eq!(
            usage_from_response(&response, "requested-model"),
            Some(Usage {
                model: "served-model".to_owned(),
                input_tokens: 11,
                output_tokens: 8,
                cached_input_tokens: 4,
                cache_creation_input_tokens: None,
            })
        );
    }

    #[test]
    fn stop_reasons_distinguish_a_budget_cut_from_a_finished_turn() {
        assert_eq!(
            stop_reason(&json!({"status": "completed"}), false),
            StopReason::EndTurn
        );
        assert_eq!(
            stop_reason(&json!({"status": "completed"}), true),
            StopReason::ToolCalls
        );
        assert_eq!(
            stop_reason(
                &json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}),
                false
            ),
            StopReason::MaxOutputTokens
        );
        assert_eq!(
            stop_reason(
                &json!({"status":"incomplete","incomplete_details":{"reason":"content_filter"}}),
                false
            ),
            StopReason::Incomplete {
                reason: "content_filter".to_owned()
            }
        );
    }

    #[test]
    fn a_failed_response_names_the_provider_code() {
        let error = response_error(
            &json!({"status":"failed","error":{"code":"server_error","message":"boom"}}),
        );
        assert_eq!(error.code, harness_wire::WireErrorCode::Transport);
        assert!(error.message.contains("server_error"), "{}", error.message);
        assert!(error.message.contains("boom"), "{}", error.message);
    }

    #[test]
    fn only_the_providers_own_failures_are_worth_another_attempt() {
        let failed = |code: &str| {
            response_error(&json!({"status": "failed", "error": {"code": code, "message": "m"}}))
        };
        assert!(failed("server_error").retriable);
        assert!(failed("rate_limit_exceeded").retriable);
        assert_eq!(
            failed("rate_limit_exceeded").code,
            harness_wire::WireErrorCode::RateLimited
        );
        // A request refused on its own terms is refused identically a second later.
        for code in [
            "invalid_prompt",
            "invalid_image",
            "invalid_image_format",
            "unknown",
        ] {
            let error = failed(code);
            assert_eq!(error.code, harness_wire::WireErrorCode::Refused, "{code}");
            assert!(!error.retriable, "{code}");
        }
    }

    #[test]
    fn a_top_level_error_event_carries_its_own_code_and_message() {
        // The `error` event puts them at the top level rather than under `error`. Read from the
        // wrong place they became `unknown` with no message, and a retriable class could never
        // fire because the code was never seen.
        let error = response_error(&json!({
            "type": "error",
            "code": "server_error",
            "message": "the gateway gave up",
        }));
        assert_eq!(error.code, harness_wire::WireErrorCode::Transport);
        assert!(error.retriable);
        assert!(
            error.message.contains("the gateway gave up"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_request_body_is_stateless_and_asks_for_reasoning() {
        let body = request_body(
            "session-under-test",
            &turn(
                "m",
                "be useful",
                &[Item::user("hi")],
                &[ToolSpec {
                    name: ToolName::new("t").expect("valid"),
                    description: "d".to_owned(),
                    envelope: Envelope::default(),
                    input_schema: json!({"type": "object"}),
                    approval: Approval::NotRequired,
                }],
                Some(256),
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
        );
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["max_output_tokens"], json!(256));
        assert_eq!(body["tools"][0]["type"], json!("function"));
        assert_eq!(body["tools"][0]["name"], json!("t"));
    }

    #[test]
    fn an_absent_output_bound_is_absent_from_the_body() {
        let body = request_body(
            "s",
            &turn(
                "m",
                "",
                &[],
                &[],
                None,
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
        );
        assert!(body.get("max_output_tokens").is_none(), "{body}");
    }

    #[test]
    fn sampling_nobody_set_is_absent_rather_than_defaulted() {
        let body = request_body(
            "s",
            &turn(
                "m",
                "",
                &[],
                &[],
                None,
                &Sampling::default(),
                &ToolChoice::Auto,
            ),
        );
        // Writing a default here would take a choice the provider is entitled to make and quietly
        // make it ours, and the request would look identical to one somebody actually chose.
        for field in ["temperature", "top_p", "reasoning"] {
            assert!(body.get(field).is_none(), "{field} leaked into {body}");
        }
    }

    #[test]
    fn each_sampling_field_travels_under_its_own_wire_name() {
        let body = request_body(
            "session-under-test",
            &turn(
                "m",
                "",
                &[],
                &[],
                None,
                &Sampling {
                    temperature: Some(0.2),
                    top_p: Some(0.95),
                    reasoning_effort: Some("high".to_owned()),
                },
                &ToolChoice::Auto,
            ),
        );
        assert_eq!(body["temperature"], json!(0.2));
        assert_eq!(body["top_p"], json!(0.95));
        // Effort is nested on this wire, not a flat field. A flat `reasoning_effort` is silently
        // ignored by the provider, which is the failure this assertion exists to prevent.
        assert_eq!(body["reasoning"], json!({"effort": "high"}));
    }

    /// This route's three spellings, and the one that is silence.
    ///
    /// `auto` is absent because the model choosing is this route's own default; `required` is a
    /// bare string; a named tool is an object whose `name` is the same name `tools` published.
    #[test]
    fn a_tool_choice_travels_in_this_routes_own_spelling_and_auto_is_absent() {
        use harness_wire::ToolChoice;

        let with = |choice: &ToolChoice| {
            request_body(
                "s",
                &turn(
                    "m",
                    "",
                    &[],
                    &[ToolSpec {
                        name: ToolName::new("answer").expect("valid"),
                        description: "d".to_owned(),
                        input_schema: json!({"type": "object"}),
                        approval: Approval::NotRequired,
                        envelope: Envelope::default(),
                    }],
                    None,
                    &Sampling::default(),
                    choice,
                ),
            )
        };

        let auto = with(&ToolChoice::Auto);
        assert!(auto.get("tool_choice").is_none(), "{auto}");
        assert_eq!(
            with(&ToolChoice::Required)["tool_choice"],
            json!("required")
        );
        assert_eq!(
            with(&ToolChoice::Named(ToolName::new("answer").expect("valid")))["tool_choice"],
            json!({"type": "function", "name": "answer"})
        );
    }
}
