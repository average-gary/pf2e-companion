# pf2e-companion

Cross-platform PF2e Remaster + Christian Biblical worldview reference & worldbuilding companion. Tauri 2 + SvelteKit + SQLite (FTS5 + sqlite-vec).

> **Status**: Phase 2 of 9 — Lewisian content pack landed 2026-05-26.
> Plan: `~/wiki/topics/pf2e-worldbuilding-tool/output/plan-cross-platform-pf2e-biblical-reference-2026-05-25.md`.

## What works

### Phase 0 — scaffold (done)
- Tauri 2 + SvelteKit + TypeScript skeleton.
- SQLite schema, FTS5, sqlite-vec extension registered via `sqlite3_auto_extension`.
- Vault watcher (notify-rs) starts on launch.
- IPC: `search`, `list_entities`, `schema_version`.

### Phase 1 — reference layer (done)
- **Seed extraction**: `scripts/extract_seeds.py` parses the wiki reference articles into JSON. **161 Remaster aliases + 71 miracle-spell rows** produced; embedded via `include_str!` so the binary ships the data.
- **Reference data lookups**: `lookup_alias("Magic Missile")` → `Force Barrage`; `lookup_miracle("Mt 14:25")` → *Water Walk* (Gospels). Aasimar/Tiefling/Aphorite/Ganzi all resolve to **Nephilim**. Mithral → **Dawnsilver**.
- **Encounter math**: `xp_budget` matches PF2e Remaster GM Core p.49 (party of 4: 40/60/80/120/160 XP for trivial/low/moderate/severe/extreme). `creature_xp` returns the level-vs-PL XP cost table.
- **Statblock validator**: structural sanity check + sanctification taxonomy enforcement (5 Remaster values incl. `none`, the Pharasma escape hatch).
- **Lens manifest**: `list_lenses()` returns the 5 v1 lens packs (Lewisian default + Catholic / Reformed / Pentecostal / Orthodox).
- **Markdown vault ingestion**: the watcher now parses YAML frontmatter, slugifies IDs, attaches sibling `.statblock.json`, indexes into FTS5, and propagates updates/deletes.
- **Foundry-pf2e ingest stub**: `import_foundry_pack(path, license)` walks a packs directory and inserts `entities` rows tagged with the chosen license posture.
- **UI**: 4 pages — `/` search with type filter; `/encounter` XP budget calculator + creature-XP table; `/aliases` Remaster name lookup; `/miracles` Bible-reference → spell. Lens picker in the top bar; bottom-tabs nav for mobile.

### Phase 2 — Lewisian content pack (done)
- **Bundled content** — `data/content/lewisian/` packaged into the binary via `include_dir!`. **16 entries**: 1 deity (YHWH with full Remaster stat block JSON sidecar), 4 archangels (Michael, Gabriel, Raphael, Uriel — Uriel flagged as the deuterocanonical "swing case"), 6 cosmology entries (Heaven, Sheol, Gehenna, Tartarus, Abyss, New Jerusalem), 5 class reskin notes (Champion, Cleric, Oracle, Thaumaturge, Sorcerer).
- **Backend**: `content::load_bundled_packs` runs at startup; entries indexed under `source = 'reference'`, lens = `'lewisian'`, and surfaced through the existing search.
- **`get_entity(id)` IPC** for the detail page.
- **Detail page**: `/entity/[id]` route renders markdown body + sidebar with stat block card + sources frontmatter. `[[id]]` wikilinks in markdown are rewritten to internal hrefs. Search hits and tab targets all link through.
- **`<Statblock />` component** for deity stat block cards (edicts/anathema/sanctification/domains/cleric spells/iconography).

### Tests
**26 tests passing** (`cargo test`):
```
running 12 tests             [unit tests in lib]
test rules::tests::xp_budget_canonical_party_of_4 ... ok
test rules::tests::xp_budget_scales_with_party_size ... ok
test rules::tests::creature_xp_table ... ok
test rules::tests::validate_minimal_statblock ... ok
test rules::tests::validate_rejects_bad_sanctification ... ok
test rules::tests::validate_warns_on_missing_license ... ok
test vault::tests::split_frontmatter_basic ... ok
test vault::tests::split_frontmatter_missing_returns_body_only ... ok
test vault::tests::slugify_collapses_punctuation ... ok
test vault::tests::strip_markdown_keeps_text_drops_syntax ... ok
test foundry::tests::foundry_record_extraction ... ok
test foundry::tests::license_posture_strings ... ok

running 2 tests              [tests/phase1.rs]
test seeds_load_and_canonical_lookups_resolve ... ok
test xp_budget_matches_pf2e_gm_core ... ok

running 1 test               [tests/smoke.rs]
test schema_migrates_seeds_and_searches ... ok
```

## What's not yet in the app (intentional, by phase)

- **Phase 3** — full markdown editor for the worldbuilding vault.
- **Phase 4** — mobile builds (`tauri ios init` / `tauri android init`).
- **Phase 5** — Catholic / Reformed / Pentecostal / Orthodox lens packs.
- **Phase 6** — LLM (Ollama + Anthropic), RAG, agent loop.
- **Phase 7** — Foundry export round-trip + plugin SDK.
- **Phase 8** — App-store submission + signing pipeline.

## Develop

Prerequisites: Rust 1.77+, Node 20+, pnpm. Python 3 for the seed extractor.

```bash
cd pf2e-companion
pnpm install
pnpm tauri dev          # launches the app window with HMR

# Re-extract seeds when the wiki reference articles change:
python3 scripts/extract_seeds.py

# Run the Rust test suite (15 tests, ~2s):
cd src-tauri && cargo test

# Production build (no installer):
pnpm tauri build --no-bundle
```

## Architecture

```
SvelteKit (src/) ─── IPC ─── Rust core (src-tauri/src/)
  routes/                        ├── db.rs        SQLite + sqlite-vec, schema v2
    +layout.svelte                │                + Phase 1 seeds (aliases, miracles)
    +page.svelte (search)         ├── commands.rs IPC: search/lookup/budget/validate/import
    encounter/+page.svelte        ├── rules.rs    XP budget, creature XP, statblock validation
    aliases/+page.svelte          ├── vault.rs    notify-rs + markdown frontmatter ingest
    miracles/+page.svelte         ├── foundry.rs  Foundry-pf2e JSON import (stub)
  lib/                            └── lib.rs      Tauri setup
    ipc.ts (typed bindings)
    lens.svelte.ts (state)
data/seeds/
  remaster_aliases.json     [161 rows]
  miracle_spell_map.json    [71 rows]
scripts/
  extract_seeds.py          [wiki .md → JSON]
```

Vault data is plain markdown + JSON in a user-chosen folder. SQLite is a derived index, never canonical. License-provenance (`orc | community-use | homebrew | proprietary`) is tracked per record and validated on ingest.

Full spec: `~/wiki/topics/pf2e-worldbuilding-tool/output/plan-cross-platform-pf2e-biblical-reference-2026-05-25.md`.

## Phase 3 next steps

1. **Full markdown editor for the worldbuilding vault** (CodeMirror 6 + frontmatter; entity creation; add-relation UI).
2. **Campaign management** — create/switch campaigns; campaign-scoped entities.
3. **License-provenance UI** on the editor (per the Phase 0 metadata field).
4. **Foundry-pf2e end-to-end test** against a downloaded `foundryvtt/pf2e/packs/spells` directory.
