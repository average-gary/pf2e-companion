//! Phase 3 smoke tests: vault CRUD round-trip + Foundry-pf2e e2e ingest.

#[path = "../src/db.rs"]
mod db;
#[path = "../src/foundry.rs"]
mod foundry;
#[path = "../src/vault_write.rs"]
mod vault_write;

use foundry::LicensePosture;
use rusqlite::params;
use std::fs;
use tempfile::TempDir;
use vault_write::{EntityInput, EntityPatch, VaultRoot};

#[test]
fn campaigns_crud_round_trip() {
    let tmp = TempDir::new().unwrap();
    let dbf = tmp.path().join("p3.db");
    let database = db::Db::open(&dbf).unwrap();

    let vault_dir = tmp.path().join("vault");
    fs::create_dir_all(&vault_dir).unwrap();
    let vault = VaultRoot(vault_dir.clone());

    // Empty vault → no campaigns yet.
    let camps = vault_write::list_campaigns(&vault, &database).unwrap();
    assert!(camps.is_empty(), "fresh vault should be empty");

    // Create a campaign — the manifest file appears on disk.
    let c = vault_write::create_campaign(&vault, "Burning Bush Saga", Some("lewisian")).unwrap();
    assert_eq!(c.id, "burning-bush-saga");
    assert_eq!(c.default_lens.as_deref(), Some("lewisian"));
    let manifest = vault_dir
        .join("campaigns")
        .join("burning-bush-saga")
        .join("campaign.toml");
    assert!(manifest.exists(), "manifest written to disk");
    let manifest_text = fs::read_to_string(&manifest).unwrap();
    assert!(manifest_text.contains("name = \"Burning Bush Saga\""));
    assert!(manifest_text.contains("default_lens = \"lewisian\""));

    // Empty campaign should still surface in list_campaigns.
    let camps = vault_write::list_campaigns(&vault, &database).unwrap();
    assert_eq!(camps.len(), 1);
    assert_eq!(camps[0].id, "burning-bush-saga");
    assert_eq!(camps[0].entity_count, 0);
}

#[test]
fn entity_create_update_round_trip_through_vault() {
    let tmp = TempDir::new().unwrap();
    let dbf = tmp.path().join("p3-entity.db");
    let database = db::Db::open(&dbf).unwrap();

    let vault_dir = tmp.path().join("vault");
    fs::create_dir_all(&vault_dir).unwrap();
    let vault = VaultRoot(vault_dir.clone());

    vault_write::create_campaign(&vault, "main", None).unwrap();

    let mut frontmatter = serde_json::Map::new();
    frontmatter.insert(
        "status".into(),
        serde_json::Value::String("budding".into()),
    );

    let input = EntityInput {
        campaign_id: "main".into(),
        r#type: "npc".into(),
        title: "Lord Cassian".into(),
        lens: Some("lewisian".into()),
        license_provenance: Some("homebrew".into()),
        body: Some("# Lord Cassian\n\nA knight of the watch.\n".into()),
        frontmatter,
    };

    let created = vault_write::create_entity(&vault, &input).unwrap();
    assert_eq!(created.id, "main:npc/lord-cassian");
    let on_disk = vault_dir.join("campaigns/main/npcs/lord-cassian.md");
    assert!(on_disk.exists(), "{} should exist", on_disk.display());
    let text = fs::read_to_string(&on_disk).unwrap();
    assert!(text.contains("title: Lord Cassian"));
    assert!(text.contains("license_provenance: homebrew"));
    assert!(text.contains("status: budding"));
    assert!(text.contains("A knight of the watch."));

    // Manually mirror the file into the SQLite index so update_entity
    // (which looks the entity up in `entities`) can find it. In the running
    // app this happens via the notify-rs watcher.
    fake_index_md_file(&database, &vault_dir, &on_disk);

    // Update — change the body, leave the title alone.
    let patch = EntityPatch {
        title: None,
        lens: None,
        license_provenance: None,
        body: Some("# Lord Cassian\n\nKnight Commander of the Watch.\n".into()),
        frontmatter: None,
    };
    let updated = vault_write::update_entity(&vault, &database, &created.id, &patch).unwrap();
    assert_eq!(updated.id, "main:npc/lord-cassian");
    let text = fs::read_to_string(&on_disk).unwrap();
    assert!(text.contains("Knight Commander of the Watch."));
    assert!(text.contains("title: Lord Cassian")); // preserved

    // Refresh the SQLite mirror, then update again — this time renaming.
    fake_index_md_file(&database, &vault_dir, &on_disk);

    let rename = EntityPatch {
        title: Some("Lord Cassian Velerian".into()),
        lens: None,
        license_provenance: None,
        body: None,
        frontmatter: None,
    };
    let renamed = vault_write::update_entity(&vault, &database, &updated.id, &rename).unwrap();
    assert_eq!(renamed.id, "main:npc/lord-cassian-velerian");
    let new_path = vault_dir.join("campaigns/main/npcs/lord-cassian-velerian.md");
    assert!(new_path.exists());
    assert!(!on_disk.exists(), "old file should be removed on rename");
}

