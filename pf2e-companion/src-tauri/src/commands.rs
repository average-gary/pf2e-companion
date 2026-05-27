//! Capability-gated IPC commands. Phase 0 ships a minimal subset
//! sufficient for the smoke test; the full surface is documented in
//! the plan spec § 3.

use crate::db::Db;
use crate::foundry::{self, ImportReport, LicensePosture};
use crate::keystore;
use crate::llm::{self, ChatChunk, ChatOpts, LlmConfig, LlmRegistry, LlmStatus, Message};
use crate::rag;
use crate::rules::{self, Difficulty};
use crate::vault_write::{
    self, Campaign, CrudResult, EntityInput, EntityPatch, RelationRow, VaultRoot,
};
use anyhow::Result;
use futures_util::StreamExt;
use rusqlite::params;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use uuid::Uuid;
use tauri::State;

/// Hybrid search. Delegates to `rag::hybrid_search` which fuses FTS5 and
/// vector retrieval via reciprocal-rank fusion when the LLM provider is
/// configured AND the corpus has been indexed; otherwise transparently
/// falls back to FTS-only.
#[tauri::command]
pub async fn search(
    query: String,
    lens: Option<String>,
    db: State<'_, Arc<Db>>,
    llm: State<'_, Arc<LlmRegistry>>,
) -> Result<Vec<rag::HybridHit>, String> {
    let lens = lens.unwrap_or_else(|| "lewisian".to_string());
    rag::hybrid_search(&db, &llm, &query, &lens)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Serialize, Debug)]
pub struct EntitySummary {
    pub id: String,
    pub title: String,
    pub r#type: String,
    pub lens: Option<String>,
}

