//! The model port over `llm`'s client crates, and the credential sources that feed them.
//!
//! The one place Harness's neutral values (`harness_wire`) meet `llm_core`'s. Everything vendor
//! shaped — request bodies, stream decoding, status classes, credential presentation, Codex login
//! renewal — is `llm`'s; this module only translates values and bridges two differences of
//! execution model: Harness's loop is blocking and its sink is not `Send`, and its cancellation
//! token is a flag rather than a waker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use harness_wire::{
    CallId, Cancel, Item, ModelPort, StopReason, StreamEvent, StreamSink, ToolCall, ToolName,
    TurnOutcome, TurnRequest, Usage, WireError, WireErrorCode, WireId,
};
use llm_blocking::BlockingModel;
use llm_core::{
    AuthKind, BillingKind, BoxFuture, Capabilities, ErrorCode, Id, Model, Protocol, Provenance,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, ApiKeyHeader, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel,
    ServingModel,
};

/// The identifier the Responses wire tags its opaque items with. Unchanged from the retired
/// `harness-responses` crate, because sessions on disk carry it.
pub const RESPONSES_WIRE: &str = "openai-responses";
/// The identifier the Messages wire tags its opaque items with, as `harness-messages` did.
pub const MESSAGES_WIRE: &str = "anthropic-messages";

/// What this client calls itself on the Responses wire, as `harness-responses` did.
const ORIGINATOR: &str = "b10x-harness";
/// The output bound a Messages turn is sent when the run named none, as `harness-messages` did.
const DEFAULT_MESSAGES_MAX_OUTPUT_TOKENS: u64 = 8192;
/// The reasoning efforts a binding declares. `llm` refuses an effort its binding does not
/// declare; Harness passed any string through, and `--reasoning-effort` still takes any string.
/// `max` is sent as it is on Messages and as the highest Responses effort, `xhigh`
/// (`llm_core::controls::wire_effort`).
const REASONING_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
/// How often the calling thread looks at Harness's cancellation flag while a turn runs.
const CANCEL_POLL: Duration = Duration::from_millis(10);
/// Connect bound, as Harness's `Settings::streaming`.
const CONNECT: Duration = Duration::from_secs(15);
/// Harness's transport had a 180 s per-read bound and no whole-turn bound; `llm` requires one, so
/// the turn gets the widest it admits.
const LIMITS: Limits = Limits {
    response_headers: Duration::from_secs(180),
    idle: Duration::from_secs(180),
    total: Duration::from_hours(24),
};

/// How a credential is presented on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    /// A key issued to a program.
    ApiKey,
    /// A token obtained on a person's behalf.
    Subscription,
}

/// A credential source: an `llm` resolver bound to one reference, and how it is presented.
#[derive(Clone)]
pub struct Source {
    reference: SecretRef,
    resolver: Arc<dyn SecretResolver>,
    presentation: Presentation,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Source")
            .field("reference", &self.reference.as_str())
            .field("presentation", &self.presentation)
            .finish_non_exhaustive()
    }
}

impl Source {
    /// A key the run already read, held for the process lifetime.
    ///
    /// # Errors
    /// Names the refusal when the key is larger than `llm` accepts.
    pub fn key(value: &str) -> Result<Self, String> {
        let reference = reference("api-key")?;
        let secret = Secret::new(value.as_bytes().to_vec()).map_err(|error| error.to_string())?;
        Ok(Self {
            reference,
            resolver: Arc::new(HeldKey(secret)),
            presentation: Presentation::ApiKey,
        })
    }

    /// A subscription token re-read from `path` on every request, optionally at a JSON pointer.
    ///
    /// # Errors
    /// Names the refusal for a path `llm` will not read or a pointer that is not RFC 6901.
    pub fn file(path: &Path, pointer: Option<&str>) -> Result<Self, String> {
        let reference = reference("oauth-token-file")?;
        let absolute = std::path::absolute(path)
            .map_err(|error| format!("the credential file `{}`: {error}", path.display()))?;
        let resolver = llm_credentials::file::FileResolver::new(BTreeMap::from([(
            reference.clone(),
            absolute,
        )]))
        .map_err(|error| format!("the credential file `{}`: {error}", path.display()))?;
        Self::subscription(reference, Arc::new(resolver), pointer)
    }