#[test]
fn entity_create_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let vault = VaultRoot(tmp.path().join("vault"));
    fs::create_dir_all(&vault.0).unwrap();
    let bad = EntityInput {
        campaign_id: "../../etc".into(),
        r#type: "npc".into(),
        title: "x".into(),
        lens: None,
        license_provenance: None,
        body: None,
        frontmatter: serde_json::Map::new(),
    };
    let err = vault_write::create_entity(&vault, &bad).unwrap_err();
    assert!(err.to_string().contains("unsafe path"));
}

#[test]
fn entity_create_rejects_bad_license() {
    let tmp = TempDir::new().unwrap();
    let vault = VaultRoot(tmp.path().join("vault"));
    fs::create_dir_all(&vault.0).unwrap();
    let bad = EntityInput {
        campaign_id: "main".into(),
        r#type: "npc".into(),
        title: "x".into(),
        lens: None,
        license_provenance: Some("OGL".into()),
        body: None,
        frontmatter: serde_json::Map::new(),
    };
    let err = vault_write::create_entity(&vault, &bad).unwrap_err();
    assert!(err.to_string().contains("license_provenance"));
}

#[test]
fn relations_crud() {
    let tmp = TempDir::new().unwrap();
    let dbf = tmp.path().join("p3-rel.db");
    let database = db::Db::open(&dbf).unwrap();

    // Insert two minimal entities directly so the FK passes.
    {
        let conn = database.conn.lock().unwrap();
        for id in ["main:npc/cassian", "main:faction/house-velerian"] {
            conn.execute(
                "INSERT INTO entities
                 (id, type, campaign_id, source, lens, license_provenance,
                  frontmatter, body, body_text, statblock, file_path, mtime, hash)
                 VALUES (?1, 'npc', 'main', 'vault', NULL, 'homebrew',
                         '{}', NULL, NULL, NULL, ?2, 0, 'h')",
                params![id, format!("campaigns/main/{id}.md")],
            )
            .unwrap();
        }
    }

    vault_write::add_relation(
        &database,
        "main:npc/cassian",
        "member_of",
        "main:faction/house-velerian",
    )
    .unwrap();
    let rels = vault_write::list_relations(&database, "main:npc/cassian").unwrap();
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].edge_type, "member_of");

    // Idempotent on duplicate insert.
    vault_write::add_relation(
        &database,
        "main:npc/cassian",
        "member_of",
        "main:faction/house-velerian",
    )
    .unwrap();
    let rels = vault_write::list_relations(&database, "main:npc/cassian").unwrap();
    assert_eq!(rels.len(), 1);

    vault_write::delete_relation(
        &database,
        "main:npc/cassian",
        "member_of",
        "main:faction/house-velerian",
    )
    .unwrap();
    let rels = vault_write::list_relations(&database, "main:npc/cassian").unwrap();
    assert!(rels.is_empty());
}

