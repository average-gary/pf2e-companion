//! SQLite + FTS5 + sqlite-vec storage layer.
//!
//! Schema follows the spec in
//! `~/wiki/topics/pf2e-worldbuilding-tool/output/plan-cross-platform-pf2e-biblical-reference-2026-05-25.md`
//! § 2.4. Single .db file inside the user's vault folder per
//! `desktop-app-stack-recommendation.md`.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::path::Path;
use std::sync::{Mutex, Once};

pub struct Db {
    pub conn: Mutex<Connection>,
}

const REMASTER_ALIASES_JSON: &str =
    include_str!("../data/seeds/remaster_aliases.json");
const MIRACLE_SPELL_MAP_JSON: &str =
    include_str!("../data/seeds/miracle_spell_map.json");

#[derive(Deserialize)]
struct AliasSeed {
    legacy_name: String,
    remaster_name: String,
    category: String,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct MiracleSeed {
    miracle: String,
    reference: String,
    book: String,
    spell_name: String,
    tradition: Option<String>,
    sanctification: Option<String>,
    notes: Option<String>,
}

/// Register sqlite-vec via sqlite3_auto_extension exactly once per process.
/// All subsequent Connection::open calls inherit the vec0 virtual table.
/// Adapted from sqlite-vec 0.1.9's own integration test; rusqlite 0.32 types
/// `sqlite3_auto_extension` with the full SQLite-API signature so we transmute
/// to that exact shape.
fn ensure_vec_extension_registered() {
    use rusqlite::ffi::{sqlite3, sqlite3_api_routines};
    type SqliteAutoExtension = unsafe extern "C" fn(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut std::os::raw::c_char,
        p_thunk: *const sqlite3_api_routines,
    ) -> std::os::raw::c_int;

    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        let init: unsafe extern "C" fn() = sqlite_vec::sqlite3_vec_init;
        let init: SqliteAutoExtension = std::mem::transmute(init);
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    });
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        ensure_vec_extension_registered();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite db at {}", path.display()))?;
        let mut db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(SCHEMA_V1)?;
        Ok(())
    }

    /// Insert a single fixture entity for the smoke test (Phase 0 validation).
    pub fn seed_smoke_fixture(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entities WHERE id = ?1",
            params!["smoke-yhwh"],
            |r| r.get(0),
        )?;
        if already > 0 {
            return Ok(());
        }
        conn.execute(
            r#"
            INSERT INTO entities
              (id, type, campaign_id, source, lens, license_provenance,
               frontmatter, body, body_text, statblock, file_path, mtime, hash)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, 0, ?11)
            "#,
            params![
                "smoke-yhwh",
                "deity",
                "_reference",
                "reference",
                "lewisian",
                "orc",
                r#"{"title":"YHWH (Lord of Hosts)","type":"deity","lens":"lewisian"}"#,
                "The Lord of Hosts. Smoke-test fixture; replace with the real Lewisian YHWH entry in Phase 2.",
                "The Lord of Hosts. Smoke-test fixture; replace with the real Lewisian YHWH entry in Phase 2.",
                "reference/biblical/lewisian/deities/yhwh.md",
                "phase-0-smoke-fixture",
            ],
        )?;
        // Refresh the FTS index for this row.
        conn.execute(
            "INSERT INTO entities_fts(rowid, title, body_text)
             SELECT rowid,
                    json_extract(frontmatter, '$.title') AS title,
                    body_text
             FROM entities WHERE id = ?1",
            params!["smoke-yhwh"],
        )?;
        Ok(())
    }

    /// Phase 1 reference data: load the bundled JSON seeds into SQLite
    /// (idempotent — `INSERT OR REPLACE`).
    pub fn seed_reference_data(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let aliases: Vec<AliasSeed> =
            serde_json::from_str(REMASTER_ALIASES_JSON).context("parse aliases seed")?;
        let miracles: Vec<MiracleSeed> =
            serde_json::from_str(MIRACLE_SPELL_MAP_JSON).context("parse miracles seed")?;

        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO remaster_aliases
                 (legacy_name, remaster_name, category, notes)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for a in &aliases {
                stmt.execute(params![
                    a.legacy_name,
                    a.remaster_name,
                    a.category,
                    a.notes,
                ])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO miracle_spell_map
                 (miracle, reference, book, spell_id, spell_name, tradition,
                  sanctification, notes)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7)",
            )?;
            for m in &miracles {
                stmt.execute(params![
                    m.miracle,
                    m.reference,
                    m.book,
                    m.spell_name,
                    m.tradition,
                    m.sanctification,
                    m.notes,
                ])?;
            }
        }
        tx.commit()?;
        tracing::info!(
            aliases = aliases.len(),
            miracles = miracles.len(),
            "seeded reference data",
        );
        Ok(())
    }
}