    /// A subscription token re-read from the variable `name` on every request.
    ///
    /// # Errors
    /// Names the refusal for a variable name or pointer `llm` will not accept.
    pub fn environment(name: &str, pointer: Option<&str>) -> Result<Self, String> {
        let reference = reference("oauth-token-env")?;
        let resolver = llm_credentials::environment::EnvironmentResolver::new(BTreeMap::from([(
            reference.clone(),
            name.to_owned(),
        )]))
        .map_err(|error| format!("the environment variable `{name}`: {error}"))?;
        Self::subscription(reference, Arc::new(resolver), pointer)
    }

    fn subscription(
        reference: SecretRef,
        source: Arc<dyn SecretResolver>,
        pointer: Option<&str>,
    ) -> Result<Self, String> {
        let resolver: Arc<dyn SecretResolver> = match pointer {
            None => source,
            Some(pointer) => Arc::new(
                llm_credentials::pointer::JsonPointerResolver::new(
                    source,
                    BTreeMap::from([(reference.clone(), pointer.to_owned())]),
                )
                .map_err(|error| format!("the token pointer `{pointer}`: {error}"))?,
            ),
        };
        Ok(Self {
            reference,
            resolver,
            presentation: Presentation::Subscription,
        })
    }
}

fn reference(name: &str) -> Result<SecretRef, String> {
    SecretRef::new(name).map_err(|error| error.to_string())
}

/// A key held in memory, answered unchanged on every resolve.
struct HeldKey(Secret);

impl SecretResolver for HeldKey {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(self.0.expose().to_vec())?,
                version: SecretVersion::new("held".to_owned())?,
            })
        })
    }
}

/// The resolver an anonymous binding is given. `llm` never asks it: an anonymous account names
/// no reference.
struct NoSecret;

impl SecretResolver for NoSecret {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// What a client for one endpoint is built from.
pub struct Target<'a> {
    pub protocol: Protocol,
    pub base_url: &'a str,
    pub model: &'a str,
    pub context_window: u64,
    pub max_output_tokens: Option<u64>,
}

/// A blocking [`ModelPort`] over an `llm` client for `target`, authenticated by `source` or not.
///
/// # Errors
/// Names what refused: the endpoint, the model, the bound, or a credential this build cannot
/// present on that wire.
pub fn client(
    target: &Target<'_>,
    source: Option<Source>,
    cancel: &Cancel,
) -> Result<Box<dyn ModelPort>, String> {
    let binding = binding(target, source.as_ref())?;
    let http = HttpClient::with_connect_timeout(LIMITS, CONNECT).map_err(|error| error.message)?;
    let resolver: Arc<dyn SecretResolver> = source.map_or_else(
        || Arc::new(NoSecret) as Arc<dyn SecretResolver>,
        |source| source.resolver,
    );
    let (model, wire): (Arc<dyn Model>, &str) = match target.protocol {
        Protocol::Messages => (
            Arc::new(
                llm_messages::MessagesClient::new(binding, http, resolver)
                    .map_err(|error| error.message)?,
            ),
            MESSAGES_WIRE,
        ),
        Protocol::Responses => {
            let conversation = llm_responses::Conversation::new(id(&new_session())?)
                .with_originator(id(ORIGINATOR)?);
            (
                Arc::new(
                    llm_responses::ResponsesClient::new(binding, http, resolver)
                        .map_err(|error| error.message)?
                        .with_conversation(conversation),
                ),
                RESPONSES_WIRE,
            )
        }
        Protocol::ChatCompletions => {
            return Err("this build speaks no chat-completions wire".to_owned());
        }
    };
    Ok(Box::new(LlmPort {
        blocking: BlockingModel::new(model).map_err(|error| error.message)?,
        wire: WireId::new(wire).map_err(|error| error.to_string())?,
        model: target.model.to_owned(),
        cancel: cancel.clone(),
    }))
}