#[test]
fn foundry_e2e_round_trips_a_synthetic_pack() {
    let tmp = TempDir::new().unwrap();
    let dbf = tmp.path().join("p3-foundry.db");
    let database = db::Db::open(&dbf).unwrap();

    // Build a synthetic packs/spells directory mimicking foundryvtt/pf2e shape.
    let pack_root = tmp.path().join("packs/spells");
    fs::create_dir_all(&pack_root).unwrap();
    let entries = [
        (
            "force-barrage.json",
            r#"{
                "_id": "AAAAAAAAAAAAAAAA",
                "name": "Force Barrage",
                "type": "spell",
                "system": {
                  "details": {"description": {"value": "<p>Three magical missiles…</p>"}}
                }
            }"#,
        ),
        (
            "heal.json",
            r#"{
                "_id": "BBBBBBBBBBBBBBBB",
                "name": "Heal",
                "type": "spell",
                "system": {
                  "description": {"value": "<p>You channel positive energy…</p>"}
                }
            }"#,
        ),
        (
            "broken.txt",
            r#"# not json; should be skipped without exploding"#,
        ),
    ];
    for (name, body) in entries {
        fs::write(pack_root.join(name), body).unwrap();
    }

    let report = foundry::import_packs(&database, &pack_root, LicensePosture::Orc).unwrap();
    assert_eq!(report.files_seen, 2, "txt is filtered before counting");
    assert_eq!(report.files_imported, 2);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);

    let conn = database.conn.lock().unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE source = 'reference' AND type = 'spell'
             AND license_provenance = 'orc'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);

    let force_barrage_id: String = conn
        .query_row(
            "SELECT id FROM entities WHERE json_extract(frontmatter, '$.title') = 'Force Barrage'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(force_barrage_id, "foundry:AAAAAAAAAAAAAAAA");
}

// -----------------------------------------------------------------------
// Test helpers
// -----------------------------------------------------------------------

/// In the running app the vault watcher (notify-rs) populates `entities`
/// when a markdown file is written. Tests don't run the watcher; this
/// minimal mirror stamps an entity row directly from a file we know exists.
fn fake_index_md_file(database: &db::Db, vault: &std::path::Path, file: &std::path::Path) {
    let raw = fs::read_to_string(file).unwrap();
    let (fm_yaml, body) = split_fm(&raw);
    let fm: serde_yaml::Mapping = serde_yaml::from_str(fm_yaml).unwrap();
    let id = fm
        .get(serde_yaml::Value::String("id".into()))
        .and_then(serde_yaml::Value::as_str)
        .unwrap()
        .to_string();
    let title = fm
        .get(serde_yaml::Value::String("title".into()))
        .and_then(serde_yaml::Value::as_str)
        .unwrap()
        .to_string();
    let entity_type = fm
        .get(serde_yaml::Value::String("type".into()))
        .and_then(serde_yaml::Value::as_str)
        .unwrap()
        .to_string();
    let campaign = fm
        .get(serde_yaml::Value::String("campaign_id".into()))
        .and_then(serde_yaml::Value::as_str)
        .unwrap()
        .to_string();
    let license = fm
        .get(serde_yaml::Value::String("license_provenance".into()))
        .and_then(serde_yaml::Value::as_str)
        .unwrap()
        .to_string();
    let rel = file.strip_prefix(vault).unwrap().to_string_lossy().into_owned();
    let fm_json = serde_json::json!({"title": title, "type": entity_type});
    let conn = database.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO entities
         (id, type, campaign_id, source, lens, license_provenance,
          frontmatter, body, body_text, statblock, file_path, mtime, hash)
         VALUES (?1, ?2, ?3, 'vault', NULL, ?4, ?5, ?6, ?6, NULL, ?7, 0, 'h')
         ON CONFLICT(id) DO UPDATE SET
           file_path = excluded.file_path,
           frontmatter = excluded.frontmatter,
           body = excluded.body,
           body_text = excluded.body_text",
        params![
            id,
            entity_type,
            campaign,
            license,
            fm_json.to_string(),
            body,
            rel,
        ],
    )
    .unwrap();
}

fn split_fm(s: &str) -> (&str, &str) {
    let trimmed = s.trim_start_matches('\u{FEFF}');
    if !trimmed.starts_with("---") {
        return ("", trimmed);
    }
    let after = trimmed[3..].trim_start_matches(['\r', '\n']);
    if let Some(end) = after.find("\n---") {
        let yaml = &after[..end];
        let rest = &after[end + 4..];
        let body = rest.trim_start_matches(['\r', '\n']);
        (yaml, body)
    } else {
        ("", trimmed)
    }
}
