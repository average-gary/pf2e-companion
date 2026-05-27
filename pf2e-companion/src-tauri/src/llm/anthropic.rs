//! Anthropic Messages API client.
//!
//! Streaming via the SSE `messages` endpoint with `stream: true`. Embedding
//! is **not natively supported by Anthropic** — this impl returns an error
//! from `embed()`. RAG users should run Ollama (or another embedding
//! provider) for the corpus and Anthropic for chat.
//!
//! References:
//! - <https://docs.anthropic.com/en/api/messages>
//! - <https://docs.anthropic.com/en/api/messages-streaming>

use crate::llm::{
    ChatChunk, ChatOpts, ChatStream, LlmProvider, Message, Role, SseBuffer, ToolCall,
    UsageSummary,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn new(model: String, api_key: String) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("anthropic api key is empty"));
        }
        let client = Client::builder()
            .user_agent(concat!("pf2e-companion/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("building reqwest client")?;
        Ok(Self {
            client,
            api_key,
            model,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(&self, messages: Vec<Message>, opts: ChatOpts) -> Result<ChatStream> {
        // Anthropic forbids `system` as a Role::System message inside `messages`.
        // Promote it to the top-level `system` field.
        let mut sys = opts.system.clone();
        let mut filtered: Vec<&Message> = Vec::with_capacity(messages.len());
        for m in &messages {
            if m.role == Role::System {
                if sys.is_some() {
                    sys = Some(format!(
                        "{}\n\n{}",
                        sys.as_deref().unwrap_or(""),
                        &m.content
                    ));
                } else {
                    sys = Some(m.content.clone());
                }
            } else {
                filtered.push(m);
            }
        }

        // System block: plain string for naked chat, content-block array
        // when prompt caching is on (cache_control rides on a content block).
        let system_field: serde_json::Value = match (&sys, opts.cache_system) {
            (Some(s), true) => json!([{
                "type": "text",
                "text": s,
                "cache_control": { "type": "ephemeral" },
            }]),
            (Some(s), false) => json!(s),
            (None, _) => serde_json::Value::Null,
        };

        // Translate our flat (role, content, tool_*) messages into
        // Anthropic's content-block array. An assistant turn that issued
        // tool calls becomes [text + tool_use blocks]; a tool-role
        // message becomes a user turn containing tool_result blocks.
        let messages_field: Vec<serde_json::Value> = filtered
            .iter()
            .map(|m| message_to_anthropic_block(m))
            .collect();

        let mut body = json!({
            "model": self.model,
            "max_tokens": opts.max_tokens.unwrap_or(2048),
            "temperature": opts.temperature.unwrap_or(0.7),
            "stream": true,
            "messages": messages_field,
        });
        if !system_field.is_null() {
            body["system"] = system_field;
        }
        if !opts.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(
                opts.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "input_schema": t.input_schema,
                        })
                    })
                    .collect(),
            );
        }

        let resp = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("anthropic POST /v1/messages")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("anthropic {status}: {body}"));
        }

        // Stream response → SSE frames → ChatChunk events. We need to
        // accumulate `input_json_delta` fragments per content-block index
        // so we can emit a complete `ToolCall` once the block stops.
        let byte_stream = resp.bytes_stream();
        let mut buf = SseBuffer::default();
        let mut tool_buf: HashMap<u32, PartialToolCall> = HashMap::new();
        let chunk_stream = byte_stream.flat_map(move |bytes_result| {
            let chunk = match bytes_result {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) => return stream::iter(vec![Err(anyhow!("anthropic stream: {e}"))]),
            };
            let events = buf.push(&chunk);
            let mut out: Vec<Result<ChatChunk>> = Vec::new();
            for ev in events {
                if let Some(chunk) = decode_event(ev, &mut tool_buf) {
                    out.push(chunk);
                }
            }
            stream::iter(out)
        });

        Ok(chunk_stream.boxed())
    }

    async fn embed(&self, _texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Err(anyhow!(
            "anthropic does not provide native embeddings; configure an Ollama \
             provider for corpus embedding (RAG indexing)"
        ))
    }
}