/// A name for one conversation, stable for the life of a client and distinct between runs, as
/// `harness-responses` minted it.
fn new_session() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("b10x-{:x}-{:x}", std::process::id(), nanos)
}

fn id(value: &str) -> Result<Id, String> {
    Id::new(value).map_err(|error| format!("`{value}`: {error}"))
}

/// A single-target binding for the endpoint the command line named.
fn binding(target: &Target<'_>, source: Option<&Source>) -> Result<Binding, String> {
    const LOCAL: &str = "harness";
    let presentation = source.map(|source| source.presentation);
    let (auth_kind, api_key_header) = match (target.protocol, presentation) {
        (_, None) => (AuthKind::Anonymous, None),
        (Protocol::Messages, Some(Presentation::ApiKey)) => (
            AuthKind::ApiKey,
            Some(ApiKeyHeader::new("x-api-key").map_err(|error| error.message)?),
        ),
        // `llm` adds the Messages route's own subscription presentation to this kind — the OAuth
        // beta header and the client preamble — and serves it over Messages only; on Responses a
        // subscription token is the plain bearer that route was measured with.
        (Protocol::Messages, Some(Presentation::Subscription)) => {
            (AuthKind::SubscriptionOauth, None)
        }
        (_, Some(_)) => (AuthKind::Bearer, None),
    };
    let billing_kind = match presentation {
        Some(Presentation::Subscription) => BillingKind::Subscription,
        _ => BillingKind::Metered,
    };
    let max_output_tokens = match target.protocol {
        Protocol::Messages => target
            .max_output_tokens
            .unwrap_or(DEFAULT_MESSAGES_MAX_OUTPUT_TOKENS),
        _ => target.context_window,
    };
    let capabilities = Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: REASONING_EFFORTS
            .iter()
            .map(|&effort| effort.to_owned())
            .collect(),
        ..Capabilities::text(
            target.context_window,
            max_output_tokens.min(target.context_window),
        )
    };
    let model = id(target.model)?;
    BindingDocument::new(
        Provider {
            id: id(LOCAL)?,
            category: id(LOCAL)?,
        },
        Account {
            id: id(LOCAL)?,
            provider_id: id(LOCAL)?,
            auth_kind,
            billing_kind,
            secret_reference_id: source.map(|source| source.reference.clone()),
            api_key_header,
        },
        Endpoint {
            id: id(LOCAL)?,
            account_id: id(LOCAL)?,
            base_url: BaseUrl::new(target.base_url).map_err(|error| {
                format!("the base URL `{}`: {}", target.base_url, error.message)
            })?,
        },
        ServedModel {
            id: model.clone(),
            upstream_name: model.clone(),
        },
        ServingModel {
            id: model.clone(),
            endpoint_id: id(LOCAL)?,
            model_id: model,
            protocol: target.protocol,
            capabilities,
        },
    )
    .bind()
    .map_err(|error| error.message)
}

/// Renews a Codex login at `path` when its access token is within `margin` of expiry.
///
/// Returns the new token's expiry and whether the refresh token rotated, or [`None`] when nothing
/// was due (or the expiry could not be read, which `llm` leaves alone as Harness did).
///
/// # Errors
/// `llm`'s refusal, which names the file and never a token.
pub fn renew_codex_login(
    path: &Path,
    token_endpoint: &str,
    client_id: &str,
    margin: Duration,
) -> Result<Option<(Option<u64>, bool)>, String> {
    use llm_credentials::codex::{CodexAuthFile, CodexRenewal, Renewal};
    let absolute: PathBuf = std::path::absolute(path).map_err(|error| error.to_string())?;
    let login = CodexAuthFile::new(reference("codex-login")?, absolute);
    let renewal = CodexRenewal::new()
        .map_err(|error| error.message)?
        .with_endpoint(token_endpoint, client_id)
        .with_margin(margin);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let outcome = runtime
        .block_on(login.renew(&renewal, &llm_core::Cancel::new()))
        .map_err(|error| error.to_string())?;
    Ok(match outcome {
        Renewal::NotDue | Renewal::Undated => None,
        Renewal::Renewed(renewed) => Some((
            renewed
                .expires_unix
                .and_then(|expires| u64::try_from(expires).ok()),
            renewed.refresh_token_rotated,
        )),
    })
}

