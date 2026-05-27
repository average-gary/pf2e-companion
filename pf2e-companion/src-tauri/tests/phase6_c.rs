//! Phase 6 § C — RAG over the bundled content.
//!
//! What we cover:
//! - `chunk_paragraphs` shape (covered in `rag.rs` unit tests too;
//!   re-exercised here against multi-paragraph fixtures).
//! - `hybrid_search` gracefully falls back to FTS-only when no provider
//!   is configured.
//! - End-to-end embed → vector search → RRF fusion against a tiny
//!   3-entry mini corpus using a deterministic in-process fake
//!   provider — proves the FTS+vector half ranks the same entry top-1
//!   even when only the vector half should match.

use anyhow::Result;
use async_trait::async_trait;
use futures_util::stream;
use futures_util::StreamExt;
use pf2e_companion_lib::db::Db;
use pf2e_companion_lib::llm::{
    ChatChunk, ChatOpts, ChatStream, LlmConfig, LlmProvider, LlmProviderKind, LlmRegistry, Message,
};
use pf2e_companion_lib::rag::{self, EMBED_DIM};
use rusqlite::params;

// ===== Deterministic fake embedder =====================================
//
// Maps a single keyword to a 768-dim "hot" vector — value 1.0 in one
// reserved slot, 0.0 elsewhere. Different keywords get different slots,
// so cosine/L2 ranks the matching entity first. Anything not containing
// the keyword embeds to the zero vector, which sorts last.

struct FakeEmbedder {
    /// Slot index for each registered keyword.
    rules: Vec<(String, usize)>,
}

impl FakeEmbedder {
    fn new(rules: &[(&str, usize)]) -> Self {
        Self {
            rules: rules.iter().map(|(k, i)| (k.to_string(), *i)).collect(),
        }
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0_f32; EMBED_DIM];
        let lower = text.to_lowercase();
        for (kw, slot) in &self.rules {
            if lower.contains(kw) {
                v[*slot] = 1.0;
            }
        }
        v
    }
}

#[async_trait]
impl LlmProvider for FakeEmbedder {
    fn id(&self) -> &'static str {
        "ollama"
    }
    fn model(&self) -> &str {
        "fake-embedder-768"
    }
    async fn chat(&self, _m: Vec<Message>, _o: ChatOpts) -> Result<ChatStream> {
        // Not exercised by the RAG path; return a single end frame so
        // anything that *does* call us doesn't deadlock.
        Ok(stream::iter(vec![Ok(ChatChunk::End { usage: None })]).boxed())
    }
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }
}

// ===== Helpers ========================================================

fn open_db_with_seed(tmp: &std::path::Path) -> Db {
    let database = Db::open(&tmp.join("phase6c.db")).unwrap();
    {
        let conn = database.conn.lock().unwrap();
        // Three reference entities. The "fire from heaven" prompt should
        // surface the Elijah-on-Carmel entry via vector even though the
        // entry's body uses the wording "holy light".
        let rows: &[(&str, &str, &str, &str)] = &[
            (
                "lewisian:elijah-carmel",
                "Elijah on Carmel",
                r#"{"title":"Elijah on Carmel","type":"miracle","lens":"lewisian"}"#,
                "On Mount Carmel, holy light fell from the heavens and consumed the soaked altar. The prophets of Baal were silenced.",
            ),
            (
                "lewisian:moses-staff",
                "Moses' Staff",
                r#"{"title":"Moses' Staff","type":"item","lens":"lewisian"}"#,
                "A wooden rod that parts seas and turns to a serpent. Used to deliver the plagues.",
            ),
            (
                "lewisian:david-goliath",
                "David and Goliath",
                r#"{"title":"David and Goliath","type":"event","lens":"lewisian"}"#,
                "A young shepherd defeats a Philistine champion with a sling and a stone in the name of the Lord.",
            ),
        ];
        for (id, _title, frontmatter, body) in rows {
            conn.execute(
                r#"
                INSERT INTO entities
                  (id, type, campaign_id, source, lens, license_provenance,
                   frontmatter, body, body_text, statblock, file_path, mtime, hash)
                VALUES (?1, 'miracle', '_reference', 'reference', 'lewisian', 'orc',
                        ?2, ?3, ?3, NULL, ?4, 0, 'phase6c-fixture')
                "#,
                params![id, frontmatter, body, format!("test/{id}.md")],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO entities_fts(rowid, title, body_text)
                 SELECT rowid,
                        json_extract(frontmatter, '$.title') AS title,
                        body_text
                 FROM entities WHERE id = ?1",
                params![id],
            )
            .unwrap();
        }
    }
    database
}

// ===== Tests ===========================================================

