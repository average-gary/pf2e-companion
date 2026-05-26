//! Vault writes — campaign + entity CRUD that goes through the markdown
//! filesystem. The watcher (vault.rs) picks the file change up and re-indexes
//! it; that's the canonical way the SQLite mirror stays current.
//!
//! Plan § 3.2 / § 4.1.

use crate::db::Db;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// App-state pointer to the user's vault root. Held alongside the Db Arc
/// in Tauri's State container.
pub struct VaultRoot(pub PathBuf);

#[derive(Debug, Serialize, Clone)]
pub struct Campaign {
    pub id: String,
    pub name: String,
    pub default_lens: Option<String>,
    pub entity_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct EntityInput {
    pub campaign_id: String,
    pub r#type: String,
    pub title: String,
    pub lens: Option<String>,
    pub license_provenance: Option<String>,
    pub body: Option<String>,
    /// Free-form extra frontmatter; merged into the YAML.
    #[serde(default)]
    pub frontmatter: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct CrudResult {
    pub id: String,
    pub file_path: String,
}

/// Sanitize a campaign or entity slug. Lowercase, hyphens, alphanumerics.
fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev = false;
        } else if !prev && !out.is_empty() {
            out.push('-');
            prev = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Reject anything that escapes the campaign root.
fn safe_segment(seg: &str) -> Result<()> {
    if seg.is_empty() || seg.contains('/') || seg.contains('\\') || seg == ".." || seg.starts_with('.') {
        bail!("unsafe path segment: {seg:?}");
    }
    Ok(())
}

pub fn list_campaigns(vault: &VaultRoot, db: &Db) -> Result<Vec<Campaign>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT campaign_id, COUNT(*) FROM entities
         WHERE source = 'vault'
         GROUP BY campaign_id
         ORDER BY campaign_id",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    drop(conn);

    let mut campaigns: Vec<Campaign> = rows
        .into_iter()
        .map(|(id, count)| Campaign {
            name: id.clone(),
            id,
            default_lens: None,
            entity_count: count,
        })
        .collect();

    // Surface campaign directories that exist on disk but are still empty
    // (newly created via create_campaign before any entity write).
    let campaigns_dir = vault.0.join("campaigns");
    if campaigns_dir.is_dir() {
        for entry in std::fs::read_dir(&campaigns_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = match path.file_name().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            if !campaigns.iter().any(|c| c.id == id) {
                campaigns.push(Campaign {
                    name: id.clone(),
                    id,
                    default_lens: None,
                    entity_count: 0,
                });
            }
        }
    }
    campaigns.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(campaigns)
}

pub fn create_campaign(
    vault: &VaultRoot,
    name: &str,
    default_lens: Option<&str>,
) -> Result<Campaign> {
    let id = slug(name);
    if id.is_empty() {
        bail!("campaign name produces an empty slug");
    }
    safe_segment(&id)?;
    let dir = vault.0.join("campaigns").join(&id);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    // Drop a manifest so the campaign survives even with zero entities.
    let manifest = dir.join("campaign.toml");
    if !manifest.exists() {
        let toml = format!(
            r#"# pf2e-companion campaign manifest
[campaign]
id = "{id}"
name = {name:?}
{lens_line}
created = "{}"
"#,
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            lens_line = default_lens
                .map(|l| format!(r#"default_lens = "{l}""#))
                .unwrap_or_default(),
        );
        std::fs::write(&manifest, toml)?;
    }

    Ok(Campaign {
        name: name.to_string(),
        id,
        default_lens: default_lens.map(String::from),
        entity_count: 0,
    })
}

/// Build a stable, on-disk-friendly id of the form `<campaign>:<type>/<slug>`.
fn entity_id(campaign: &str, type_: &str, title: &str) -> Result<String> {
    safe_segment(campaign)?;
    let type_slug = slug(type_);
    let title_slug = slug(title);
    if type_slug.is_empty() || title_slug.is_empty() {
        bail!("type and title must produce non-empty slugs");
    }
    safe_segment(&type_slug)?;
    safe_segment(&title_slug)?;
    Ok(format!("{campaign}:{type_slug}/{title_slug}"))
}

fn entity_file(vault: &VaultRoot, campaign: &str, type_: &str, title: &str) -> Result<PathBuf> {
    let type_slug = slug(type_);
    let title_slug = slug(title);
    safe_segment(campaign)?;
    safe_segment(&type_slug)?;
    safe_segment(&title_slug)?;
    Ok(vault
        .0
        .join("campaigns")
        .join(campaign)
        .join(plural(&type_slug))
        .join(format!("{title_slug}.md")))
}

fn plural(type_slug: &str) -> String {
    // Simple English-pluralization for path readability. NPCs → npcs/,
    // locations → locations/, classes → classes/. Order matters: the
    // -es rule wins for ch/sh/x/z; -ss takes -es too (class → classes);
    // bare -s words are treated as already-plural.
    let s = type_slug;
    if s.ends_with("ch") || s.ends_with("sh") || s.ends_with('x') || s.ends_with('z')
        || s.ends_with("ss")
    {
        format!("{s}es")
    } else if s.ends_with('s') {
        s.to_string()
    } else if s.ends_with('y') && s.len() > 1 {
        format!("{}ies", &s[..s.len() - 1])
    } else {
        format!("{s}s")
    }
}

pub fn create_entity(vault: &VaultRoot, input: &EntityInput) -> Result<CrudResult> {
    let id = entity_id(&input.campaign_id, &input.r#type, &input.title)?;
    let file = entity_file(vault, &input.campaign_id, &input.r#type, &input.title)?;
    if file.exists() {
        bail!(
            "{} already exists; use update_entity to overwrite",
            file.display()
        );
    }
    let body = input.body.as_deref().unwrap_or("").trim_end();
    let yaml = build_frontmatter_yaml(&id, input)?;
    let contents = format!("---\n{yaml}---\n\n{body}\n");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file, contents)
        .with_context(|| format!("write {}", file.display()))?;
    Ok(CrudResult {
        id,
        file_path: relative_to_vault(&file, &vault.0),
    })
}

#[derive(Debug, Deserialize)]
pub struct EntityPatch {
    pub title: Option<String>,
    pub lens: Option<String>,
    pub license_provenance: Option<String>,
    pub body: Option<String>,
    /// Replace the entire extras map. (Use a distinct command later if a
    /// merge-patch becomes necessary.)
    pub frontmatter: Option<serde_json::Map<String, Value>>,
}

pub fn update_entity(vault: &VaultRoot, db: &Db, id: &str, patch: &EntityPatch) -> Result<CrudResult> {
    let (campaign, type_, title, current_path) = lookup_entity(db, id)?;
    let new_title = patch.title.clone().unwrap_or_else(|| title.clone());
    let target_file = entity_file(vault, &campaign, &type_, &new_title)?;
    let new_id = if patch.title.is_some() {
        entity_id(&campaign, &type_, &new_title)?
    } else {
        id.to_string()
    };

    // Read the current file so we can preserve any frontmatter the user has
    // hand-edited that the patch doesn't touch.
    let current_disk = vault.0.join(&current_path);
    let raw = std::fs::read_to_string(&current_disk)
        .with_context(|| format!("read {}", current_disk.display()))?;
    let (fm_yaml, body_existing) = split_frontmatter(&raw);
    let mut fm: serde_yaml::Mapping = if fm_yaml.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(fm_yaml)?
    };
    fm.insert(
        serde_yaml::Value::from("id"),
        serde_yaml::Value::from(new_id.clone()),
    );
    if let Some(t) = patch.title.as_ref() {
        fm.insert("title".into(), t.clone().into());
    }
    if let Some(l) = patch.lens.as_ref() {
        fm.insert("lens".into(), l.clone().into());
    }
    if let Some(lp) = patch.license_provenance.as_ref() {
        validate_license(lp)?;
        fm.insert("license_provenance".into(), lp.clone().into());
    }
    if let Some(extra) = patch.frontmatter.as_ref() {
        for (k, v) in extra {
            // Don't allow patches to overwrite the canonical id or campaign.
            if k == "id" || k == "campaign_id" {
                continue;
            }
            let yv = serde_yaml::to_value(v)?;
            fm.insert(k.as_str().into(), yv);
        }
    }
    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(fm))?;
    let body = patch
        .body
        .clone()
        .unwrap_or_else(|| body_existing.to_string());
    let contents = format!("---\n{yaml}---\n\n{}\n", body.trim_end());

    if let Some(parent) = target_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if target_file != current_disk {
        std::fs::write(&target_file, contents)?;
        // Use rename-by-copy-then-delete so the watcher sees a Create on the
        // new file before a Remove on the old one — keeps SQLite consistent.
        std::fs::remove_file(&current_disk).ok();
    } else {
        std::fs::write(&target_file, contents)?;
    }

    Ok(CrudResult {
        id: new_id,
        file_path: relative_to_vault(&target_file, &vault.0),
    })
}

pub fn delete_entity(vault: &VaultRoot, db: &Db, id: &str) -> Result<()> {
    let (_, _, _, path) = lookup_entity(db, id)?;
    let disk = vault.0.join(path);
    if disk.exists() {
        std::fs::remove_file(&disk)?;
    }
    Ok(())
}

fn lookup_entity(db: &Db, id: &str) -> Result<(String, String, String, String)> {
    let conn = db.conn.lock().unwrap();
    let row = conn
        .query_row(
            "SELECT campaign_id, type, json_extract(frontmatter, '$.title'), file_path
             FROM entities
             WHERE id = ?1 AND source = 'vault'",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(|e| anyhow!("{id}: {e}"))?;
    Ok((row.0, row.1, row.2.unwrap_or_default(), row.3))
}

fn validate_license(s: &str) -> Result<()> {
    match s {
        "orc" | "community-use" | "homebrew" | "proprietary" => Ok(()),
        other => bail!(
            "license_provenance `{other}` not in [orc, community-use, homebrew, proprietary]"
        ),
    }
}

fn build_frontmatter_yaml(id: &str, input: &EntityInput) -> Result<String> {
    let mut m = serde_yaml::Mapping::new();
    m.insert("id".into(), id.into());
    m.insert("title".into(), input.title.clone().into());
    m.insert("type".into(), input.r#type.clone().into());
    m.insert("campaign_id".into(), input.campaign_id.clone().into());
    if let Some(l) = input.lens.as_ref() {
        m.insert("lens".into(), l.clone().into());
    }
    let lp = input
        .license_provenance
        .clone()
        .unwrap_or_else(|| "homebrew".into());
    validate_license(&lp)?;
    m.insert("license_provenance".into(), lp.into());
    for (k, v) in &input.frontmatter {
        if k == "id" || k == "campaign_id" || k == "title" || k == "type" {
            continue; // canonical fields are owned by the IPC, not the patch
        }
        let yv = serde_yaml::to_value(v)?;
        m.insert(k.as_str().into(), yv);
    }
    Ok(serde_yaml::to_string(&serde_yaml::Value::Mapping(m))?)
}

fn split_frontmatter(s: &str) -> (&str, &str) {
    let trimmed = s.trim_start_matches('\u{FEFF}');
    if !trimmed.starts_with("---") {
        return ("", trimmed);
    }
    let after = &trimmed[3..].trim_start_matches(['\r', '\n']);
    if let Some(end) = after.find("\n---") {
        let yaml = &after[..end];
        let rest = &after[end + 4..];
        let body = rest.trim_start_matches(['\r', '\n']);
        (yaml, body)
    } else {
        ("", trimmed)
    }
}

fn relative_to_vault(path: &Path, vault: &Path) -> String {
    path.strip_prefix(vault)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

// === Relations =============================================================

#[derive(Debug, Serialize)]
pub struct RelationRow {
    pub from_id: String,
    pub edge_type: String,
    pub to_id: String,
}

pub fn add_relation(db: &Db, from_id: &str, edge_type: &str, to_id: &str) -> Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO relations (from_id, edge_type, to_id, properties)
         VALUES (?1, ?2, ?3, NULL)",
        params![from_id, edge_type, to_id],
    )?;
    Ok(())
}

pub fn delete_relation(db: &Db, from_id: &str, edge_type: &str, to_id: &str) -> Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "DELETE FROM relations WHERE from_id = ?1 AND edge_type = ?2 AND to_id = ?3",
        params![from_id, edge_type, to_id],
    )?;
    Ok(())
}

pub fn list_relations(db: &Db, entity_id: &str) -> Result<Vec<RelationRow>> {
    let conn = db.conn.lock().unwrap();
    let mut out = Vec::new();
    let mut outgoing = conn.prepare(
        "SELECT from_id, edge_type, to_id FROM relations WHERE from_id = ?1",
    )?;
    out.extend(
        outgoing
            .query_map(params![entity_id], |r| {
                Ok(RelationRow {
                    from_id: r.get(0)?,
                    edge_type: r.get(1)?,
                    to_id: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut incoming = conn.prepare(
        "SELECT from_id, edge_type, to_id FROM relations WHERE to_id = ?1",
    )?;
    out.extend(
        incoming
            .query_map(params![entity_id], |r| {
                Ok(RelationRow {
                    from_id: r.get(0)?,
                    edge_type: r.get(1)?,
                    to_id: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(out)
}

/// Convenience for tests.
#[cfg(test)]
pub fn vault_root(vault: &VaultRoot) -> &Path {
    &vault.0
}

#[allow(dead_code)] // keeps the Arc bound type-checked even before the
                    // Tauri State machinery wires it.
pub type SharedVaultRoot = Arc<VaultRoot>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_basic() {
        assert_eq!(slug("Lord Cassian!"), "lord-cassian");
        assert_eq!(slug("House Velerian"), "house-velerian");
        assert_eq!(slug("YHWH (Lord of Hosts)"), "yhwh-lord-of-hosts");
    }

    #[test]
    fn safe_segment_rejects_traversal() {
        assert!(safe_segment("..").is_err());
        assert!(safe_segment(".hidden").is_err());
        assert!(safe_segment("a/b").is_err());
        assert!(safe_segment("ok").is_ok());
    }

    #[test]
    fn pluralizes_paths() {
        assert_eq!(plural("npc"), "npcs");
        assert_eq!(plural("location"), "locations");
        assert_eq!(plural("city"), "cities");
        assert_eq!(plural("class"), "classes");
        assert_eq!(plural("notes"), "notes"); // already plural
    }

    #[test]
    fn validate_license_strict() {
        assert!(validate_license("orc").is_ok());
        assert!(validate_license("community-use").is_ok());
        assert!(validate_license("OGL").is_err());
    }
}