/// A blocking turn over one `llm` client.
struct LlmPort {
    blocking: BlockingModel,
    wire: WireId,
    /// The model the run named, which `Usage::model` falls back to when the provider names none.
    model: String,
    cancel: Cancel,
}

impl ModelPort for LlmPort {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        if self.cancel.is_cancelled() {
            return Err(WireError::cancelled());
        }
        request.validate()?;
        request.check_opaque_items(&self.wire)?;
        let request = to_llm_request(request, self.blocking.provenance())?;
        let token = llm_core::Cancel::new();
        let (sender, receiver) = mpsc::channel();
        let (blocking, request_ref, token_ref) = (&self.blocking, &request, &token);
        let result = std::thread::scope(|scope| {
            // The turn runs on a worker so this thread can hand each event to Harness's sink as
            // it arrives (that sink is not `Send`) and turn Harness's flag into `llm`'s token.
            let worker = scope.spawn(move || {
                let mut forward = move |event: llm_core::StreamEvent| {
                    sender.send(event).map_err(|_| llm_core::Error::cancelled())
                };
                blocking.turn(request_ref, &mut forward, token_ref)
            });
            // A translation failure ends the turn like a sink refusal: the worker is told to stop,
            // and nothing after it reaches the sink.
            let mut failure = None;
            loop {
                match receiver.recv_timeout(CANCEL_POLL) {
                    Ok(event) => match from_llm_event(event) {
                        Ok(Some(event)) if failure.is_none() => sink.emit(event),
                        Ok(_) => {}
                        Err(error) => {
                            token_ref.cancel();
                            failure.get_or_insert(error);
                        }
                    },
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if self.cancel.is_cancelled() {
                    token_ref.cancel();
                }
            }
            let result = worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            match failure {
                Some(error) => Err(error),
                None => result.map_err(from_llm_error),
            }
        })?;
        warn_about_dropped_usage(&result.observation, &self.model, sink);
        from_llm_outcome(result, &self.wire, &self.model)
    }

    fn fork(&self) -> Option<Box<dyn ModelPort + Send + '_>> {
        Some(Box::new(Self {
            blocking: self.blocking.fork(),
            wire: self.wire.clone(),
            model: self.model.clone(),
            cancel: self.cancel.clone(),
        }))
    }
}

fn call_id(value: &str) -> Result<llm_core::CallId, WireError> {
    llm_core::CallId::new(value).map_err(|error| WireError::protocol(error.to_string()))
}

fn tool_name(value: &str) -> Result<llm_core::ToolName, WireError> {
    llm_core::ToolName::new(value).map_err(|error| WireError::protocol(error.to_string()))
}

/// Harness's request as `llm`'s. Opaque items were checked to be this wire's own, so each is bound
/// to the one binding this client serves.
fn to_llm_request(
    request: &TurnRequest,
    provenance: &Provenance,
) -> Result<llm_core::TurnRequest, WireError> {
    let items = request
        .items
        .iter()
        .map(|item| {
            Ok(match item {
                Item::UserText { text } => llm_core::Item::UserText { text: text.clone() },
                Item::AssistantText { text } => {
                    llm_core::Item::AssistantText { text: text.clone() }
                }
                Item::ToolCall(call) => llm_core::Item::ToolCall(llm_core::ToolCall {
                    call_id: call_id(call.call_id.as_str())?,
                    name: tool_name(call.name.as_str())?,
                    arguments: call.arguments.clone(),
                }),
                Item::ToolResult {
                    call_id: id,
                    output,
                    failed,
                } => llm_core::Item::ToolResult {
                    call_id: call_id(id.as_str())?,
                    output: output.clone(),
                    failed: *failed,
                },
                Item::Opaque { payload, .. } => llm_core::Item::Opaque {
                    provenance: provenance.clone(),
                    payload: payload.clone(),
                },
            })
        })
        .collect::<Result<Vec<_>, WireError>>()?;
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            Ok(llm_core::ToolSpec {
                name: tool_name(tool.name.as_str())?,
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
            })
        })
        .collect::<Result<Vec<_>, WireError>>()?;
    Ok(llm_core::TurnRequest {
        model: request.model.clone(),
        instructions: request.instructions.clone(),
        items,
        tools,
        max_output_tokens: request.max_output_tokens,
        sampling: llm_core::Sampling {
            temperature: request.sampling.temperature,
            top_p: request.sampling.top_p,
            reasoning_effort: request.sampling.reasoning_effort.clone(),
        },
        tool_choice: match &request.tool_choice {
            harness_wire::ToolChoice::Auto => llm_core::ToolChoice::Auto,
            harness_wire::ToolChoice::Required => llm_core::ToolChoice::Required,
            harness_wire::ToolChoice::Named(name) => {
                llm_core::ToolChoice::Named(tool_name(name.as_str())?)
            }
        },
        // Harness's turn carries no thinking, parallel-tool-call or hint control to map; the
        // empty value is the pre-0.7.0 turn exactly.
        controls: llm_core::Controls::default(),
    })
}

