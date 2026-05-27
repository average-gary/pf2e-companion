//! LLM provider trait + dispatch.
//!
//! Phase 6 § B: provider-agnostic abstraction with two implementations
//! (Anthropic, Ollama). Stage B ships the trait, types, the registry, and
//! both impls. Tool-use loop + RAG composition layer in Stages C/D.
//!
//! Design notes:
//! - **Off by default.** No provider is constructed at app startup. The
//!   user opts in via `/settings/llm`; `commands::llm_configure` builds a
//!   provider and stashes it in `LlmRegistry` (locked behind a `RwLock`
//!   for swap-on-reconfigure semantics).
//! - **BYO key.** Keys live in the OS keychain via `keystore.rs`, never on
//!   disk in plaintext.
//! - **Tauri events for streaming.** `chat()` returns a stream the IPC
//!   handler forwards as `llm:token` events the SvelteKit chat page
//!   subscribes to.

use anyhow::Result;
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub mod anthropic;
pub mod ollama;

/// Single message in a chat conversation. Mirrors the Anthropic /
/// OpenAI-compatible shape; Ollama accepts the same.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatOpts {
    /// Soft cap on tokens to generate (provider-specific defaults if None).
    pub max_tokens: Option<u32>,
    /// 0.0-1.0; provider default when None.
    pub temperature: Option<f32>,
    /// Anthropic-only: a `system` prompt. Ollama folds this into
    /// `messages` upstream.
    pub system: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatChunk {
    /// A piece of assistant text. Concatenate to form the reply.
    Text(String),
    /// The model has stopped emitting tokens. Optionally carries a usage
    /// summary; providers report this differently.
    End {
        usage: Option<UsageSummary>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSummary {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

pub type ChatStream = BoxStream<'static, Result<ChatChunk>>;

/// The minimal contract every LLM provider must satisfy.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn model(&self) -> &str;
    async fn chat(&self, messages: Vec<Message>, opts: ChatOpts) -> Result<ChatStream>;
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProviderKind {
    Anthropic,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProviderKind,
    pub model: String,
    /// Required for Ollama (e.g. <http://localhost:11434>); ignored for
    /// Anthropic (which always points at api.anthropic.com).
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmStatus {
    pub configured: bool,
    pub provider: Option<LlmProviderKind>,
    pub model: Option<String>,
    /// True iff a key is present in the keystore for the configured
    /// provider. Anthropic requires a key; Ollama does not.
    pub key_present: bool,
}

/// App-state holder for the active provider. Wrapped in an `RwLock` so
/// reconfiguration is safe across concurrent IPC calls.
pub struct LlmRegistry {
    inner: RwLock<Option<ActiveProvider>>,
}

pub struct ActiveProvider {
    config: LlmConfig,
    provider: Box<dyn LlmProvider>,
}

impl Default for LlmRegistry {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl LlmRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn configure(&self, config: LlmConfig, api_key: Option<String>) -> Result<()> {
        let provider: Box<dyn LlmProvider> = match config.provider {
            LlmProviderKind::Anthropic => Box::new(anthropic::AnthropicProvider::new(
                config.model.clone(),
                api_key.ok_or_else(|| anyhow::anyhow!("anthropic provider requires an api key"))?,
            )?),
            LlmProviderKind::Ollama => Box::new(ollama::OllamaProvider::new(
                config.model.clone(),
                config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string()),
            )?),
        };
        let mut guard = self.inner.write().await;
        *guard = Some(ActiveProvider { config, provider });
        Ok(())
    }

    pub async fn clear(&self) {
        let mut g = self.inner.write().await;
        *g = None;
    }

    pub async fn status(&self, key_present: bool) -> LlmStatus {
        let guard = self.inner.read().await;
        match guard.as_ref() {
            Some(active) => LlmStatus {
                configured: true,
                provider: Some(active.config.provider),
                model: Some(active.config.model.clone()),
                key_present,
            },
            None => LlmStatus {
                configured: false,
                provider: None,
                model: None,
                key_present,
            },
        }
    }

    /// Hand the caller a shared reference to the active provider for the
    /// duration of the read guard. The guard is `Send`, so this works inside
    /// `tokio::spawn`. Caller must drop the guard before returning to a
    /// long-lived future to avoid blocking writers.
    pub async fn read(
        &self,
    ) -> tokio::sync::RwLockReadGuard<'_, Option<ActiveProvider>> {
        self.inner.read().await
    }

    pub async fn current_kind(&self) -> Option<LlmProviderKind> {
        self.inner
            .read()
            .await
            .as_ref()
            .map(|a| a.config.provider)
    }

    /// Inject a custom provider directly. Used by integration tests to
    /// exercise the embed/chat plumbing without making real HTTP calls.
    /// The associated `LlmConfig` reports whatever kind/model the caller
    /// passes — `current_kind()` will return it verbatim, so tests that
    /// need the Ollama branch in `rag::vector_search` can advertise as
    /// Ollama while being backed by a fake.
    pub async fn install_provider(
        &self,
        config: LlmConfig,
        provider: Box<dyn LlmProvider>,
    ) {
        let mut g = self.inner.write().await;
        *g = Some(ActiveProvider { config, provider });
    }
}

impl ActiveProvider {
    pub fn provider(&self) -> &dyn LlmProvider {
        self.provider.as_ref()
    }
}

// ===== SSE token-buffer parser =================================================
//
// Anthropic's streaming API uses Server-Sent Events; Ollama's uses
// newline-delimited JSON. The Anthropic frame format is:
//
//   event: content_block_delta
//   data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}
//
// We accumulate bytes from the wire, split on `\n\n` (SSE frame boundary),
// and emit one event per frame. The function below is pure so it tests
// cleanly under Vitest *and* Rust unit tests (the frontend has the
// equivalent in TypeScript).

#[derive(Default, Debug)]
pub struct SseBuffer {
    /// Bytes received but not yet split into a full frame.
    accum: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

impl SseBuffer {
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.accum.push_str(chunk);
        let mut out = Vec::new();
        // SSE frames are separated by a blank line: \n\n (LF) or
        // \r\n\r\n (CRLF). We normalize to LF first.
        loop {
            let pos = self.accum.find("\n\n");
            let pos = match pos {
                Some(p) => p,
                None => break,
            };
            let frame = self.accum[..pos].to_string();
            self.accum.drain(..(pos + 2));
            if let Some(ev) = parse_sse_frame(&frame) {
                out.push(ev);
            }
        }
        out
    }
}

fn parse_sse_frame(frame: &str) -> Option<SseEvent> {
    let mut event: Option<String> = None;
    let mut data = String::new();
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
        // ignore comments, retry, id
    }
    if data.is_empty() && event.is_none() {
        return None;
    }
    Some(SseEvent { event, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_buffer_splits_two_complete_frames() {
        let mut b = SseBuffer::default();
        let evs = b.push(
            "event: a\ndata: {\"x\":1}\n\nevent: b\ndata: {\"x\":2}\n\n",
        );
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("a"));
        assert_eq!(evs[0].data, r#"{"x":1}"#);
        assert_eq!(evs[1].event.as_deref(), Some("b"));
        assert_eq!(evs[1].data, r#"{"x":2}"#);
    }

    #[test]
    fn sse_buffer_holds_partial_frame_until_blank_line() {
        let mut b = SseBuffer::default();
        let evs = b.push("event: a\ndata: {\"x\":1");
        assert!(evs.is_empty());
        let evs = b.push("23}\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, r#"{"x":123}"#);
    }

    #[test]
    fn sse_buffer_handles_data_only_frames() {
        let mut b = SseBuffer::default();
        let evs = b.push("data: hello\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].event, None);
        assert_eq!(evs[0].data, "hello");
    }

    #[test]
    fn sse_buffer_ignores_comments_and_blanks() {
        let mut b = SseBuffer::default();
        let evs = b.push(": this is a comment\nretry: 5000\n\n");
        // comment-only frames produce nothing — both event and data empty
        assert!(evs.is_empty());
    }

    #[test]
    fn sse_buffer_multi_line_data() {
        let mut b = SseBuffer::default();
        let evs = b.push("data: line one\ndata: line two\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "line one\nline two");
    }
}
