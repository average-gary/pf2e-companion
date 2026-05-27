//! Phase 6 § D — tool-use loop + agent surface.
//!
//! What we cover:
//! - Tool dispatch routes to the right local handler for every name in
//!   `TOOL_IDS` (xp_budget, lookup_alias, lookup_miracle, search,
//!   validate_statblock).
//! - The agent loop drives a multi-iteration conversation:
//!     turn 1 → model emits `xp_budget(4, moderate)` tool call
//!     turn 2 → model sees the tool result, emits final assistant text
//!   Asserts the messages history grows correctly and that the final
//!   user-visible text matches.
//! - The 5-iteration cap fires when the model loops forever.
//! - Tool errors are surfaced as `AgentEvent::ToolResult { error: true }`
//!   and fed back to the model as JSON `{"error":...}`.

use anyhow::Result;
use async_trait::async_trait;
use futures_util::stream;
use futures_util::StreamExt;
use pf2e_companion_lib::db::Db;
use pf2e_companion_lib::llm::{
    ChatChunk, ChatOpts, ChatStream, LlmConfig, LlmProvider, LlmProviderKind, LlmRegistry,
    Message, Role, ToolCall, UsageSummary,
};
use pf2e_companion_lib::llm_tools::{self, AgentEvent, MAX_ITERATIONS, TOOL_IDS};
use serde_json::json;
use std::sync::{Arc, Mutex};

// ===== Scripted provider ===============================================
//
// On each `chat()` call, drains one element from `script` and streams it
// out. A "turn" is a list of chunks the model would have emitted.

type Turn = Vec<ChatChunk>;

struct ScriptedProvider {
    script: Mutex<Vec<Turn>>,
    /// Captures the messages the agent sent on each turn so tests can
    /// assert the conversation grew correctly.
    seen: Mutex<Vec<Vec<Message>>>,
}

impl ScriptedProvider {
    fn new(script: Vec<Turn>) -> Self {
        Self {
            script: Mutex::new(script),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn turns_seen(&self) -> Vec<Vec<Message>> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }
    fn model(&self) -> &str {
        "scripted-fake"
    }
    async fn chat(&self, messages: Vec<Message>, _opts: ChatOpts) -> Result<ChatStream> {
        self.seen.lock().unwrap().push(messages);
        let next = {
            let mut s = self.script.lock().unwrap();
            if s.is_empty() {
                vec![ChatChunk::End { usage: None }]
            } else {
                s.remove(0)
            }
        };
        let chunks: Vec<Result<ChatChunk>> = next.into_iter().map(Ok).collect();
        Ok(stream::iter(chunks).boxed())
    }
    async fn embed(&self, _texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(Vec::new())
    }
}

// Trait-object handle so the registry can own a `Box<dyn LlmProvider>`
// while tests still hold a strong ref to the `ScriptedProvider` for
// inspection.
struct ScriptedHandle(Arc<ScriptedProvider>);

#[async_trait]
impl LlmProvider for ScriptedHandle {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn model(&self) -> &str {
        self.0.model()
    }
    async fn chat(&self, messages: Vec<Message>, opts: ChatOpts) -> Result<ChatStream> {
        self.0.chat(messages, opts).await
    }
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        self.0.embed(texts).await
    }
}

// ===== Helpers ========================================================

fn open_seeded_db(tmp: &std::path::Path) -> Arc<Db> {
    let database = Db::open(&tmp.join("phase6d.db")).unwrap();
    database.seed_reference_data().unwrap();
    Arc::new(database)
}

async fn drain_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Result<AgentEvent>>,
) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    while let Some(item) = rx.recv().await {
        match item {
            Ok(ev) => {
                let is_terminal = matches!(ev, AgentEvent::End { .. });
                out.push(ev);
                if is_terminal {
                    break;
                }
            }
            Err(e) => panic!("agent error: {e}"),
        }
    }
    out
}

