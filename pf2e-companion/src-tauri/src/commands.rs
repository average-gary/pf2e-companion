//! Capability-gated IPC commands. Phase 0 ships a minimal subset
//! sufficient for the smoke test; the full surface is documented in
//! the plan spec § 3.

use crate::db::Db;
use crate::foundry::{self, ImportReport, LicensePosture};
use crate::rules::{self, Difficulty};
use anyhow::Result;
use rusqlite::params;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize, Debug)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub r#type: String,
    pub snippet: String,
    pub score: f64,
}

/// Hybrid search. Phase 0 = FTS5 only; vector path lights up in Phase 6 when
/// the LLM layer is wired (see plan § 4.2).
#[tauri::command]
pub fn search(
    query: String,
    lens: Option<String>,
    db: State<'_, Arc<Db>>,
) -> Result<Vec<SearchHit>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let lens = lens.unwrap_or_else(|| "lewisian".to_string());

    let mut stmt = conn
        .prepare(
            r#"
            SELECT
                e.id,
                json_extract(e.frontmatter, '$.title') AS title,
                e.type,
                snippet(entities_fts, 1, '<mark>', '</mark>', '...', 12) AS snippet,
                bm25(entities_fts) AS score
            FROM entities_fts
            JOIN entities e ON e.rowid = entities_fts.rowid
            WHERE entities_fts MATCH ?1
              AND (e.lens IS NULL OR e.lens = ?2)
            ORDER BY score
            LIMIT 50
            "#,
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![query, lens], |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r#type: row.get(2)?,
                snippet: row.get(3)?,
                score: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
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
