//! Headless Phase-0 smoke test. Validates that the storage layer comes up,
//! seeds the YHWH fixture, and the FTS index returns it for "Hosts".
//!
//! Run with: cargo test --test smoke -- --nocapture

#[path = "../src/db.rs"]
mod db;

use rusqlite::params;
use tempfile::TempDir;

#[test]
fn schema_migrates_seeds_and_searches() {
    // tempfile is a dev-only crate; ensure it's available via the test harness.
    let tmp = TempDir::new().expect("tmp dir");
    let path = tmp.path().join("index.db");
    let conn_db = db::Db::open(&path).expect("open db");
    conn_db.seed_smoke_fixture().expect("seed fixture");

    let conn = conn_db.conn.lock().unwrap();

    // Schema bumped to v3 in Phase 6 § C (added `embeddings_meta` table).
    let v: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 3, "schema_version row should be 3");

    // FTS index returns the smoke fixture for "Hosts".
    let mut stmt = conn
        .prepare(
            "SELECT e.id
             FROM entities_fts
             JOIN entities e ON e.rowid = entities_fts.rowid
             WHERE entities_fts MATCH ?1
             LIMIT 5",
        )
        .unwrap();
    let ids: Vec<String> = stmt
        .query_map(params!["Hosts"], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(ids, vec!["smoke-yhwh"], "FTS should match the YHWH fixture");

    // Vector virtual table is loadable (sqlite-vec extension registered).
    conn.execute(
        "INSERT INTO entities_vec(rowid, embedding) VALUES (1, vec_f32('[0.0, 0.1, 0.2]'))",
        [],
    )
    .err()
    .map(|e| {
        // 768-dim mismatch is fine; what we care about is that vec_f32() and vec0
        // are recognised by sqlite — the call would fail with a parser error if
        // sqlite-vec hadn't loaded.
        assert!(
            e.to_string().contains("dim")
                || e.to_string().contains("expected")
                || e.to_string().to_lowercase().contains("size"),
            "unexpected sqlite-vec error: {e}"
        );
    });
}
