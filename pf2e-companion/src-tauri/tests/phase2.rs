//! Phase 2 smoke test: bundled Lewisian content pack loads, YHWH has a
//! statblock attached, and FTS surfaces "Lord of Hosts".

#[path = "../src/db.rs"]
mod db;
#[path = "../src/content.rs"]
mod content;

use rusqlite::params;
use tempfile::TempDir;

#[test]
fn lewisian_pack_loads_with_statblocks() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("phase2.db");
    let database = db::Db::open(&path).unwrap();
    let report = content::load_bundled_packs(&database).unwrap();

    assert!(report.packs_loaded >= 1, "expected at least 1 pack loaded");
    assert!(
        report.entities_loaded >= 16,
        "v1-minimum Lewisian pack: 1 deity + 4 saints + 6 cosmology + 5 classes = 16; got {}",
        report.entities_loaded
    );

    let conn = database.conn.lock().unwrap();

    // Per-type counts: validates each section authored.
    for (entity_type, expected) in [
        ("deity", 1),
        ("saint", 4),
        ("plane", 6),
        ("class-reskin", 5),
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE type = ?1 AND lens = 'lewisian'",
                params![entity_type],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, expected, "lewisian {entity_type} count");
    }

    // YHWH has a statblock attached (sidecar JSON wired through).
    let (yhwh_title, sb): (String, Option<String>) = conn
        .query_row(
            "SELECT json_extract(frontmatter, '$.title'), statblock
             FROM entities WHERE id = 'lewisian:yhwh'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("lewisian:yhwh missing");
    assert!(yhwh_title.contains("YHWH"), "title should include YHWH");
    let sb = sb.expect("YHWH statblock JSON should be attached");
    assert!(sb.contains("longsword"), "statblock should set favored_weapon");
    assert!(sb.contains("\"sanctification\": \"can-choose-holy\""));

    // FTS surfaces a substring from a known entry.
    let fts_hit: String = conn
        .query_row(
            "SELECT e.id
             FROM entities_fts
             JOIN entities e ON e.rowid = entities_fts.rowid
             WHERE entities_fts MATCH 'archangel'
               AND e.lens = 'lewisian'
             LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("FTS should surface a Lewisian archangel entry");
    assert!(fts_hit.starts_with("lewisian:"));

    // Saints are correctly cross-categorized so a search for the
    // canonical names lands.
    for name in ["lewisian:michael", "lewisian:gabriel", "lewisian:raphael", "lewisian:uriel"] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE id = ?1",
                params![name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "{name} should be loaded");
    }

    // Cosmology entries with the expected ids
    for name in [
        "lewisian:cosmology-heaven",
        "lewisian:cosmology-sheol",
        "lewisian:cosmology-gehenna",
        "lewisian:cosmology-tartarus",
        "lewisian:cosmology-abyss",
        "lewisian:cosmology-new-jerusalem",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE id = ?1",
                params![name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "{name} should be loaded");
    }
}