#[tauri::command]
pub fn list_entities(
    type_filter: Option<String>,
    lens: Option<String>,
    db: State<'_, Arc<Db>>,
) -> Result<Vec<EntitySummary>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let lens = lens.unwrap_or_else(|| "lewisian".to_string());
    let type_filter = type_filter.unwrap_or_default();
    let mut stmt = conn
        .prepare(
            r#"
            SELECT
                id,
                COALESCE(json_extract(frontmatter, '$.title'), id) AS title,
                type,
                lens
            FROM entities
            WHERE (?1 = '' OR type = ?1)
              AND (lens IS NULL OR lens = ?2)
            ORDER BY title
            LIMIT 200
            "#,
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![type_filter, lens], |row| {
            Ok(EntitySummary {
                id: row.get(0)?,
                title: row.get(1)?,
                r#type: row.get(2)?,
                lens: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[derive(Serialize, Debug)]
pub struct EntityDetail {
    pub id: String,
    pub title: String,
    pub r#type: String,
    pub lens: Option<String>,
    pub license_provenance: String,
    pub source: String,
    pub frontmatter: serde_json::Value,
    pub body: Option<String>,
    pub statblock: Option<serde_json::Value>,
}

#[tauri::command]
pub fn get_entity(
    id: String,
    db: State<'_, Arc<Db>>,
) -> Result<Option<EntityDetail>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id,
                   COALESCE(json_extract(frontmatter, '$.title'), id) AS title,
                   type,
                   lens,
                   license_provenance,
                   source,
                   frontmatter,
                   body,
                   statblock
            FROM entities
            WHERE id = ?1
            LIMIT 1
            "#,
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![id], |row| {
            let fm_str: String = row.get(6)?;
            let sb_str: Option<String> = row.get(8)?;
            Ok(EntityDetail {
                id: row.get(0)?,
                title: row.get(1)?,
                r#type: row.get(2)?,
                lens: row.get(3)?,
                license_provenance: row.get(4)?,
                source: row.get(5)?,
                frontmatter: serde_json::from_str(&fm_str)
                    .unwrap_or(serde_json::Value::Null),
                body: row.get(7)?,
                statblock: sb_str
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok()),
            })
        })
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

/// Schema-version probe. Phase 0 sanity check.
#[tauri::command]
pub fn schema_version(db: State<'_, Arc<Db>>) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT MAX(version) FROM schema_version",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())
}

#[derive(Serialize, Debug)]
pub struct AliasHit {
    pub legacy_name: String,
    pub remaster_name: String,
    pub category: String,
    pub notes: Option<String>,
}

/// Lookup a Remaster alias by either side (legacy or current).
/// Case-insensitive, exact-match-first then prefix-match.
#[tauri::command]
pub fn lookup_alias(
    name: String,
    db: State<'_, Arc<Db>>,
) -> Result<Vec<AliasHit>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let needle = format!("{trimmed}%");
    let mut stmt = conn
        .prepare(
            "SELECT legacy_name, remaster_name, category, notes
             FROM remaster_aliases
             WHERE legacy_name = ?1 COLLATE NOCASE
                OR remaster_name = ?1 COLLATE NOCASE
                OR legacy_name LIKE ?2 COLLATE NOCASE
                OR remaster_name LIKE ?2 COLLATE NOCASE
             ORDER BY
               CASE WHEN legacy_name = ?1 COLLATE NOCASE OR remaster_name = ?1 COLLATE NOCASE THEN 0 ELSE 1 END,
               legacy_name
             LIMIT 25",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![trimmed, needle], |row| {
            Ok(AliasHit {
                legacy_name: row.get(0)?,
                remaster_name: row.get(1)?,
                category: row.get(2)?,
                notes: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[derive(Serialize, Debug)]
pub struct MiracleHit {
    pub miracle: String,
    pub reference: String,
    pub book: String,
    pub spell_name: String,
    pub tradition: Option<String>,
    pub sanctification: Option<String>,
    pub notes: Option<String>,
}

/// Lookup biblical-miracle → PF2e spell mapping by Bible reference
/// ("Mt 14:25"), miracle name fragment, or book.
#[tauri::command]
pub fn lookup_miracle(
    query: String,
    db: State<'_, Arc<Db>>,
) -> Result<Vec<MiracleHit>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let needle = format!("%{trimmed}%");
    let mut stmt = conn
        .prepare(
            "SELECT miracle, reference, book, spell_name, tradition, sanctification, notes
             FROM miracle_spell_map
             WHERE reference   = ?1 COLLATE NOCASE
                OR miracle    LIKE ?2 COLLATE NOCASE
                OR reference  LIKE ?2 COLLATE NOCASE
                OR book        = ?1 COLLATE NOCASE
                OR spell_name LIKE ?2 COLLATE NOCASE
             ORDER BY
               CASE WHEN reference = ?1 COLLATE NOCASE THEN 0 ELSE 1 END,
               book, miracle
             LIMIT 50",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![trimmed, needle], |row| {
            Ok(MiracleHit {
                miracle: row.get(0)?,
                reference: row.get(1)?,
                book: row.get(2)?,
                spell_name: row.get(3)?,
                tradition: row.get(4)?,
                sanctification: row.get(5)?,
                notes: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[derive(Serialize, Debug)]
pub struct XpBudgetResult {
    pub party_size: u8,
    pub difficulty: Difficulty,
    pub xp_budget: i32,
    pub per_pc_adjust: i32,
    pub base_for_party_of_4: i32,
}

/// PF2e Remaster encounter XP budget. See `rules::xp_budget`.
#[tauri::command]
pub fn xp_budget(
    party_size: u8,
    difficulty: Difficulty,
) -> Result<XpBudgetResult, String> {
    if party_size == 0 || party_size > 12 {
        return Err(format!("party_size {party_size} outside the supported 1..=12 range"));
    }
    Ok(XpBudgetResult {
        party_size,
        difficulty,
        xp_budget: rules::xp_budget(party_size, difficulty),
        per_pc_adjust: difficulty.per_pc_adjust(),
        base_for_party_of_4: difficulty.base_budget_for_party_of_4(),
    })
}

/// Per-creature XP cost relative to party level. Helper for the encounter
/// builder UI.
#[tauri::command]
pub fn creature_xp(party_level_delta: i32) -> Result<Option<i32>, String> {
    Ok(rules::creature_xp_for_party_level_delta(party_level_delta))
}

#[tauri::command]
pub fn validate_statblock(
    statblock: serde_json::Value,
) -> Result<rules::ValidationResult, String> {
    Ok(rules::validate_statblock(&statblock))
}

#[derive(Serialize, Debug)]
pub struct LensManifest {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

/// Import a Foundry-pf2e pack directory (or single .json) at `root_path`.
/// `license` is one of: orc | community-use | homebrew | proprietary.
#[tauri::command]
pub fn import_foundry_pack(
    root_path: String,
    license: String,
    db: State<'_, Arc<Db>>,
) -> Result<ImportReport, String> {
    let posture = match license.as_str() {
        "orc" => LicensePosture::Orc,
        "community-use" => LicensePosture::CommunityUse,
        "homebrew" => LicensePosture::Homebrew,
        "proprietary" => LicensePosture::Proprietary,
        other => return Err(format!("unknown license `{other}`")),
    };
    let path = PathBuf::from(root_path);
    foundry::import_packs(&db, &path, posture).map_err(|e| e.to_string())
}

// === Phase 3 — campaigns + entity CRUD =====================================

#[tauri::command]
pub fn list_campaigns(
    vault: State<'_, Arc<VaultRoot>>,
    db: State<'_, Arc<Db>>,
) -> Result<Vec<Campaign>, String> {
    vault_write::list_campaigns(&vault, &db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_campaign(
    name: String,
    default_lens: Option<String>,
    vault: State<'_, Arc<VaultRoot>>,
) -> Result<Campaign, String> {
    vault_write::create_campaign(&vault, &name, default_lens.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_entity(
    input: EntityInput,
    vault: State<'_, Arc<VaultRoot>>,
) -> Result<CrudResult, String> {
    vault_write::create_entity(&vault, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_entity(
    id: String,
    patch: EntityPatch,
    vault: State<'_, Arc<VaultRoot>>,
    db: State<'_, Arc<Db>>,
) -> Result<CrudResult, String> {
    vault_write::update_entity(&vault, &db, &id, &patch).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_entity(
    id: String,
    vault: State<'_, Arc<VaultRoot>>,
    db: State<'_, Arc<Db>>,
) -> Result<(), String> {
    vault_write::delete_entity(&vault, &db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_relation(
    from_id: String,
    edge_type: String,
    to_id: String,
    db: State<'_, Arc<Db>>,
) -> Result<(), String> {
    vault_write::add_relation(&db, &from_id, &edge_type, &to_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_relation(
    from_id: String,
    edge_type: String,
    to_id: String,
    db: State<'_, Arc<Db>>,
) -> Result<(), String> {
    vault_write::delete_relation(&db, &from_id, &edge_type, &to_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_relations(
    entity_id: String,
    db: State<'_, Arc<Db>>,
) -> Result<Vec<RelationRow>, String> {
    vault_write::list_relations(&db, &entity_id).map_err(|e| e.to_string())
}

// === Phase 6 — LLM (off by default, BYO key) ==============================

#[tauri::command]
pub async fn llm_status(llm: State<'_, Arc<LlmRegistry>>) -> Result<LlmStatus, String> {
    let kind = llm.current_kind().await;
    let key_present = match kind {
        Some(llm::LlmProviderKind::Anthropic) => keystore::has_key("anthropic"),
        Some(llm::LlmProviderKind::Ollama) | None => true, // Ollama needs no key
    };
    Ok(llm.status(key_present).await)
}

#[tauri::command]
pub async fn llm_configure(
    config: LlmConfig,
    api_key: Option<String>,
    llm: State<'_, Arc<LlmRegistry>>,
) -> Result<LlmStatus, String> {
    if matches!(config.provider, llm::LlmProviderKind::Anthropic) {
        let supplied = api_key.as_deref().filter(|k| !k.is_empty());
        let final_key = match supplied {
            Some(k) => Some(k.to_string()),
            None => keystore::get_key("anthropic").map_err(|e| e.to_string())?,
        };
        let final_key = final_key.ok_or_else(|| {
            "anthropic provider requires an api key (none supplied and none in keychain)"
                .to_string()
        })?;
        keystore::set_key("anthropic", &final_key).map_err(|e| e.to_string())?;
        llm.configure(config.clone(), Some(final_key))
            .await
            .map_err(|e| e.to_string())?;
    } else {
        llm.configure(config.clone(), None)
            .await
            .map_err(|e| e.to_string())?;
    }
    let key_present = match config.provider {
        llm::LlmProviderKind::Anthropic => keystore::has_key("anthropic"),
        llm::LlmProviderKind::Ollama => true,
    };
    Ok(llm.status(key_present).await)
}

#[tauri::command]
pub async fn llm_clear_config(
    llm: State<'_, Arc<LlmRegistry>>,
) -> Result<LlmStatus, String> {
    llm.clear().await;
    keystore::clear_key("anthropic").map_err(|e| e.to_string())?;
    Ok(llm.status(false).await)
}

#[derive(Serialize, Debug, Clone)]
pub struct LlmTokenEvent {
    pub session_id: String,
    pub token: Option<String>,
    pub done: bool,
    pub error: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(serde::Deserialize, Debug)]
pub struct LlmChatRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// Stream a chat response. Returns immediately with a session id; tokens
/// arrive as `llm:token` events keyed by that id.
#[tauri::command]
pub async fn llm_chat(
    request: LlmChatRequest,
    app: tauri::AppHandle,
    llm: State<'_, Arc<LlmRegistry>>,
) -> Result<String, String> {
    let session_id = Uuid::new_v4().to_string();
    let opts = ChatOpts {
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        system: request.system,
    };

    let session = session_id.clone();
    let session_for_err = session_id.clone();
    let app_clone = app.clone();
    let llm_arc = llm.inner().clone();
    let messages = request.messages;

    tokio::spawn(async move {
        // Acquire the provider stream synchronously *while holding* the
        // read-guard, then drop the guard before consuming the stream
        // — provider.chat() returns a future that must complete before
        // streaming can begin.
        let stream_result = {
            let guard = llm_arc.read().await;
            match guard.as_ref() {
                Some(active) => active.provider().chat(messages, opts).await,
                None => Err(anyhow::anyhow!("llm provider not configured")),
            }
        };

        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                let _ = app.emit(
                    "llm:token",
                    LlmTokenEvent {
                        session_id: session_for_err,
                        token: None,
                        done: true,
                        error: Some(e.to_string()),
                        input_tokens: None,
                        output_tokens: None,
                    },
                );
                return;
            }
        };

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(ChatChunk::Text(token)) => {
                    let _ = app_clone.emit(
                        "llm:token",
                        LlmTokenEvent {
                            session_id: session.clone(),
                            token: Some(token),
                            done: false,
                            error: None,
                            input_tokens: None,
                            output_tokens: None,
                        },
                    );
                }
                Ok(ChatChunk::End { usage }) => {
                    let _ = app_clone.emit(
                        "llm:token",
                        LlmTokenEvent {
                            session_id: session.clone(),
                            token: None,
                            done: true,
                            error: None,
                            input_tokens: usage.as_ref().and_then(|u| u.input_tokens),
                            output_tokens: usage.as_ref().and_then(|u| u.output_tokens),
                        },
                    );
                }
                Err(e) => {
                    let _ = app_clone.emit(
                        "llm:token",
                        LlmTokenEvent {
                            session_id: session.clone(),
                            token: None,
                            done: true,
                            error: Some(e.to_string()),
                            input_tokens: None,
                            output_tokens: None,
                        },
                    );
                    return;
                }
            }
        }
    });

    Ok(session_id)
}

// === Phase 6 § C — RAG (corpus embedding + index stats) ===================

#[derive(Serialize, Debug, Clone)]
pub struct RagIndexStats {
    pub indexed: bool,
    pub entities: i64,
    pub chunks: i64,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[tauri::command]
pub fn rag_index_stats(db: State<'_, Arc<Db>>) -> Result<RagIndexStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM embeddings_meta", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if chunks == 0 {
        return Ok(RagIndexStats {
            indexed: false,
            entities: 0,
            chunks: 0,
            provider: None,
            model: None,
        });
    }
    let entities: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT entity_id) FROM embeddings_meta",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let (provider, model): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT provider, model FROM embeddings_meta LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    Ok(RagIndexStats {
        indexed: true,
        entities,
        chunks,
        provider,
        model,
    })
}

/// (Re)embed the bundled content corpus with the active provider's
/// embedding model. Wipes prior vectors first. Long-running.
#[tauri::command]
pub async fn rag_reindex(
    db: State<'_, Arc<Db>>,
    llm: State<'_, Arc<LlmRegistry>>,
) -> Result<rag::EmbedReport, String> {
    rag::embed_corpus(&db, &llm)
        .await
        .map_err(|e| e.to_string())
}

/// The 5 v1 lens packs. Phase 1 returns the manifest; the actual content
/// packs land in Phase 2 alongside the YHWH and saint entries.
#[tauri::command]
pub fn list_lenses() -> Result<Vec<LensManifest>, String> {
    Ok(vec![
        LensManifest {
            id: "lewisian",
            label: "Lewisian (mere Christianity)",
            description:
                "Default. Charism+Lewisian magic-theology hybrid; deferred denominational specifics.",
        },
        LensManifest {
            id: "catholic",
            label: "Catholic",
            description:
                "73-book canon (deuterocanon); 9-choir angels; saint-attached Champion patrons; Thaumaturge implements as relics.",
        },
        LensManifest {
            id: "reformed",
            label: "Reformed",
            description:
                "66-book canon; cessationist tilt; covenant theology scaffold; Word-as-sword.",
        },
        LensManifest {
            id: "pentecostal",
            label: "Pentecostal",
            description:
                "66-book canon; spiritual-warfare frame; healing/tongues/prophecy charisms.",
        },
        LensManifest {
            id: "orthodox",
            label: "Orthodox",
            description:
                "LXX+ canon; theosis; icons; Aerial Toll Houses as a level-15+ post-mortem dungeon.",
        },
    ])
}