#[tokio::test]
async fn hybrid_search_falls_back_to_fts_when_no_llm() {
    let tmp = tempfile::tempdir().unwrap();
    let database = open_db_with_seed(tmp.path());
    let registry = LlmRegistry::new();

    // No provider configured → vector half disabled; FTS-only path runs.
    let hits = rag::hybrid_search(&database, &registry, "Carmel", "lewisian")
        .await
        .unwrap();
    assert!(!hits.is_empty(), "FTS should still match `Carmel` literal");
    assert_eq!(hits[0].id, "lewisian:elijah-carmel");
    assert_eq!(hits[0].source, "fts");
}

#[tokio::test]
async fn hybrid_search_fuses_fts_and_vector() {
    let tmp = tempfile::tempdir().unwrap();
    let database = open_db_with_seed(tmp.path());
    let registry = LlmRegistry::new();

    // Embedding rules: any text containing "carmel" or "fire" or "holy
    // light" lights up slot 0 — they cluster together. Other entries
    // light up other slots so they don't collide.
    let fake = FakeEmbedder::new(&[
        ("carmel", 0),
        ("fire", 0),
        ("holy light", 0),
        ("heaven", 0),
        ("moses", 1),
        ("staff", 1),
        ("rod", 1),
        ("david", 2),
        ("goliath", 2),
        ("sling", 2),
    ]);
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "fake-embedder-768".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(fake),
        )
        .await;

    rag::embed_corpus(&database, &registry).await.unwrap();

    // The query "fire from heaven" doesn't appear literally in any
    // entry, but maps to slot 0, which the Carmel entry also occupies.
    // Vector half should rank Carmel first; FTS half won't surface it
    // (no shared terms). The fused score still puts Carmel on top.
    let hits = rag::hybrid_search(&database, &registry, "fire from heaven", "lewisian")
        .await
        .unwrap();
    assert!(!hits.is_empty(), "vector half should produce hits");
    assert_eq!(
        hits[0].id, "lewisian:elijah-carmel",
        "hybrid fusion must rank the semantic match first; got {hits:?}"
    );
    // It should be tagged as vec-only since FTS won't find "fire" or
    // "heaven" in the Carmel body (which says "holy light").
    assert_eq!(hits[0].source, "vec");
}

#[tokio::test]
async fn hybrid_search_marks_overlapping_hits_as_both() {
    let tmp = tempfile::tempdir().unwrap();
    let database = open_db_with_seed(tmp.path());
    let registry = LlmRegistry::new();

    let fake = FakeEmbedder::new(&[
        ("carmel", 0),
        ("holy light", 0),
        ("moses", 1),
        ("staff", 1),
        ("david", 2),
        ("goliath", 2),
    ]);
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "fake-embedder-768".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(fake),
        )
        .await;
    rag::embed_corpus(&database, &registry).await.unwrap();

    // "Carmel" matches FTS exactly *and* the vector half — fused source
    // should be "both".
    let hits = rag::hybrid_search(&database, &registry, "Carmel", "lewisian")
        .await
        .unwrap();
    let carmel = hits
        .iter()
        .find(|h| h.id == "lewisian:elijah-carmel")
        .expect("carmel hit present");
    assert_eq!(carmel.source, "both");
}

#[tokio::test]
async fn embed_corpus_populates_metadata_table() {
    let tmp = tempfile::tempdir().unwrap();
    let database = open_db_with_seed(tmp.path());
    let registry = LlmRegistry::new();

    let fake = FakeEmbedder::new(&[("carmel", 0), ("moses", 1), ("david", 2)]);
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "fake-embedder-768".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(fake),
        )
        .await;
    let report = rag::embed_corpus(&database, &registry).await.unwrap();

    assert_eq!(report.entities_processed, 3);
    assert!(report.chunks_embedded >= 3);
    assert_eq!(report.provider, "ollama");
    assert_eq!(report.model, "fake-embedder-768");

    let conn = database.conn.lock().unwrap();
    let n_meta: i64 = conn
        .query_row("SELECT COUNT(*) FROM embeddings_meta", [], |r| r.get(0))
        .unwrap();
    let n_vec: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities_vec", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_meta, n_vec, "vec and meta tables should be 1:1");
    assert!(n_meta >= 3, "at least 3 chunks (one per entity)");
}

#[tokio::test]
async fn reembed_replaces_prior_index() {
    let tmp = tempfile::tempdir().unwrap();
    let database = open_db_with_seed(tmp.path());
    let registry = LlmRegistry::new();

    let fake = FakeEmbedder::new(&[("carmel", 0), ("moses", 1), ("david", 2)]);
    registry
        .install_provider(
            LlmConfig {
                provider: LlmProviderKind::Ollama,
                model: "fake-embedder-768".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
            },
            Box::new(fake),
        )
        .await;
    rag::embed_corpus(&database, &registry).await.unwrap();
    let n_first: i64 = database
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM embeddings_meta", [], |r| r.get(0))
        .unwrap();
    rag::embed_corpus(&database, &registry).await.unwrap();
    let n_second: i64 = database
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM embeddings_meta", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        n_first, n_second,
        "re-embed should clear before re-inserting (no duplicates)",
    );
}
