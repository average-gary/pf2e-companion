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

use crate::llm::{ChatChunk, ChatOpts, ChatStream, LlmProvider, Message, Role, SseBuffer, UsageSummary};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

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
                    // multiple system messages: concatenate
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

        let body = json!({
            "model": self.model,
            "max_tokens": opts.max_tokens.unwrap_or(2048),
            "temperature": opts.temperature.unwrap_or(0.7),
            "stream": true,
            "system": sys,
            "messages": filtered.iter().map(|m| json!({
                "role": match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool | Role::System => "user", // Tool replies fold into user-side; system is hoisted above
                },
                "content": m.content,
            })).collect::<Vec<_>>(),
        });

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

        // Stream response → SSE frames → ChatChunk events.
        let byte_stream = resp.bytes_stream();
        let mut buf = SseBuffer::default();
        let chunk_stream = byte_stream
            .flat_map(move |bytes_result| {
                let chunk = match bytes_result {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Err(e) => return stream::iter(vec![Err(anyhow!("anthropic stream: {e}"))]),
                };
                let events = buf.push(&chunk);
                let chunks: Vec<Result<ChatChunk>> =
                    events.into_iter().filter_map(decode_event).collect();
                stream::iter(chunks)
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

/// Decode one SSE event to either a `ChatChunk` or `None` (we ignore frames
/// that don't carry visible content — `message_start`, `content_block_start`,
/// `ping`, etc.).
fn decode_event(ev: crate::llm::SseEvent) -> Option<Result<ChatChunk>> {
    let event = ev.event.as_deref().unwrap_or("");

    match event {
        "content_block_delta" => {
            let parsed: AnthropicEvent = match serde_json::from_str(&ev.data) {
                Ok(v) => v,
                Err(e) => return Some(Err(anyhow!("anthropic decode: {e}"))),
            };
            if let Some(delta) = parsed.delta {
                if let Some(text) = delta.text {
                    return Some(Ok(ChatChunk::Text(text)));
                }
            }
            None
        }
        "message_delta" | "message_stop" => {
            // `message_delta` carries usage in `usage` field; `message_stop`
            // is the stream terminator.
            let parsed: AnthropicEvent = serde_json::from_str(&ev.data).unwrap_or_default();
            let usage = parsed.usage.map(|u| UsageSummary {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
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

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicEvent {
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
}
