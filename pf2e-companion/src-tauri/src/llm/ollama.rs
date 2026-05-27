//! Ollama HTTP client.
//!
//! Stream via newline-delimited JSON on `POST /api/chat`. Embeddings via
//! `POST /api/embeddings`. No API key required; assumes the server runs
//! locally (or on the user-supplied `base_url`).
//!
//! Reference: <https://github.com/ollama/ollama/blob/main/docs/api.md>

use crate::llm::{ChatChunk, ChatOpts, ChatStream, LlmProvider, Message, Role, UsageSummary};
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
            all_messages.push(Message {
                role: Role::System,
                content: sys,
            });
        }
        all_messages.extend(messages);

        let body = json!({
            "model": self.model,
            "stream": true,
            "options": {
                "temperature": opts.temperature.unwrap_or(0.7),
                "num_predict": opts.max_tokens.unwrap_or(2048),
            },
            "messages": all_messages.iter().map(|m| json!({
                "role": match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                },
                "content": m.content,
            })).collect::<Vec<_>>(),
        });

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

        // Ollama streams newline-delimited JSON; one object per line.
        let byte_stream = resp.bytes_stream();
        let mut accum = String::new();
        let chunk_stream = byte_stream.flat_map(move |bytes_result| {
            let chunk = match bytes_result {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) => return stream::iter(vec![Err(anyhow!("ollama stream: {e}"))]),
            };
            accum.push_str(&chunk);

            let mut out = Vec::<Result<ChatChunk>>::new();
            // Split on newline; the last element may be partial — preserve it.
            while let Some(idx) = accum.find('\n') {
                let line = accum[..idx].trim().to_string();
                accum.drain(..(idx + 1));
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<OllamaChatResponse>(&line) {
                    Ok(resp) => {
                        if let Some(msg) = resp.message {
                            if !msg.content.is_empty() {
                                out.push(Ok(ChatChunk::Text(msg.content)));
                            }
                        }
                        if resp.done {
                            out.push(Ok(ChatChunk::End {
                                usage: Some(UsageSummary {
                                    input_tokens: resp.prompt_eval_count,
                                    output_tokens: resp.eval_count,
                                }),
                            }));
                        }
                    }
                    Err(e) => out.push(Err(anyhow!("ollama decode `{line}`: {e}"))),
                }
            }
            stream::iter(out)
        });

        Ok(chunk_stream.boxed())
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
    #[allow(dead_code)] // reserved for tool-use phase
    role: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}
