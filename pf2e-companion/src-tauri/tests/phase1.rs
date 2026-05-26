//! Phase 1 smoke test: load real wiki-derived seed JSON, verify the
//! flagship lookups land where the wiki + plan say they should.

#[path = "../src/db.rs"]
mod db;
#[path = "../src/rules.rs"]
mod rules;

use rusqlite::params;
use rules::Difficulty;
use tempfile::TempDir;

#[test]
fn seeds_load_and_canonical_lookups_resolve() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("phase1.db");
    let database = db::Db::open(&path).unwrap();
    database.seed_reference_data().unwrap();

    let conn = database.conn.lock().unwrap();

    // Magic Missile → Force Barrage (the flagship Remaster rename).
    let remaster: String = conn
        .query_row(
            "SELECT remaster_name FROM remaster_aliases
             WHERE legacy_name = ?1 COLLATE NOCASE",
            params!["Magic Missile"],
            |r| r.get(0),
        )
        .expect("Magic Missile alias missing");
    assert_eq!(remaster, "Force Barrage");

    // Reverse direction also resolvable.
    let legacy: String = conn
        .query_row(
            "SELECT legacy_name FROM remaster_aliases
             WHERE remaster_name = ?1 COLLATE NOCASE
             ORDER BY legacy_name LIMIT 1",
            params!["Force Barrage"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy.to_lowercase(), "magic missile");

    // Mithral → Dawnsilver (canonical material rename).
    let mithral: String = conn
        .query_row(
            "SELECT remaster_name FROM remaster_aliases
             WHERE legacy_name = ?1 COLLATE NOCASE",
            params!["Mithral"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mithral, "Dawnsilver");

    // Walking on water (Mt 14:25) → has a Water Walk-flavored mapping.
    let row = conn
        .query_row::<(String, String, Option<String>), _, _>(
            "SELECT spell_name, book, tradition
             FROM miracle_spell_map
             WHERE reference = ?1",
            params!["Mt 14:25"],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("Mt 14:25 miracle missing");
    assert!(
        row.0.contains("Water Walk") || row.0.contains("Air Walk"),
        "expected Water Walk-flavored mapping, got {:?}",
        row.0
    );
    assert!(row.1.contains("Gospels"));

    // Aasimar / Tiefling / Aphorite / Ganzi all merged into Nephilim.
    for legacy in ["Aasimar", "Tiefling", "Aphorite", "Ganzi"] {
        let n: String = conn
            .query_row(
                "SELECT remaster_name FROM remaster_aliases
                 WHERE legacy_name = ?1 COLLATE NOCASE",
                params![legacy],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("{legacy} alias missing"));
        assert_eq!(n, "Nephilim", "{legacy} should map to Nephilim");
    }

    // Sanity: at least 100 aliases and 50 miracles loaded.
    let alias_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM remaster_aliases", [], |r| r.get(0))
        .unwrap();
    let miracle_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM miracle_spell_map", [], |r| r.get(0))
        .unwrap();
    assert!(alias_count >= 100, "alias count: {alias_count}");
    assert!(miracle_count >= 50, "miracle count: {miracle_count}");
}

#[test]
fn xp_budget_matches_pf2e_gm_core() {
    // Spot-check Remaster GM Core p.49 budget table.
    assert_eq!(rules::xp_budget(4, Difficulty::Trivial), 40);
    assert_eq!(rules::xp_budget(4, Difficulty::Low), 60);
    assert_eq!(rules::xp_budget(4, Difficulty::Moderate), 80);
    assert_eq!(rules::xp_budget(4, Difficulty::Severe), 120);
    assert_eq!(rules::xp_budget(4, Difficulty::Extreme), 160);

    // Party of 5 severe: +30
    assert_eq!(rules::xp_budget(5, Difficulty::Severe), 150);
    // Party of 6 extreme: +80
    assert_eq!(rules::xp_budget(6, Difficulty::Extreme), 240);
}
