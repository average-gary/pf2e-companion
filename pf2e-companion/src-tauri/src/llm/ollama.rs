//! Ollama HTTP client.
//!
//! Stream via newline-delimited JSON on `POST /api/chat`. Embeddings via
//! `POST /api/embeddings`. No API key required; assumes the server runs
//! locally (or on the user-supplied `base_url`).
//!
//! Reference: <https://github.com/ollama/ollama/blob/main/docs/api.md>

use crate::llm::{
    ChatChunk, ChatOpts, ChatStream, LlmProvider, Message, Role, ToolCall, UsageSummary,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(model: String, base_url: String) -> Result<Self> {
        if base_url.is_empty() {
            return Err(anyhow!("ollama base_url is empty"));
        }
        let client = Client::builder()
            .user_agent(concat!("pf2e-companion/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("building reqwest client")?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(&self, messages: Vec<Message>, opts: ChatOpts) -> Result<ChatStream> {
        // Ollama accepts `system` as the first message with role=system.
        let mut all_messages: Vec<Message> = Vec::with_capacity(messages.len() + 1);
        if let Some(sys) = opts.system.clone() {
            all_messages.push(Message::new(Role::System, sys));
        }
        all_messages.extend(messages);

        // Ollama's tool-use protocol requires `stream: false` (the
        // server batches the response) when tools are advertised in the
        // request — streaming + tool_calls together is unstable across
        // model versions. We pick the right knob here.
        let stream_mode = opts.tools.is_empty();

        let mut body = json!({
            "model": self.model,
            "stream": stream_mode,
            "options": {
                "temperature": opts.temperature.unwrap_or(0.7),
                "num_predict": opts.max_tokens.unwrap_or(2048),
            },
            "messages": all_messages.iter().map(message_to_ollama_block).collect::<Vec<_>>(),
        });
        if !opts.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(
                opts.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema,
                            },
                        })
                    })
                    .collect(),
            );
        }

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("ollama POST {}/api/chat", self.base_url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("ollama {status}: {body}"));
        }

        // Two response shapes: streaming NDJSON (one object per line)
        // when `stream: true`, or a single JSON object when tools were
        // advertised. We branch up front; the streaming path is
        // unchanged from the pre-Stage-D code.
        if stream_mode {
            let byte_stream = resp.bytes_stream();
            let mut accum = String::new();
            let chunk_stream = byte_stream.flat_map(move |bytes_result| {
                let chunk = match bytes_result {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Err(e) => return stream::iter(vec![Err(anyhow!("ollama stream: {e}"))]),
                };
                accum.push_str(&chunk);

                let mut out = Vec::<Result<ChatChunk>>::new();
                while let Some(idx) = accum.find('\n') {
                    let line = accum[..idx].trim().to_string();
                    accum.drain(..(idx + 1));
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<OllamaChatResponse>(&line) {
                        Ok(resp) => {
                            for chunk in decode_ollama_response(resp) {
                                out.push(Ok(chunk));
                            }
                        }
                        Err(e) => out.push(Err(anyhow!("ollama decode `{line}`: {e}"))),
                    }
                }
                stream::iter(out)
            });
            Ok(chunk_stream.boxed())
        } else {
            // Single-object response (tools path). Buffer the whole body.
            let bytes = resp.bytes().await.context("ollama tool response body")?;
            let parsed: OllamaChatResponse =
                serde_json::from_slice(&bytes).with_context(|| {
                    format!("ollama tool response decode: {}", String::from_utf8_lossy(&bytes))
                })?;
            let chunks: Vec<Result<ChatChunk>> =
                decode_ollama_response(parsed).into_iter().map(Ok).collect();
            Ok(stream::iter(chunks).boxed())
        }
    }

    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        // Ollama 0.x: POST /api/embeddings with one prompt per call.
        // Newer servers support /api/embed with batched input — we use the
        // legacy single-prompt endpoint for compatibility.
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let body = json!({
                "model": self.model,
                "prompt": text,
            });
            let resp = self
                .client
                .post(format!("{}/api/embeddings", self.base_url))
                .json(&body)
                .send()
                .await
                .with_context(|| format!("ollama POST {}/api/embeddings", self.base_url))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("ollama embed {status}: {body}"));
            }
            let parsed: OllamaEmbedResponse = resp
                .json()
                .await
                .context("decoding ollama /api/embeddings response")?;
            out.push(parsed.embedding);
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaChatResponse {
    #[serde(default)]
    message: Option<OllamaMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaMessage {
    #[serde(default)]
    #[allow(dead_code)] // role isn't read by the trait; kept for round-trip
    role: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaToolCall {
    #[serde(default)]
    function: OllamaToolFunction,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct OllamaToolFunction {
    #[serde(default)]
    name: String,
    /// Ollama returns `arguments` as a JSON object directly (not a
    /// stringified payload like OpenAI). We preserve it verbatim.
    #[serde(default)]
    arguments: serde_json::Value,
}

/// Convert a finalized Ollama response object into our `ChatChunk` events.
fn decode_ollama_response(resp: OllamaChatResponse) -> Vec<ChatChunk> {
    let mut out = Vec::new();
    if let Some(msg) = resp.message {
        if !msg.content.is_empty() {
            out.push(ChatChunk::Text(msg.content));
        }
        for (idx, tc) in msg.tool_calls.into_iter().enumerate() {
            // Ollama doesn't issue stable ids for tool calls; synthesize
            // one so the agent loop can match results back. Combining
            // index + name is sufficient inside a single response.
            let id = format!("ollama_tc_{idx}_{}", tc.function.name);
            out.push(ChatChunk::ToolCall(ToolCall {
                id,
                name: tc.function.name,
                input: tc.function.arguments,
            }));
        }
    }
    if resp.done {
        out.push(ChatChunk::End {
            usage: Some(UsageSummary {
                input_tokens: resp.prompt_eval_count,
                output_tokens: resp.eval_count,
                ..Default::default()
            }),
        });
    }
    out
}

/// Translate one of our `Message` values into the JSON object Ollama
/// expects in its `messages` array.
fn message_to_ollama_block(m: &Message) -> serde_json::Value {
    let role_str = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut obj = json!({
        "role": role_str,
        "content": m.content,
    });
    // Round-trip an assistant turn that issued tool calls.
    if matches!(m.role, Role::Assistant) && !m.tool_calls.is_empty() {
        obj["tool_calls"] = serde_json::Value::Array(
            m.tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "function": {
                            "name": tc.name,
                            "arguments": tc.input,
                        }
                    })
                })
                .collect(),
        );
    }
    obj
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}
