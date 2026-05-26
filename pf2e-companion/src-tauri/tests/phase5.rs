//! Phase 5 smoke test: all 5 bundled lens packs load, each with its
//! per-type minimums, and the YHWH stat block exists for every lens.

#[path = "../src/db.rs"]
mod db;
#[path = "../src/content.rs"]
mod content;

use rusqlite::params;
use tempfile::TempDir;

#[test]
fn all_five_lens_packs_load() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("phase5.db");
    let database = db::Db::open(&path).unwrap();
    let report = content::load_bundled_packs(&database).unwrap();

    assert!(
        report.packs_loaded >= 5,
        "expected ≥5 packs (lewisian/catholic/reformed/pentecostal/orthodox); got {}",
        report.packs_loaded
    );

    let conn = database.conn.lock().unwrap();

    for lens in ["lewisian", "catholic", "reformed", "pentecostal", "orthodox"] {
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE lens = ?1",
                params![lens],
                |r| r.get(0),
            )
            .unwrap();
        assert!(total >= 12, "{lens} should have ≥12 entries, got {total}");

        let deity_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE lens = ?1 AND type = 'deity'",
                params![lens],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deity_count, 1, "{lens} should have exactly 1 deity");

        // YHWH stat block sidecar reaches every pack.
        let (title, sb): (String, Option<String>) = conn
            .query_row(
                "SELECT json_extract(frontmatter, '$.title'), statblock
                 FROM entities WHERE id = ?1",
                params![format!("{lens}:yhwh")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|e| panic!("{lens}:yhwh missing: {e}"));
        assert!(title.to_lowercase().contains("yhwh") || title.contains("Lord"));
        let sb = sb.unwrap_or_else(|| panic!("{lens}:yhwh statblock missing"));
        assert!(sb.contains("\"name\""), "{lens} statblock has name field");
    }
}

#[test]
fn lens_specific_distinctives_present() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("phase5-distinctives.db");
    let database = db::Db::open(&path).unwrap();
    content::load_bundled_packs(&database).unwrap();
    let conn = database.conn.lock().unwrap();

    // Catholic: purgatory cosmology entry — load-bearing distinctive.
    let purgatory: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id = 'catholic:cosmology-purgatory'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(purgatory, 1, "catholic must ship purgatory cosmology");

    // Catholic: a substantial saint roster (≥18). Twelve Apostles + Mary
    // + Joseph + 4 archangels would be 18; the wiki worked example asks
    // for ~20.
    let cath_saints: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE lens = 'catholic' AND type = 'saint'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(cath_saints >= 18, "catholic should ship ≥18 saints, got {cath_saints}");

    // Orthodox: aerial-toll-houses headline content.
    let toll_houses: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id = 'orthodox:cosmology-aerial-toll-houses'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(toll_houses, 1, "orthodox must ship aerial-toll-houses");

    // Orthodox: 7 archangels (Michael, Gabriel, Raphael, Uriel, Selaphiel,
    // Jegudiel, Barachiel) — vs Catholic 4, Reformed 2.
    let orth_arch: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities
             WHERE lens = 'orthodox' AND type = 'saint'
               AND id IN (
                 'orthodox:michael','orthodox:gabriel','orthodox:raphael',
                 'orthodox:uriel','orthodox:selaphiel','orthodox:jegudiel',
                 'orthodox:barachiel'
               )",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orth_arch, 7, "orthodox should ship the Synaxis of Seven");

    // Reformed: explicitly does NOT venerate saints — should ship Michael
    // + Gabriel + 4 covenant figures (Abraham, Moses, David, Paul).
    // Should NOT ship Raphael or Uriel.
    for not_present in ["reformed:raphael", "reformed:uriel"] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE id = ?1",
                params![not_present],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            n, 0,
            "{not_present} should NOT ship in the Reformed pack (deuterocanonical)",
        );
    }

    // Pentecostal: charismata reference content.
    let charismata: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities
             WHERE lens = 'pentecostal' AND id LIKE 'pentecostal:%charismata%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(charismata >= 1, "pentecostal must ship a charismata reference");
}

#[test]
fn fts_surfaces_distinctive_content_per_lens() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("phase5-fts.db");
    let database = db::Db::open(&path).unwrap();
    content::load_bundled_packs(&database).unwrap();
    let conn = database.conn.lock().unwrap();

    // Each query targets a phrase that should only land in the
    // matching lens, given how the packs were authored.
    let cases: &[(&str, &str)] = &[
        ("purgatory", "catholic"),
        ("toll", "orthodox"),
        ("covenant", "reformed"),
        ("charism", "pentecostal"),
    ];

    for (term, expected_lens) in cases {
        let mut stmt = conn
            .prepare(
                "SELECT e.lens
                 FROM entities_fts
                 JOIN entities e ON e.rowid = entities_fts.rowid
                 WHERE entities_fts MATCH ?1
                 LIMIT 50",
            )
            .unwrap();
        let lenses: Vec<Option<String>> = stmt
            .query_map(params![term], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            lenses.iter().any(|l| l.as_deref() == Some(*expected_lens)),
            "FTS should surface `{term}` somewhere in the `{expected_lens}` pack; got lenses {lenses:?}"
        );
    }
}
