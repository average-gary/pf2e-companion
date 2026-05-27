//! Phase 6 § B — provider abstraction + key onboarding.
//!
//! Validates the LLM scaffolding without making real HTTP calls and without
//! touching the OS keychain (which would prompt on macOS or skip on
//! headless CI).
//!
//! What we cover here:
//! - `LlmRegistry` lifecycle: empty → configured → cleared.
//! - Provider construction (`AnthropicProvider`, `OllamaProvider`) and
//!   their guard rails (empty key / empty base url).
//! - `LlmStatus` reporting matches the registry state.
//! - `SseBuffer` end-to-end: a realistic Anthropic message stream chopped
//!   into byte-level fragments produces the expected event sequence.
//!
//! Stages C/D will add network-touching tests behind a `cargo test --
//! --ignored` gate; for now, network is out of scope to keep CI hermetic.

use pf2e_companion_lib::llm::{LlmConfig, LlmProviderKind, LlmRegistry, SseBuffer};

#[tokio::test]
async fn registry_starts_empty() {
    let r = LlmRegistry::new();
    let s = r.status(false).await;
    assert!(!s.configured);
    assert!(s.provider.is_none());
    assert!(s.model.is_none());
}

#[tokio::test]
async fn registry_configure_ollama_then_clear() {
    let r = LlmRegistry::new();
    r.configure(
        LlmConfig {
            provider: LlmProviderKind::Ollama,
            model: "qwen3:4b".to_string(),
            base_url: Some("http://localhost:11434".to_string()),
        },
        None,
    )
    .await
    .expect("ollama config without api key should succeed");

    let s = r.status(false).await;
    assert!(s.configured);
    assert_eq!(s.provider, Some(LlmProviderKind::Ollama));
    assert_eq!(s.model.as_deref(), Some("qwen3:4b"));
    // Ollama doesn't need a key, so key_present stays whatever the caller
    // passed in (false here).
    assert!(!s.key_present);

    r.clear().await;
    let s = r.status(false).await;
    assert!(!s.configured);
    assert!(s.provider.is_none());
}

#[tokio::test]
async fn registry_configure_anthropic_requires_key() {
    let r = LlmRegistry::new();
    let err = r
        .configure(
            LlmConfig {
                provider: LlmProviderKind::Anthropic,
                model: "claude-sonnet-4-6".to_string(),
                base_url: None,
            },
            None,
        )
        .await
        .expect_err("anthropic without an api key must fail");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("api key") || msg.to_lowercase().contains("anthropic"),
        "error mentions api key requirement, got: {msg}"
    );
}

#[tokio::test]
async fn registry_configure_anthropic_succeeds_with_key() {
    let r = LlmRegistry::new();
    r.configure(
        LlmConfig {
            provider: LlmProviderKind::Anthropic,
            model: "claude-sonnet-4-6".to_string(),
            base_url: None,
        },
        Some("sk-ant-fake-test-key".to_string()),
    )
    .await
    .expect("anthropic config with a key should succeed (no network call)");

    // `key_present` flag is supplied by the caller (commands.rs derives it
    // from the keystore); the registry just echoes it back.
    let s = r.status(true).await;
    assert!(s.configured);
    assert_eq!(s.provider, Some(LlmProviderKind::Anthropic));
    assert!(s.key_present);
}

#[tokio::test]
async fn registry_reconfigure_swaps_provider() {
    let r = LlmRegistry::new();
    r.configure(
        LlmConfig {
            provider: LlmProviderKind::Ollama,
            model: "qwen3:4b".to_string(),
            base_url: Some("http://localhost:11434".to_string()),
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(r.current_kind().await, Some(LlmProviderKind::Ollama));

    r.configure(
        LlmConfig {
            provider: LlmProviderKind::Anthropic,
            model: "claude-haiku-4-5-20251001".to_string(),
            base_url: None,
        },
        Some("sk-ant-fake".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(r.current_kind().await, Some(LlmProviderKind::Anthropic));
}

#[tokio::test]
async fn ollama_provider_rejects_empty_base_url() {
    let r = LlmRegistry::new();
    let err = r
        .configure(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "qwen3:4b".to_string(),
                base_url: Some("".to_string()),
            },
            None,
        )
        .await
        .expect_err("empty base_url should fail");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("base_url") || msg.to_lowercase().contains("empty"),
        "error mentions empty base_url, got: {msg}"
    );
}

#[test]
fn sse_buffer_replays_realistic_anthropic_stream() {
    // Reconstructed from a real Anthropic SSE response. We split it into
    // arbitrary byte fragments to ensure the buffer survives partial
    // frames at any boundary.
    let full = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"role\":\"assistant\"}}\n",
        "\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n",
        "\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
        "\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":12,\"output_tokens\":4}}\n",
        "\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n",
        "\n",
    );

    // Chop the wire into 17-byte fragments — small enough to split in
    // mid-frame, mid-data-line, and mid-blank-separator.
    let mut buf = SseBuffer::default();
    let mut events = Vec::new();
    for chunk in full.as_bytes().chunks(17) {
        let s = std::str::from_utf8(chunk).unwrap();
        events.extend(buf.push(s));
    }

    let event_names: Vec<_> = events
        .iter()
        .map(|e| e.event.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        event_names,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ],
        "SSE parser must reproduce the full event sequence regardless of fragmentation",
    );
}
