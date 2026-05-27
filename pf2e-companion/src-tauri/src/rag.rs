//! Retrieval-Augmented Generation: chunk + embed + hybrid search.
//!
//! Phase 6 § C lights up the `entities_vec` (sqlite-vec) virtual table the
//! Phase 0 schema reserved. Two surfaces:
//!
//! 1. `embed_corpus()` — paragraph-chunk every entity body, embed via the
//!    active LLM provider, write float[768] vectors to `entities_vec`
//!    keyed by rowid, and mirror the (entity_id, chunk_idx, chunk_text,
//!    provider, model) tuple into `embeddings_meta`. Idempotent: clears
//!    prior rows first so re-running with a different model is safe.
//!
//! 2. `hybrid_search()` — when the LLM is configured AND the corpus is
//!    embedded, fuse FTS5 and vector results via reciprocal-rank fusion
//!    (RRF, k=60) and return the top 50 hits. When either condition is
//!    false, fall through to FTS-only — the function's signature is
//!    transparent to callers.
//!
//! Embedding dimensionality is fixed at 768. If the active provider's
//! model returns a different dimension, embedding fails loudly so the
//! user can switch models rather than silently storing junk.

use crate::db::Db;
use crate::llm::{LlmProviderKind, LlmRegistry};
use anyhow::{anyhow, Result};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashMap;

pub const EMBED_DIM: usize = 768;

/// Reciprocal-rank-fusion constant. The standard k=60 from Cormack-Clarke-Buettcher.
const RRF_K: f64 = 60.0;