/// Per-content-block scratchpad while a `tool_use` block is streaming. We
/// learn the id+name in `content_block_start`, accumulate JSON
/// fragments in `input_json_delta`, and emit a finalized `ToolCall` on
/// `content_block_stop`.
#[derive(Debug, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    json_buf: String,
}

/// Decode one SSE event. Tool-call events are stitched across multiple
/// frames using `tool_buf`. Frames that don't carry visible content
/// (`message_start`, `ping`, etc.) yield `None`.
fn decode_event(
    ev: crate::llm::SseEvent,
    tool_buf: &mut HashMap<u32, PartialToolCall>,
) -> Option<Result<ChatChunk>> {
    let event = ev.event.as_deref().unwrap_or("");

    match event {
        "content_block_start" => {
            let parsed: AnthropicEvent = match serde_json::from_str(&ev.data) {
                Ok(v) => v,
                Err(_) => return None,
            };
            if let (Some(idx), Some(block)) = (parsed.index, parsed.content_block) {
                if block.kind.as_deref() == Some("tool_use") {
                    tool_buf.insert(
                        idx,
                        PartialToolCall {
                            id: block.id.unwrap_or_default(),
                            name: block.name.unwrap_or_default(),
                            json_buf: String::new(),
                        },
                    );
                }
            }
            None
        }
        "content_block_delta" => {
            let parsed: AnthropicEvent = match serde_json::from_str(&ev.data) {
                Ok(v) => v,
                Err(e) => return Some(Err(anyhow!("anthropic decode: {e}"))),
            };
            let delta = parsed.delta?;
            // Two delta variants we care about: text_delta (assistant
            // prose) and input_json_delta (streaming tool-call args).
            if let Some(text) = delta.text {
                return Some(Ok(ChatChunk::Text(text)));
            }
            if let Some(partial) = delta.partial_json {
                if let Some(idx) = parsed.index {
                    if let Some(slot) = tool_buf.get_mut(&idx) {
                        slot.json_buf.push_str(&partial);
                    }
                }
            }
            None
        }
        "content_block_stop" => {
            let parsed: AnthropicEvent = match serde_json::from_str(&ev.data) {
                Ok(v) => v,
                Err(_) => return None,
            };
            let idx = parsed.index?;
            let partial = tool_buf.remove(&idx)?;
            // Empty input is legal (zero-arg tool); decode {} when so.
            let input: serde_json::Value = if partial.json_buf.trim().is_empty() {
                json!({})
            } else {
                match serde_json::from_str(&partial.json_buf) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(anyhow!(
                        "anthropic tool_use input json decode (`{}`): {e}",
                        partial.json_buf
                    ))),
                }
            };
            Some(Ok(ChatChunk::ToolCall(ToolCall {
                id: partial.id,
                name: partial.name,
                input,
            })))
        }
        "message_delta" | "message_stop" => {
            let parsed: AnthropicEvent = serde_json::from_str(&ev.data).unwrap_or_default();
            let usage = parsed.usage.map(|u| UsageSummary {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_input_tokens: u.cache_read_input_tokens,
                cache_creation_input_tokens: u.cache_creation_input_tokens,
            });
            if event == "message_stop" {
                Some(Ok(ChatChunk::End { usage }))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Convert one of our `Message` values to the JSON object Anthropic expects.
fn message_to_anthropic_block(m: &Message) -> serde_json::Value {
    match m.role {
        Role::User => {
            json!({ "role": "user", "content": m.content })
        }
        Role::Assistant => {
            // If the assistant turn issued tool calls, emit a content
            // block array combining any text + tool_use blocks. Else a
            // plain string is fine.
            if m.tool_calls.is_empty() {
                json!({ "role": "assistant", "content": m.content })
            } else {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.content }));
                }
                for tc in &m.tool_calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.input,
                    }));
                }
                json!({ "role": "assistant", "content": blocks })
            }
        }
        Role::Tool => {
            // Anthropic encodes tool results as a user turn whose content
            // is one or more `tool_result` blocks.
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content,
                }],
            })
        }
        Role::System => {
            // Should already be hoisted by the caller; if it slipped
            // through, treat it as a user message rather than crashing.
            json!({ "role": "user", "content": m.content })
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicEvent {
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlockMeta>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicContentBlockMeta {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
    /// Streaming JSON fragment for a tool_use block.
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}