// ===== Tests ===========================================================

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_routes_xp_budget() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = LlmRegistry::new();
    let call = ToolCall {
        id: "tool_1".to_string(),
        name: "xp_budget".to_string(),
        input: json!({ "party_size": 4, "difficulty": "moderate" }),
    };
    let result = llm_tools::dispatch_tool(&call, &db, &registry).await.unwrap();
    assert_eq!(result["xp_budget"], 80);
    assert_eq!(result["per_pc_adjust"], 20);
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_routes_validate_statblock() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = LlmRegistry::new();
    let call = ToolCall {
        id: "tool_1".to_string(),
        name: "validate_statblock".to_string(),
        input: json!({ "statblock": { "name": "Fake", "level": 5 } }),
    };
    let result = llm_tools::dispatch_tool(&call, &db, &registry).await.unwrap();
    assert!(result.get("valid").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_routes_lookup_alias() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = LlmRegistry::new();
    let call = ToolCall {
        id: "t".to_string(),
        name: "lookup_alias".to_string(),
        input: json!({ "name": "Aasimar" }),
    };
    let result = llm_tools::dispatch_tool(&call, &db, &registry).await.unwrap();
    let arr = result.get("matches").unwrap().as_array().unwrap();
    assert!(!arr.is_empty(), "Aasimar should map to Nephilim");
    assert_eq!(arr[0]["remaster_name"], "Nephilim");
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_routes_lookup_miracle() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = LlmRegistry::new();
    let call = ToolCall {
        id: "t".to_string(),
        name: "lookup_miracle".to_string(),
        input: json!({ "query": "5000" }),
    };
    let result = llm_tools::dispatch_tool(&call, &db, &registry).await.unwrap();
    assert!(result.get("matches").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_unknown_tool_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = LlmRegistry::new();
    let call = ToolCall {
        id: "t".to_string(),
        name: "definitely_not_a_tool".to_string(),
        input: json!({}),
    };
    let err = llm_tools::dispatch_tool(&call, &db, &registry).await.unwrap_err();
    assert!(format!("{err}").contains("unknown tool"));
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_loop_runs_one_tool_then_finalizes() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = Arc::new(LlmRegistry::new());

    // Turn 1: model emits some prose plus a tool call.
    // Turn 2: model emits the final answer.
    let prov = ScriptedProvider::new(vec![
        vec![
            ChatChunk::Text("Let me check the budget. ".to_string()),
            ChatChunk::ToolCall(ToolCall {
                id: "call_xp_1".to_string(),
                name: "xp_budget".to_string(),
                input: json!({ "party_size": 4, "difficulty": "moderate" }),
            }),
            ChatChunk::End { usage: Some(UsageSummary { input_tokens: Some(100), output_tokens: Some(20), ..Default::default() }) },
        ],
        vec![
            ChatChunk::Text("A moderate encounter for 4 PCs is 80 XP.".to_string()),
            ChatChunk::End { usage: Some(UsageSummary { input_tokens: Some(50), output_tokens: Some(10), ..Default::default() }) },
        ],
    ]);
    let prov_arc = Arc::new(prov);
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "scripted-fake".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(ScriptedHandle(prov_arc.clone())),
        )
        .await;

    let messages = vec![Message::new(Role::User, "Build a moderate encounter for 4 PCs at level 6.")];
    let mut rx = llm_tools::run_agent(
        db,
        registry,
        messages,
        "You are a helpful PF2e GM.".to_string(),
        false,
        None,
        None,
    );
    let events = drain_events(&mut rx).await;

    // Sanity: final event is End with iterations == 2 and combined usage.
    let end = events
        .iter()
        .rev()
        .find_map(|e| match e {
            AgentEvent::End { usage, iterations } => Some((usage.clone(), *iterations)),
            _ => None,
        })
        .expect("End event present");
    assert_eq!(end.1, 2, "two iterations: tool call + finalization");
    assert_eq!(end.0.input_tokens, Some(150));
    assert_eq!(end.0.output_tokens, Some(30));

    // Tool start + result both surfaced.
    let started = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolStart { name, .. } => Some(name.clone()),
            _ => None,
        });
    assert_eq!(started.as_deref(), Some("xp_budget"));
    let result_event = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolResult { name, result, error, .. } => {
                Some((name.clone(), result.clone(), *error))
            }
            _ => None,
        })
        .expect("ToolResult emitted");
    assert_eq!(result_event.0, "xp_budget");
    assert!(!result_event.2);
    assert_eq!(result_event.1["xp_budget"], 80);

    // Concatenated text should include both turns' prose.
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains("Let me check the budget"));
    assert!(text.contains("80 XP"));

    // The provider saw the conversation grow: turn 1 = 1 user, turn 2 =
    // user + assistant(w/ tool_calls) + tool result.
    let seen = prov_arc.turns_seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].len(), 1);
    assert_eq!(seen[1].len(), 3);
    assert_eq!(seen[1][1].role, Role::Assistant);
    assert_eq!(seen[1][1].tool_calls.len(), 1);
    assert_eq!(seen[1][2].role, Role::Tool);
    assert_eq!(seen[1][2].tool_call_id.as_deref(), Some("call_xp_1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_loop_caps_at_max_iterations() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = Arc::new(LlmRegistry::new());

    // Each turn emits a tool call but never finalizes — the loop should
    // abort after MAX_ITERATIONS rather than run forever.
    let mut script: Vec<Turn> = Vec::new();
    for _ in 0..(MAX_ITERATIONS + 2) {
        script.push(vec![
            ChatChunk::ToolCall(ToolCall {
                id: format!("call_{}", script.len()),
                name: "xp_budget".to_string(),
                input: json!({ "party_size": 4, "difficulty": "moderate" }),
            }),
            ChatChunk::End { usage: None },
        ]);
    }
    let prov = Arc::new(ScriptedProvider::new(script));
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "scripted-fake".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(ScriptedHandle(prov.clone())),
        )
        .await;

    let mut rx = llm_tools::run_agent(
        db,
        registry,
        vec![Message::new(Role::User, "loop forever pls")],
        "system".to_string(),
        false,
        None,
        None,
    );

    // Drain to the first error (the cap message).
    let mut got_cap = false;
    while let Some(item) = rx.recv().await {
        if let Err(e) = item {
            assert!(format!("{e}").contains("iteration cap"), "unexpected error: {e}");
            got_cap = true;
            break;
        }
    }
    assert!(got_cap, "agent should hit the iteration cap");
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_loop_recovers_from_tool_error() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open_seeded_db(tmp.path());
    let registry = Arc::new(LlmRegistry::new());

    // Turn 1: model calls a non-existent tool. Loop should record the
    // error in the tool message rather than abort, then turn 2 finalizes.
    let prov = Arc::new(ScriptedProvider::new(vec![
        vec![
            ChatChunk::ToolCall(ToolCall {
                id: "bad_call".to_string(),
                name: "not_a_real_tool".to_string(),
                input: json!({}),
            }),
            ChatChunk::End { usage: None },
        ],
        vec![
            ChatChunk::Text("Sorry, that tool isn't available.".to_string()),
            ChatChunk::End { usage: None },
        ],
    ]));
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "scripted-fake".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(ScriptedHandle(prov.clone())),
        )
        .await;

    let mut rx = llm_tools::run_agent(
        db,
        registry,
        vec![Message::new(Role::User, "do stuff")],
        "system".to_string(),
        false,
        None,
        None,
    );
    let events = drain_events(&mut rx).await;

    let tool_result = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolResult { error, result, .. } => Some((*error, result.clone())),
            _ => None,
        })
        .expect("tool result event present");
    assert!(tool_result.0, "error flag set");
    assert!(tool_result.1.get("error").is_some());

    let text: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains("isn't available"));
}

#[test]
fn tool_specs_match_dispatch_table() {
    // Sanity: every spec name has a matching dispatch arm and vice-versa.
    let specs = llm_tools::tool_specs();
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    let mut expected: Vec<&str> = TOOL_IDS.to_vec();
    expected.sort();
    let mut got = names.clone();
    got.sort();
    assert_eq!(got, expected, "tool_specs matches TOOL_IDS exactly");
}