/// Approximate paragraph chunker. Splits on blank lines (`\n\n`), then
/// re-joins consecutive small paragraphs so each chunk lands in the
/// 200-500-token window the plan calls for. We approximate "tokens" as
/// `chars / 4` — close enough for English markdown without pulling in a
/// real tokenizer.
pub fn chunk_paragraphs(body: &str) -> Vec<String> {
    const MIN_CHARS: usize = 600;   // ≈150 tokens — under this, merge with next
    const MAX_CHARS: usize = 2000;  // ≈500 tokens — over this, split

    let raw: Vec<String> = body
        .split("\n\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if raw.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for p in raw {
        if buf.is_empty() {
            buf = p;
        } else if buf.len() < MIN_CHARS {
            buf.push_str("\n\n");
            buf.push_str(&p);
        } else {
            out.push(buf);
            buf = p;
        }

        // If we've crossed MAX_CHARS, flush. (Subsequent paragraphs start
        // a new buffer.)
        if buf.len() >= MAX_CHARS {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Pack a Vec<f32> as little-endian bytes for `entities_vec`. sqlite-vec
/// accepts any blob whose byte length matches `EMBED_DIM * 4`.
fn embedding_to_blob(vec: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vec.len() * 4);
    for f in vec {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

#[derive(Serialize, Debug, Clone)]
pub struct EmbedReport {
    pub provider: String,
    pub model: String,
    pub entities_processed: usize,
    pub chunks_embedded: usize,
}

/// Re-embed the entire corpus with the currently active provider's
/// embedding model. Wipes prior `entities_vec` + `embeddings_meta` rows
/// first; this is the single source of truth for the index.
pub async fn embed_corpus(db: &Db, registry: &LlmRegistry) -> Result<EmbedReport> {
    // Borrow the provider for the lifetime of the call. We can't hold an
    // Arc across await without `LlmRegistry` exposing one, so we copy
    // out enough metadata (provider id, model) plus a clone of the inner
    // `Box<dyn LlmProvider>`. Since `LlmProvider` requires `Send + Sync`,
    // the simpler path is: snapshot the provider id + model under the
    // read guard, then re-resolve once per batch via the registry's
    // read() helper.
    let (provider_id, model) = {
        let guard = registry.read().await;
        let active = guard
            .as_ref()
            .ok_or_else(|| anyhow!("llm provider not configured"))?;
        (
            active.provider().id().to_string(),
            active.provider().model().to_string(),
        )
    };

    // Anthropic doesn't ship embeddings — fail fast with a useful hint.
    if provider_id == "anthropic" {
        return Err(anyhow!(
            "anthropic does not provide embeddings; configure ollama at /settings/llm \
             and pull an embedding model (e.g. `ollama pull nomic-embed-text`) before reindexing"
        ));
    }

    // Snapshot every entity body the embedding will run over. Holding the
    // Mutex across awaits would deadlock the IPC handler that called us;
    // we collect first, release, then embed.
    let rows: Vec<(String, String)> = {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(body_text, body, '') FROM entities WHERE source = 'reference'",
        )?;
        let rows: Vec<_> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<_, _>>()?;
        rows
    };

    // Wipe prior index (provider/model swap = full re-embed; corruption is
    // worse than a few minutes of waiting).
    {
        let conn = db.conn.lock().unwrap();
        conn.execute("DELETE FROM entities_vec", [])?;
        conn.execute("DELETE FROM embeddings_meta", [])?;
    }

    // Walk entities; chunk; embed in batches of 16 to amortize HTTP overhead.
    const BATCH: usize = 16;
    let mut entities_processed = 0usize;
    let mut chunks_embedded = 0usize;
    let mut pending: Vec<(String, usize, String)> = Vec::with_capacity(BATCH);

    for (entity_id, body) in rows {
        let chunks = chunk_paragraphs(&body);
        if chunks.is_empty() {
            continue;
        }
        entities_processed += 1;
        for (idx, chunk) in chunks.into_iter().enumerate() {
            pending.push((entity_id.clone(), idx, chunk));
            if pending.len() >= BATCH {
                flush_batch(db, registry, &provider_id, &model, &mut pending).await?;
                chunks_embedded += BATCH;
            }
        }
    }
    if !pending.is_empty() {
        let n = pending.len();
        flush_batch(db, registry, &provider_id, &model, &mut pending).await?;
        chunks_embedded += n;
    }

    tracing::info!(
        provider = %provider_id,
        model = %model,
        entities = entities_processed,
        chunks = chunks_embedded,
        "rag corpus embedded",
    );

    Ok(EmbedReport {
        provider: provider_id,
        model,
        entities_processed,
        chunks_embedded,
    })
}

async fn flush_batch(
    db: &Db,
    registry: &LlmRegistry,
    provider_id: &str,
    model: &str,
    pending: &mut Vec<(String, usize, String)>,
) -> Result<()> {
    let texts: Vec<String> = pending.iter().map(|(_, _, c)| c.clone()).collect();
    let vectors = {
        let guard = registry.read().await;
        let active = guard
            .as_ref()
            .ok_or_else(|| anyhow!("llm provider not configured during embed"))?;
        active.provider().embed(texts).await?
    };
    if vectors.len() != pending.len() {
        return Err(anyhow!(
            "embed returned {} vectors for {} chunks",
            vectors.len(),
            pending.len()
        ));
    }
    let conn = db.conn.lock().unwrap();
    let mut ins_meta = conn.prepare(
        "INSERT INTO embeddings_meta (entity_id, chunk_idx, chunk_text, provider, model)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut ins_vec = conn.prepare("INSERT INTO entities_vec (rowid, embedding) VALUES (?1, ?2)")?;
    for ((entity_id, chunk_idx, chunk_text), vec) in pending.iter().zip(vectors) {
        if vec.len() != EMBED_DIM {
            return Err(anyhow!(
                "embedding dimension mismatch: model `{model}` returned {} dims, expected {EMBED_DIM}. \
                 Use a 768-dim model (e.g. `nomic-embed-text`).",
                vec.len(),
            ));
        }
        ins_meta.execute(params![entity_id, *chunk_idx as i64, chunk_text, provider_id, model])?;
        let rowid = conn.last_insert_rowid();
        ins_vec.execute(params![rowid, embedding_to_blob(&vec)])?;
    }
    pending.clear();
    Ok(())
}

#[derive(Serialize, Debug, Clone)]
pub struct HybridHit {
    pub id: String,
    pub title: String,
    pub r#type: String,
    pub snippet: String,
    pub score: f64,
    /// "fts" if only FTS surfaced it, "vec" if only vector, "both" if fused.
    pub source: &'static str,
}

#[derive(Debug, Clone, Default)]
struct RankRow {
    fts_rank: Option<usize>,
    vec_rank: Option<usize>,
    title: Option<String>,
    r#type: Option<String>,
    snippet: Option<String>,
}

/// Hybrid search combining FTS5 and vector retrieval. Falls back to FTS
/// only when (a) the LLM provider isn't configured, (b) the corpus
/// hasn't been indexed yet, or (c) the embed call fails for any reason.
pub async fn hybrid_search(
    db: &Db,
    registry: &LlmRegistry,
    query: &str,
    lens: &str,
) -> Result<Vec<HybridHit>> {
    let fts_rows = fts_search(db, query, lens)?;

    // Vector half — best-effort. Any failure here should degrade
    // gracefully to FTS-only rather than turning search into an error.
    let vec_rows: Vec<(String, String, String, String)> = match vector_search(db, registry, query, lens).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::debug!(error = %e, "vector half of hybrid_search disabled");
            Vec::new()
        }
    };

    // RRF fusion. Map id → rank info; FTS rank is array index in fts_rows,
    // vec rank likewise.
    let mut fused: HashMap<String, RankRow> = HashMap::new();
    for (rank, hit) in fts_rows.iter().enumerate() {
        let row = fused.entry(hit.0.clone()).or_default();
        row.fts_rank = Some(rank);
        row.title = Some(hit.1.clone());
        row.r#type = Some(hit.2.clone());
        row.snippet = Some(hit.3.clone());
    }
    for (rank, hit) in vec_rows.iter().enumerate() {
        let row = fused.entry(hit.0.clone()).or_default();
        row.vec_rank = Some(rank);
        row.title.get_or_insert(hit.1.clone());
        row.r#type.get_or_insert(hit.2.clone());
        row.snippet.get_or_insert(hit.3.clone());
    }

    let mut hits: Vec<HybridHit> = fused
        .into_iter()
        .map(|(id, r)| {
            let fts_score = r.fts_rank.map(|r| 1.0 / (RRF_K + r as f64)).unwrap_or(0.0);
            let vec_score = r.vec_rank.map(|r| 1.0 / (RRF_K + r as f64)).unwrap_or(0.0);
            let source = match (r.fts_rank.is_some(), r.vec_rank.is_some()) {
                (true, true) => "both",
                (true, false) => "fts",
                (false, true) => "vec",
                (false, false) => "none",
            };
            HybridHit {
                id,
                title: r.title.unwrap_or_default(),
                r#type: r.r#type.unwrap_or_default(),
                snippet: r.snippet.unwrap_or_default(),
                score: fts_score + vec_score,
                source,
            }
        })
        .collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(50);
    Ok(hits)
}

/// FTS-only search returning (id, title, type, snippet) sorted by bm25.
fn fts_search(
    db: &Db,
    query: &str,
    lens: &str,
) -> Result<Vec<(String, String, String, String)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        r#"
        SELECT
            e.id,
            COALESCE(json_extract(e.frontmatter, '$.title'), e.id) AS title,
            e.type,
            snippet(entities_fts, 1, '<mark>', '</mark>', '...', 12) AS snippet
        FROM entities_fts
        JOIN entities e ON e.rowid = entities_fts.rowid
        WHERE entities_fts MATCH ?1
          AND (e.lens IS NULL OR e.lens = ?2)
        ORDER BY bm25(entities_fts)
        LIMIT 50
        "#,
    )?;
    let rows = stmt
        .query_map(params![query, lens], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Vector-only search returning (entity_id, title, type, snippet) sorted
/// by L2 distance. The snippet is the chunk text itself (not the FTS
/// highlight), which is more informative for paraphrase queries.
async fn vector_search(
    db: &Db,
    registry: &LlmRegistry,
    query: &str,
    lens: &str,
) -> Result<Vec<(String, String, String, String)>> {
    // Quick precondition: index must exist.
    {
        let conn = db.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM embeddings_meta", [], |r| r.get(0))?;
        if n == 0 {
            return Err(anyhow!("corpus not indexed (embeddings_meta is empty)"));
        }
    }

    // Embed the query under the active provider. We accept Anthropic-as-
    // chat + Ollama-as-embed in the registry-configured-as-Ollama case
    // only; if the user is on Anthropic, fail (caller will fall back to
    // FTS).
    let kind = registry.current_kind().await;
    if !matches!(kind, Some(LlmProviderKind::Ollama)) {
        return Err(anyhow!(
            "vector search requires an ollama-configured provider for embeddings"
        ));
    }
    let qvec: Vec<f32> = {
        let guard = registry.read().await;
        let active = guard.as_ref().ok_or_else(|| anyhow!("provider missing mid-call"))?;
        let mut v = active.provider().embed(vec![query.to_string()]).await?;
        v.pop().ok_or_else(|| anyhow!("embed returned no vectors"))?
    };
    if qvec.len() != EMBED_DIM {
        return Err(anyhow!(
            "query embedding has {} dims, expected {EMBED_DIM}",
            qvec.len()
        ));
    }

    let blob = embedding_to_blob(&qvec);
    let conn = db.conn.lock().unwrap();
    // KNN over entities_vec, then join through embeddings_meta to entities.
    // De-dupe by entity_id keeping the best-ranking chunk.
    let mut stmt = conn.prepare(
        r#"
        SELECT
            em.entity_id,
            COALESCE(json_extract(e.frontmatter, '$.title'), e.id) AS title,
            e.type,
            em.chunk_text
        FROM entities_vec
        JOIN embeddings_meta em ON em.rowid = entities_vec.rowid
        JOIN entities        e  ON e.id    = em.entity_id
        WHERE entities_vec.embedding MATCH ?1
          AND k = 100
          AND (e.lens IS NULL OR e.lens = ?2)
        ORDER BY entities_vec.distance
        "#,
    )?;
    let raw: Vec<(String, String, String, String)> = stmt
        .query_map(params![blob, lens], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    // Dedupe preserving order — first occurrence (closest) wins.
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(raw.len());
    for row in raw {
        if seen.insert(row.0.clone()) {
            out.push(row);
        }
        if out.len() >= 50 {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_paragraphs_empty_body_yields_nothing() {
        assert!(chunk_paragraphs("").is_empty());
        assert!(chunk_paragraphs("   \n\n   ").is_empty());
    }

    #[test]
    fn chunk_paragraphs_short_body_is_one_chunk() {
        let body = "Just a single paragraph of text that's quite short.";
        let chunks = chunk_paragraphs(body);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], body);
    }

    #[test]
    fn chunk_paragraphs_merges_small_paragraphs() {
        // Three small paragraphs should fold into one chunk under MIN_CHARS.
        let body = "Para one is short.\n\nPara two is short.\n\nPara three is short.";
        let chunks = chunk_paragraphs(body);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("one"));
        assert!(chunks[0].contains("three"));
    }

    #[test]
    fn chunk_paragraphs_splits_oversized() {
        // One huge paragraph well past MAX_CHARS forms its own chunk.
        let big = "x".repeat(2500);
        let body = format!("{big}\n\nSecond para is short.");
        let chunks = chunk_paragraphs(&body);
        assert_eq!(chunks.len(), 2, "oversized paragraph should split");
        assert!(chunks[0].len() >= 2000);
    }

    #[test]
    fn rrf_score_orders_correctly() {
        // FTS rank 0 + vec rank 0  → score = 2 * 1/60 ≈ 0.0333
        // FTS rank 0 only           → 1/60 ≈ 0.0167
        // FTS rank 5 only           → 1/65 ≈ 0.0154
        let a = 1.0 / RRF_K + 1.0 / RRF_K;
        let b = 1.0 / RRF_K;
        let c = 1.0 / (RRF_K + 5.0);
        assert!(a > b);
        assert!(b > c);
    }

    #[test]
    fn embedding_to_blob_roundtrip() {
        let v: Vec<f32> = vec![1.0, -2.5, 3.14, 0.0];
        let blob = embedding_to_blob(&v);
        assert_eq!(blob.len(), v.len() * 4);
        // Decode back.
        let mut decoded = Vec::with_capacity(v.len());
        for chunk in blob.chunks_exact(4) {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            decoded.push(f32::from_le_bytes(arr));
        }
        assert_eq!(decoded, v);
    }
}
