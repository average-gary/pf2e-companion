#!/usr/bin/env python3
"""Extract seed data from wiki reference articles.

Reads:
  ~/wiki/topics/pf2e-worldbuilding-tool/wiki/reference/pf2e-remaster-name-mapping.md
  ~/wiki/topics/pf2e-biblical-reskin/wiki/reference/biblical-miracle-to-pf2e-spell-map.md

Writes:
  src-tauri/data/seeds/remaster_aliases.json
  src-tauri/data/seeds/miracle_spell_map.json

Re-run when the wiki references are updated. The Rust runtime embeds these via
include_str! at compile time (see db.rs::seed_phase1).
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

WIKI = Path.home() / "wiki" / "topics"
REMASTER_MD = WIKI / "pf2e-worldbuilding-tool" / "wiki" / "reference" / "pf2e-remaster-name-mapping.md"
MIRACLE_MD = WIKI / "pf2e-biblical-reskin" / "wiki" / "reference" / "biblical-miracle-to-pf2e-spell-map.md"

OUT_DIR = Path(__file__).resolve().parent.parent / "src-tauri" / "data" / "seeds"


def parse_pipe_tables(md: str) -> list[tuple[str, list[str], list[list[str]]]]:
    """Yield (heading, header_cells, body_rows) for each markdown table.

    A table is recognised by a header row, a separator (---|---) row, and
    one or more body rows. The heading is the most-recent ## or ### above the table.
    """
    out: list[tuple[str, list[str], list[list[str]]]] = []
    heading = ""
    lines = md.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        h = re.match(r"^(#+)\s+(.+?)\s*$", line)
        if h:
            heading = h.group(2)
            i += 1
            continue
        # Detect a table header followed by a separator
        if line.startswith("|") and i + 1 < len(lines) and re.match(r"^\|[\s:|-]+\|$", lines[i + 1]):
            header = [c.strip() for c in line.strip("|").split("|")]
            i += 2
            body: list[list[str]] = []
            while i < len(lines) and lines[i].startswith("|"):
                cells = [c.strip() for c in lines[i].strip("|").split("|")]
                body.append(cells)
                i += 1
            out.append((heading, header, body))
            continue
        i += 1
    return out


def clean(s: str) -> str:
    """Strip emphasis, links, footnote markers."""
    s = re.sub(r"\*\*(.+?)\*\*", r"\1", s)
    s = re.sub(r"\*(.+?)\*", r"\1", s)
    s = re.sub(r"_(.+?)_", r"\1", s)
    s = re.sub(r"\[\[([^|\]]+?)(\|[^\]]+)?\]\]", r"\1", s)
    s = re.sub(r"\[([^\]]+?)\]\([^)]+?\)", r"\1", s)
    return s.strip()


def extract_remaster_aliases() -> list[dict]:
    """Pull legacy↔Remaster pairs from the per-category tables.

    The reference article has tables for Class Features, Feats, Spells,
    Equipment/Items, Bestiary/Creatures, Ancestries/Heritages, Languages,
    Domains/Pantheons, plus subsections (Merged spells, Staves).
    """
    md = REMASTER_MD.read_text()
    tables = parse_pipe_tables(md)
    aliases: list[dict] = []
    seen: set[tuple[str, str]] = set()

    # Map heading patterns to category labels for the SQLite category column.
    category_for_heading = [
        (re.compile(r"class features", re.I), "class-feature"),
        (re.compile(r"\bfeats\b", re.I), "feat"),
        (re.compile(r"merged spells", re.I), "spell"),
        (re.compile(r"\bspells\b", re.I), "spell"),
        (re.compile(r"staves", re.I), "staff"),
        (re.compile(r"equipment|magic items", re.I), "item"),
        (re.compile(r"bestiary|creatures", re.I), "creature"),
        (re.compile(r"ancestries|heritages", re.I), "ancestry"),
        (re.compile(r"languages", re.I), "language"),
        (re.compile(r"domains|pantheons", re.I), "pantheon"),
    ]

    for heading, header, body in tables:
        # Detect (legacy, remaster, ...) shape: at least 2 columns where col 0 looks like a name and col 1 too.
        if len(header) < 2:
            continue
        h0 = header[0].lower()
        h1 = header[1].lower()
        # Accept "Legacy | Remaster" and "old_name | new_name" and "X | Remaster".
        is_alias_table = (
            ("legacy" in h0 or "old" in h0)
            and ("remaster" in h1 or "new" in h1)
        )
        if not is_alias_table:
            continue

        category = next(
            (cat for pat, cat in category_for_heading if pat.search(heading)),
            "other",
        )
        # Detect the spell "merged" subsection by looking at body content for / + → .
        for cells in body:
            if len(cells) < 2:
                continue
            legacy = clean(cells[0])
            remaster = clean(cells[1])
            if not legacy or not remaster:
                continue
            # Skip the "(homebrew, no canonical Remaster name)" rows that have a homebrew tag.
            if remaster.lower().startswith("**homebrew"):
                continue
            notes_parts: list[str] = []
            if len(cells) >= 3:
                notes_parts.append(clean(cells[2]))
            if len(cells) >= 4:
                notes_parts.append(clean(cells[3]))
            notes = " · ".join([n for n in notes_parts if n]) or None

            # Some legacy cells encode merges: "Remove Curse / Remove Disease" etc.
            # Split on " / " (slash with spaces) so each legacy gets its own row.
            for part in [p.strip() for p in re.split(r"\s/\s|\s+\+\s+", legacy)]:
                if not part:
                    continue
                key = (part.lower(), remaster.lower())
                if key in seen:
                    continue
                seen.add(key)
                aliases.append({
                    "legacy_name": part,
                    "remaster_name": remaster,
                    "category": category,
                    "notes": notes,
                })

    aliases.sort(key=lambda a: (a["category"], a["legacy_name"].lower()))
    return aliases


def extract_miracle_spell_map() -> list[dict]:
    """Pull Bible-miracle→PF2e-spell rows from the per-book tables."""
    md = MIRACLE_MD.read_text()
    tables = parse_pipe_tables(md)
    out: list[dict] = []
    seen: set[str] = set()

    book_for_heading = {
        re.compile(r"pentateuch", re.I): "Pentateuch",
        re.compile(r"joshua|judges", re.I): "Joshua-Judges",
        re.compile(r"kings|prophets", re.I): "Kings-Prophets",
        re.compile(r"gospels", re.I): "Gospels",
        re.compile(r"^acts", re.I): "Acts",
    }

    for heading, header, body in tables:
        if len(header) < 6:
            continue
        # Expect Miracle | Reference | PF2e Remaster spell | Tradition | Sanct. | Notes
        h = [c.lower() for c in header]
        if not (h[0].startswith("miracle") and h[1].startswith("reference")):
            continue
        book = next(
            (b for pat, b in book_for_heading.items() if pat.search(heading)),
            heading or "Other",
        )
        for cells in body:
            if len(cells) < 6:
                continue
            miracle = clean(cells[0])
            reference = clean(cells[1])
            spell_name = clean(cells[2])
            tradition = clean(cells[3]) or None
            sanct = clean(cells[4]) or None
            notes = clean(cells[5]) or None
            if not miracle:
                continue
            key = miracle.lower()
            if key in seen:
                continue
            seen.add(key)
            out.append({
                "miracle": miracle,
                "reference": reference,
                "book": book,
                "spell_name": spell_name,
                "tradition": tradition,
                "sanctification": sanct,
                "notes": notes,
            })
    return out


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    aliases = extract_remaster_aliases()
    miracles = extract_miracle_spell_map()

    (OUT_DIR / "remaster_aliases.json").write_text(
        json.dumps(aliases, indent=2, ensure_ascii=False) + "\n"
    )
    (OUT_DIR / "miracle_spell_map.json").write_text(
        json.dumps(miracles, indent=2, ensure_ascii=False) + "\n"
    )

    print(f"wrote {len(aliases)} remaster aliases", file=sys.stderr)
    print(f"wrote {len(miracles)} miracle-spell rows", file=sys.stderr)
    # Sanity checks against canonical spot examples.
    by_legacy = {a["legacy_name"].lower(): a["remaster_name"] for a in aliases}
    for legacy, expected in [("magic missile", "Force Barrage"), ("mithral", "Dawnsilver")]:
        got = by_legacy.get(legacy)
        if got != expected:
            print(f"WARN: {legacy} → {got!r}, expected {expected!r}", file=sys.stderr)
    by_ref = {m["reference"]: m["spell_name"] for m in miracles}
    for ref in ["Mt 14:25", "1 Kgs 18:38"]:
        if ref not in by_ref:
            print(f"WARN: miracle {ref!r} missing from extraction", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