/// `llm`'s event as Harness's. `ToolCallStarted` has no Harness counterpart; the call it announces
/// arrives in the outcome, so nothing the loop reads is lost.
fn from_llm_event(event: llm_core::StreamEvent) -> Result<Option<StreamEvent>, WireError> {
    Ok(match event {
        llm_core::StreamEvent::TextDelta { text } => Some(StreamEvent::TextDelta { text }),
        llm_core::StreamEvent::ReasoningDelta { text } => {
            Some(StreamEvent::ReasoningDelta { text })
        }
        llm_core::StreamEvent::ToolArgumentsDelta { call_id, delta } => {
            Some(StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new(call_id.as_str())
                    .map_err(|error| WireError::protocol(error.to_string()))?,
                delta,
            })
        }
        llm_core::StreamEvent::Warning { code, message } => {
            Some(StreamEvent::Warning { code, message })
        }
        llm_core::StreamEvent::ToolCallStarted { .. } => None,
    })
}

fn from_llm_outcome(
    outcome: llm_core::TurnOutcome,
    wire: &WireId,
    model: &str,
) -> Result<TurnOutcome, WireError> {
    let items = outcome
        .items
        .into_iter()
        .map(|item| {
            Ok(match item {
                llm_core::Item::UserText { text } => Item::UserText { text },
                llm_core::Item::AssistantText { text } => Item::AssistantText { text },
                llm_core::Item::ToolCall(call) => Item::ToolCall(ToolCall {
                    call_id: CallId::new(call.call_id.as_str())
                        .map_err(|error| WireError::protocol(error.to_string()))?,
                    name: ToolName::new(call.name.as_str())
                        .map_err(|error| WireError::protocol(error.to_string()))?,
                    arguments: call.arguments,
                }),
                llm_core::Item::ToolResult {
                    call_id,
                    output,
                    failed,
                } => Item::ToolResult {
                    call_id: CallId::new(call_id.as_str())
                        .map_err(|error| WireError::protocol(error.to_string()))?,
                    output,
                    failed,
                },
                llm_core::Item::Opaque { payload, .. } => Item::Opaque {
                    wire: wire.clone(),
                    payload,
                },
                llm_core::Item::UnattributedOpaque { .. } => {
                    return Err(WireError::protocol(
                        "model output carries opaque state no binding produced",
                    ));
                }
            })
        })
        .collect::<Result<Vec<_>, WireError>>()?;
    Ok(TurnOutcome {
        stop_reason: match outcome.stop_reason {
            llm_core::StopReason::EndTurn => StopReason::EndTurn,
            llm_core::StopReason::ToolCalls => StopReason::ToolCalls,
            llm_core::StopReason::MaxOutputTokens => StopReason::MaxOutputTokens,
            llm_core::StopReason::Incomplete { reason } => StopReason::Incomplete { reason },
        },
        items,
        usage: usage(&outcome.observation, model),
    })
}

