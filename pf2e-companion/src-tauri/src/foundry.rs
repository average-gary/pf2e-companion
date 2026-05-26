//! Foundry VTT pf2e schema ingest (read-only).
//!
//! Phase 1 ships the importer skeleton: it understands the directory shape
//! of `foundryvtt/pf2e/packs/<pack>/<entity>.json`, validates each file as
//! JSON, and inserts a `entities` row tagged `source = 'reference'` with
//! `license_provenance = 'orc'` for Remaster packs (and `community-use` for
//! pre-Remaster Bestiaries that ship under OGL — caller decides per pack).
//!
//! Detailed field mapping (action economy, traits, etc.) is deferred to the
//! reference-layer phases that build the statblock renderer.

use crate::db::Db;
use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicensePosture {
    Orc,
    CommunityUse,
    Homebrew,
    Proprietary,
}

impl LicensePosture {
    pub fn as_str(self) -> &'static str {
        match self {
            LicensePosture::Orc => "orc",
            LicensePosture::CommunityUse => "community-use",
            LicensePosture::Homebrew => "homebrew",
            LicensePosture::Proprietary => "proprietary",
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    pub files_seen: usize,
    pub files_imported: usize,
    pub files_skipped: usize,
    pub errors: Vec<String>,
}

/// Walk a Foundry pf2e packs directory and import every JSON file as a
/// single `entities` row. The directory layout we expect is
/// `<root>/packs/<pack-name>/*.json` (extracted from the Foundry repo or
/// the published level-db). If the user points us at a single pack
/// directly, we accept that too.
pub fn import_packs(
    db: &Db,
    root: &Path,
    license: LicensePosture,
) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let candidates = collect_json_files(root);
    report.files_seen = candidates.len();
    if candidates.is_empty() {
        return Ok(report);
    }

    let mut conn = db.conn.lock().unwrap();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            r#"
            INSERT INTO entities
              (id, type, campaign_id, source, lens, license_provenance,
               frontmatter, body, body_text, statblock, file_path, mtime, hash)
            VALUES (?1, ?2, '_reference', 'reference', NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
              type = excluded.type,
              license_provenance = excluded.license_provenance,
              frontmatter = excluded.frontmatter,
              body = excluded.body,
              body_text = excluded.body_text,
              statblock = excluded.statblock,
              file_path = excluded.file_path,
              mtime = excluded.mtime,
              hash = excluded.hash
            "#,
        )?;

        for path in &candidates {
            match ingest_one(&path) {
                Ok(rec) => {
                    stmt.execute(params![
                        rec.id,
                        rec.entity_type,
                        license.as_str(),
                        rec.frontmatter_json,
                        rec.body,
                        rec.body_text,
                        rec.statblock_json,
                        rec.rel_path,
                        rec.mtime,
                        rec.hash,
                    ])?;
                    report.files_imported += 1;
                }
                Err(e) => {
                    report.errors.push(format!("{}: {e}", path.display()));
                    report.files_skipped += 1;
                }
            }
        }
    }
    tx.commit()?;
    tracing::info!(
        seen = report.files_seen,
        imported = report.files_imported,
        skipped = report.files_skipped,
        "foundry import"
    );
    Ok(report)
}

struct OneRecord {
    id: String,
    entity_type: String,
    frontmatter_json: String,
    body: String,
    body_text: String,
    statblock_json: String,
    rel_path: String,
    mtime: i64,
    hash: String,
}

fn ingest_one(path: &Path) -> Result<OneRecord> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))?;

    // Foundry pf2e records: `_id` (16-char ID), `name`, `type` (npc | spell | feat | item | ...).
    let foundry_id = value
        .get("_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("(unnamed)")
        .to_string();
    let entity_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let id = match foundry_id {
        Some(fid) => format!("foundry:{fid}"),
        None => format!("foundry:{}", super_slug(&name)),
    };

    let description = extract_description(&value);
    let body_text = description.clone();

    let frontmatter = serde_json::json!({
        "title": name,
        "type": entity_type,
        "source": "foundry-pf2e",
    });

    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(OneRecord {
        id,
        entity_type,
        frontmatter_json: frontmatter.to_string(),
        body: description,
        body_text,
        statblock_json: raw.clone(),
        rel_path: path.to_string_lossy().into_owned(),
        mtime,
        hash: short_hash(&raw),
    })
}

fn collect_json_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        if root.extension().and_then(|s| s.to_str()) == Some("json") {
            out.push(root.to_path_buf());
        }
        return out;
    }
    let _ = walk(root, &mut |p: &Path| {
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            out.push(p.to_path_buf());
        }
    });
    out
}

fn walk(path: &Path, on_file: &mut dyn FnMut(&Path)) -> Result<()> {
    if path.is_file() {
        on_file(path);
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();
        if p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }
        walk(&p, on_file)?;
    }
    Ok(())
}

fn extract_description(value: &Value) -> String {
    value
        .pointer("/system/details/description/value")
        .and_then(|v| v.as_str())
        .or_else(|| {
            value
                .pointer("/system/description/value")
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string()
}

fn super_slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn short_hash(s: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundry_record_extraction() {
        let raw = r#"{
          "_id": "ABC1234567890XYZ",
          "name": "Force Barrage",
          "type": "spell",
          "system": {"description": {"value": "<p>Three magical missiles…</p>"}}
        }"#;
        let v: Value = serde_json::from_str(raw).unwrap();
        let desc = extract_description(&v);
        assert!(desc.contains("magical missiles"));
    }

    #[test]
    fn license_posture_strings() {
        assert_eq!(LicensePosture::Orc.as_str(), "orc");
        assert_eq!(LicensePosture::CommunityUse.as_str(), "community-use");
    }
}