const SCHEMA_V1: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS entities (
    id                  TEXT PRIMARY KEY,
    type                TEXT NOT NULL,
    campaign_id         TEXT NOT NULL,
    source              TEXT NOT NULL CHECK (source IN ('vault', 'reference', 'plugin')) ,
    lens                TEXT,
    license_provenance  TEXT NOT NULL CHECK (license_provenance IN ('orc', 'community-use', 'homebrew', 'proprietary')),
    frontmatter         TEXT NOT NULL,
    body                TEXT,
    body_text           TEXT,
    statblock           TEXT,
    file_path           TEXT NOT NULL UNIQUE,
    mtime               INTEGER NOT NULL,
    hash                TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entities_type        ON entities(type);
CREATE INDEX IF NOT EXISTS idx_entities_campaign    ON entities(campaign_id);
CREATE INDEX IF NOT EXISTS idx_entities_lens        ON entities(lens);
CREATE INDEX IF NOT EXISTS idx_entities_source      ON entities(source);

CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    title,
    body_text,
    tokenize='porter unicode61'
);

CREATE VIRTUAL TABLE IF NOT EXISTS entities_vec USING vec0(
    embedding float[768]
);

CREATE TABLE IF NOT EXISTS relations (
    from_id     TEXT NOT NULL,
    edge_type   TEXT NOT NULL,
    to_id       TEXT NOT NULL,
    properties  TEXT,
    PRIMARY KEY (from_id, edge_type, to_id),
    FOREIGN KEY (from_id) REFERENCES entities(id) ON DELETE CASCADE,
    FOREIGN KEY (to_id)   REFERENCES entities(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_relations_to ON relations(to_id);

CREATE TABLE IF NOT EXISTS remaster_aliases (
    legacy_name     TEXT NOT NULL,
    remaster_name   TEXT NOT NULL,
    category        TEXT NOT NULL,
    notes           TEXT,
    PRIMARY KEY (legacy_name, remaster_name)
);
-- Seeded from `pf2e-remaster-name-mapping.md` (~330 pairs) in Phase 1.

CREATE TABLE IF NOT EXISTS miracle_spell_map (
    miracle         TEXT PRIMARY KEY,
    reference       TEXT NOT NULL,
    book            TEXT NOT NULL,
    spell_id        TEXT,
    spell_name      TEXT NOT NULL,
    tradition       TEXT,
    sanctification  TEXT,
    notes           TEXT
);
CREATE INDEX IF NOT EXISTS idx_miracle_reference ON miracle_spell_map(reference);
CREATE INDEX IF NOT EXISTS idx_miracle_book      ON miracle_spell_map(book);
-- Seeded from `biblical-miracle-to-pf2e-spell-map.md` in Phase 1.

CREATE TABLE IF NOT EXISTS settings (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_alias_remaster ON remaster_aliases(remaster_name);
CREATE INDEX IF NOT EXISTS idx_alias_category ON remaster_aliases(category);

-- Phase 6 § C: per-chunk metadata for `entities_vec`. The vec0 virtual
-- table holds only the float[768] embedding; we keep the (entity_id,
-- chunk_idx, chunk_text, provider, model) tuple here, joined on rowid.
CREATE TABLE IF NOT EXISTS embeddings_meta (
    rowid       INTEGER PRIMARY KEY,
    entity_id   TEXT NOT NULL,
    chunk_idx   INTEGER NOT NULL,
    chunk_text  TEXT NOT NULL,
    provider    TEXT NOT NULL,
    model       TEXT NOT NULL,
    FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_embeddings_entity   ON embeddings_meta(entity_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_provider ON embeddings_meta(provider, model);

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY
);
INSERT OR IGNORE INTO schema_version (version) VALUES (3);
"#;