/// Harness's `Usage` needs the input and output totals and a model name. Absent totals leave the
/// usage absent rather than zero (`AGENTS.md` invariant 7); an absent cache read counts zero and
/// an absent model name is the run's own, as both Harness wires did.
fn usage(observation: &llm_core::TurnObservation, model: &str) -> Option<Usage> {
    let reported = observation.usage.as_ref()?;
    Some(Usage {
        model: observation
            .upstream_model
            .as_ref()
            .map_or_else(|| model.to_owned(), |name| name.as_str().to_owned()),
        input_tokens: reported.input_tokens?,
        output_tokens: reported.output_tokens?,
        cached_input_tokens: reported.cached_input_tokens.unwrap_or(0),
        cache_creation_input_tokens: reported.cache_creation_input_tokens,
    })
}

/// Invariant 7 for a figure Harness cannot file: `harness_wire::Usage` needs both totals, so a
/// turn whose inclusive input total `llm` leaves unknown (llm parity row M25) files no usage. The
/// parts the route did report are named instead of dropped silently.
fn warn_about_dropped_usage(
    observation: &llm_core::TurnObservation,
    model: &str,
    sink: &mut dyn StreamSink,
) {
    let Some(reported) = observation.usage.as_ref() else {
        return;
    };
    if usage(observation, model).is_some() {
        return;
    }
    let counters = [
        ("input_tokens", reported.input_tokens),
        ("output_tokens", reported.output_tokens),
        ("cached_input_tokens", reported.cached_input_tokens),
        (
            "cache_creation_input_tokens",
            reported.cache_creation_input_tokens,
        ),
    ];
    let known: Vec<String> = counters
        .into_iter()
        .filter_map(|(name, count)| count.map(|count| format!("{name} {count}")))
        .collect();
    let unknown: Vec<&str> = counters
        .into_iter()
        .filter(|(_, count)| count.is_none())
        .map(|(name, _)| name)
        .collect();
    sink.emit(StreamEvent::Warning {
        code: "usage-incomplete".to_owned(),
        message: format!(
            "the route reported {} and left {} unknown, so this turn files no usage",
            known.join(", "),
            unknown.join(", ")
        ),
    });
}

/// `llm`'s failure as Harness's. The retry class is `llm`'s own decision ([`llm_core::Error::may_retry`]);
/// the loop retries on it exactly as it retried on Harness's wires.
fn from_llm_error(error: llm_core::Error) -> WireError {
    let retriable = error.may_retry();
    let code = match error.code {
        ErrorCode::InvalidRequest | ErrorCode::Protocol => WireErrorCode::Protocol,
        // Before dispatch the only thing that can be unavailable is the credential source, which
        // is the operator's to fix and not a network to wait for. The arm keys on `Dispatch`, and
        // the assumption behind it is read off llm 0.7.0's producers of `Unavailable`:
        // `llm-providers/src/auth.rs:120` (`secret_error`) builds it with `Error::new`, so
        // `NotSent`; the decoders (`llm-messages/src/decode.rs:716`,
        // `llm-responses/src/stream.rs:462-466`) attach `Accepted`; `llm-http` builds `Transport`
        // and `Deadline`, never `Unavailable`; `llm-blocking/src/lib.rs:71` builds one in
        // `BlockingModel::new`, mapped at client construction and never reaching this function.
        // A future pre-dispatch `Unavailable` from a transport (a connect failure classed that
        // way) would need its own class rather than this arm.
        ErrorCode::Unavailable if error.dispatch == llm_core::Dispatch::NotSent => {
            WireErrorCode::Unauthorized
        }
        ErrorCode::Transport | ErrorCode::Deadline | ErrorCode::Unavailable => {
            WireErrorCode::Transport
        }
        ErrorCode::Unauthorized => WireErrorCode::Unauthorized,
        ErrorCode::RateLimited => WireErrorCode::RateLimited,
        ErrorCode::Refused => WireErrorCode::Refused,
        ErrorCode::TooLarge => WireErrorCode::TooLarge,
        ErrorCode::Unsupported => WireErrorCode::Unsupported,
        ErrorCode::Cancelled => WireErrorCode::Cancelled,
    };
    WireError::new(code, error.message, retriable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(protocol: Protocol) -> Target<'static> {
        Target {
            protocol,
            base_url: "http://127.0.0.1:1/v1",
            model: "m",
            context_window: 1000,
            max_output_tokens: None,
        }
    }

    #[test]
    fn unreported_totals_leave_usage_absent_rather_than_zero() {
        let provenance = binding(&target(Protocol::Responses), None)
            .expect("binds")
            .provenance()
            .clone();
        let mut observation = llm_core::TurnObservation::new(provenance);
        assert_eq!(usage(&observation, "m"), None);
        observation.usage = Some(llm_core::Usage {
            input_tokens: Some(3),
            ..llm_core::Usage::default()
        });
        assert_eq!(
            usage(&observation, "m"),
            None,
            "no output total was reported"
        );
        observation.usage = Some(llm_core::Usage {
            input_tokens: Some(3),
            output_tokens: Some(2),
            ..llm_core::Usage::default()
        });
        let usage = usage(&observation, "m").expect("both totals reported");
        assert_eq!(
            (
                usage.model.as_str(),
                usage.cached_input_tokens,
                usage.cache_creation_input_tokens
            ),
            ("m", 0, None)
        );
    }

    #[test]
    fn a_subscription_token_on_the_messages_wire_is_llms_subscription_oauth_account() {
        // llm 0.7.0 presents `AuthKind::SubscriptionOauth` on Messages — a bearer plus the OAuth
        // beta header and the client preamble, billed as a subscription — where the spike bridge
        // refused the token (story:harness-builds-on-llm-0-7).
        let source = Source::file(Path::new("/named/by/the/caller"), None).expect("a source");
        let binding = binding(&target(Protocol::Messages), Some(&source)).expect("binds");
        let account = &binding.declaration().account;
        assert_eq!(account.auth_kind, AuthKind::SubscriptionOauth);
        assert_eq!(account.billing_kind, BillingKind::Subscription);
        assert!(account.api_key_header.is_none());
    }

    #[test]
    fn a_subscription_token_on_the_responses_wire_stays_a_plain_bearer() {
        // llm 0.7.0 serves `SubscriptionOauth` over Messages only (`llm-providers` `bind()`), so
        // the Codex route keeps what was measured on it: a plain bearer, billed as a subscription.
        let source = Source::environment("TOKEN", None).expect("a source");
        let binding = binding(&target(Protocol::Responses), Some(&source)).expect("binds");
        let account = &binding.declaration().account;
        assert_eq!(account.auth_kind, AuthKind::Bearer);
        assert_eq!(account.billing_kind, BillingKind::Subscription);
    }

    #[test]
    fn the_binding_declares_the_max_effort_the_command_line_accepts() {
        // `--reasoning-effort` is a free string, so `max` reaches this port, and llm refuses an
        // effort the binding does not declare. 0.7.0 sends `max` as it is on Messages and as the
        // highest Responses effort, `xhigh` (`llm_core::controls::wire_effort`).
        let binding = binding(&target(Protocol::Messages), None).expect("binds");
        let efforts = &binding.declaration().serving.capabilities.reasoning_efforts;
        assert!(efforts.iter().any(|effort| effort == "max"), "{efforts:?}");
    }

    #[test]
    fn a_credential_source_refusal_is_a_credential_problem_not_a_transport_failure() {
        // `llm-providers` reports a source it will not read (`secret_error`: unsafe, unavailable)
        // as `Unavailable` built with `Error::new`, so never dispatched; every `Unavailable` the
        // transport or a decoder produces was dispatched (`Dispatch::Unknown`/`Accepted`). Before
        // dispatch the only thing that can be unavailable is the credential, which is the
        // operator's to fix (story:harness-builds-on-llm-0-7, correction round 1).
        let refused = llm_core::Error::new(
            ErrorCode::Unavailable,
            "secret reference `oauth-token-file` refused: secret source does not satisfy the \
             required access protections",
        );
        let wire = from_llm_error(refused);
        assert_eq!(wire.code, WireErrorCode::Unauthorized, "{wire:?}");
        assert!(!wire.retriable);
        let upstream = llm_core::Error::new(ErrorCode::Unavailable, "server_error")
            .with_dispatch(llm_core::Dispatch::Accepted);
        assert_eq!(from_llm_error(upstream).code, WireErrorCode::Transport);
    }

    #[test]
    fn a_retry_class_is_llms_own_decision() {
        let cut = llm_core::Error::new(ErrorCode::Transport, "cut").with_retriable(true);
        assert!(from_llm_error(cut).retriable);
        let refused = llm_core::Error::new(ErrorCode::Unauthorized, "no").with_retriable(true);
        let refused = from_llm_error(refused);
        assert!(!refused.retriable);
        assert_eq!(refused.code, WireErrorCode::Unauthorized);
    }

    // Adversary pass 1 (story:harness-builds-on-llm-0-7).

    #[test]
    fn an_api_key_is_never_a_subscription_account_on_either_wire() {
        // The mutant this pins: a `billing_kind`/`auth_kind` match that answered the subscription
        // presentation for every source would still pass the two subscription cases above.
        let key = Source::key("k").expect("a source");
        let messages = binding(&target(Protocol::Messages), Some(&key)).expect("binds");
        let account = &messages.declaration().account;
        assert_eq!(account.auth_kind, AuthKind::ApiKey);
        assert_eq!(account.billing_kind, BillingKind::Metered);
        assert_eq!(
            account
                .api_key_header
                .as_ref()
                .map(|h| h.as_str().to_owned()),
            Some("x-api-key".to_owned())
        );
        let responses = binding(&target(Protocol::Responses), Some(&key)).expect("binds");
        let account = &responses.declaration().account;
        assert_eq!(account.auth_kind, AuthKind::Bearer);
        assert_eq!(account.billing_kind, BillingKind::Metered);
        assert!(account.api_key_header.is_none());
    }

    // Adversary pass 2 (story:harness-builds-on-llm-0-7), against 36a587c5; rewritten in
    // correction round 2 to the rule AGENTS.md § Safety envelope *Credentials* now states.

    #[test]
    fn a_source_refusal_never_names_a_value_and_at_construction_names_what_the_operator_passed() {
        // AGENTS.md § Safety envelope, *Credentials*: a refusal never names the value; the
        // per-request refusal `llm` builds names only the reference (`oauth-token-file`), and a
        // refusal at construction — a variable name `llm` will not accept, a path it will not
        // read — may name the path or variable the operator passed, which is the operator's own
        // input and not a secret.
        let error = Source::environment("NOT=VALID", None).expect_err("llm refuses `=` in a name");
        assert!(
            error.contains("NOT=VALID"),
            "the operator's own input is named: {error}"
        );
        let oversize = "k".repeat(llm_credentials::MAX_SECRET_BYTES + 1);
        let error = Source::key(&oversize).expect_err("llm refuses a key past its bound");
        assert!(!error.contains(&oversize[..64]), "never the value: {error}");
    }

    #[test]
    fn an_in_stream_provider_failure_before_any_output_is_offered_for_another_attempt() {
        // The classes the former wires retried on (`harness-responses` `response.failed` with
        // `server_error`, `harness-messages` `error` `api_error`/`overloaded_error`) arrive from
        // llm 0.7.0 as `Unavailable`/`Transport`/`RateLimited` with `retriable` unset (llm parity
        // rows R28, M27), so this port hands the loop a final failure where it used to hand it a
        // retriable one. Each error here is built exactly as llm 0.7.0's decoders build it.
        for (code, what) in [
            (
                ErrorCode::Unavailable,
                "Responses `server_error`, Messages `overloaded_error`",
            ),
            (ErrorCode::RateLimited, "Responses `rate_limit_exceeded`"),
            (ErrorCode::Transport, "Messages `api_error`"),
        ] {
            let error = llm_core::Error::new(code, "not this time")
                .with_dispatch(llm_core::Dispatch::Accepted);
            let wire = from_llm_error(error);
            assert!(
                wire.retriable,
                "{what}: the far side's own failure before any output is worth another attempt \
                 (the loop retries on this flag and nothing else): {wire:?}"
            );
        }
    }
}
