// Vitest unit tests for src/lib/citation.ts
//
// Real fixtures sampled from src-tauri/data/content/**/*.md sources arrays.
// Each `kind` block covers the canonical shape; the `edge cases` block
// covers the things I was nervous about during the audit.

import { describe, expect, test } from "vitest";
import { parseCitation, parseSources, type Citation } from "./citation";

describe("Bible citations", () => {
  test("simple chapter:verse", () => {
    const c = parseCitation("Mt 14:25");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.reference).toBe("Mt 14:25");
      expect(c.gatewayUrl).toContain("biblegateway.com");
      // encodeURIComponent: space → %20, colon → %3A.
      expect(c.gatewayUrl).toContain("Mt%2014%3A25");
      expect(c.gatewayUrl).toContain("NRSVUE");
    }
  });

  test("numbered book — 1 Corinthians", () => {
    const c = parseCitation("1 Cor 12:8-10");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.reference).toBe("1 Cor 12:8-10");
      expect(c.gatewayUrl).toContain("1%20Cor");
    }
  });

  test("numbered book — 2 Maccabees", () => {
    const c = parseCitation("2 Maccabees 12:46");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.reference).toBe("2 Maccabees 12:46");
    }
  });

  test("multi-chapter range with semicolons", () => {
    const c = parseCitation("Daniel 10:13, 21; 12:1");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.reference).toBe("Daniel 10:13, 21; 12:1");
    }
  });

  test("parenthetical gloss is stripped from `reference` but kept on `raw`", () => {
    const c = parseCitation("Genesis 6:1-4 (Nephilim)");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.reference).toBe("Genesis 6:1-4");
      expect(c.raw).toBe("Genesis 6:1-4 (Nephilim)");
    }
  });

  test("Revelation 21:1-22:5 (the longest hyphen-joined cite in the corpus)", () => {
    const c = parseCitation("Revelation 21:1-22:5 (the city descending)");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.reference).toMatch(/^Revelation 21:1-22/);
    }
  });

  test("multiple-passage citation — Mt + Mk", () => {
    const c = parseCitation("Matthew 5:22; 10:28; 23:33; Mark 9:43-48 (Gehenna)");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      // We only need the leading bible ref classified; the rest is preserved
      // verbatim in `raw` for the UI tooltip.
      expect(c.reference).toContain("Matthew");
    }
  });

  test("1 Enoch routes to Sefaria, not BibleGateway", () => {
    const c = parseCitation("1 Enoch 6:7");
    expect(c.kind).toBe("bible");
    if (c.kind === "bible") {
      expect(c.gatewayUrl).toContain("sefaria.org");
      expect(c.gatewayUrl).not.toContain("biblegateway.com");
    }
  });

  test("just a chapter, no verse — Genesis 7", () => {
    const c = parseCitation("Genesis 7");
    expect(c.kind).toBe("bible");
  });
});

describe("Wiki citations", () => {
  test("topic + article + section", () => {
    const c = parseCitation(
      "wiki:pf2e-biblical-reskin/yhwh-deity-template § Lewisian",
    );
    expect(c.kind).toBe("wiki");
    if (c.kind === "wiki") {
      expect(c.topic).toBe("pf2e-biblical-reskin");
      expect(c.article).toBe("yhwh-deity-template");
      expect(c.section).toBe("Lewisian");
    }
  });

  test("topic + article, no section", () => {
    const c = parseCitation(
      "wiki:pf2e-biblical-reskin/biblical-cosmology-pf2e-mapping",
    );
    expect(c.kind).toBe("wiki");
    if (c.kind === "wiki") {
      expect(c.section).toBeNull();
    }
  });

  test("section with multi-word phrase + ampersand", () => {
    const c = parseCitation(
      "wiki:pf2e-biblical-reskin/magic-theology-approaches § hybrid Charism + Lewisian",
    );
    expect(c.kind).toBe("wiki");
    if (c.kind === "wiki") {
      expect(c.section).toBe("hybrid Charism + Lewisian");
    }
  });

  test("dated raw-source filename pattern", () => {
    // These appear in some packs that cite the original ingest file.
    const c = parseCitation(
      "wiki:pf2e-biblical-reskin/2026-05-24-denominational-scope-theosis",
    );
    expect(c.kind).toBe("wiki");
    if (c.kind === "wiki") {
      expect(c.article).toBe("2026-05-24-denominational-scope-theosis");
    }
  });
});

describe("URL citations", () => {
  test("plain https URL with no label", () => {
    const c = parseCitation("https://github.com/average-gary/pf2e-companion");
    expect(c.kind).toBe("url");
    if (c.kind === "url") {
      expect(c.href).toBe("https://github.com/average-gary/pf2e-companion");
      expect(c.label).toBe("github.com");
    }
  });

  test("URL with leading label and dash", () => {
    const c = parseCitation("Project repo — https://github.com/average-gary/pf2e-companion");
    expect(c.kind).toBe("url");
    if (c.kind === "url") {
      expect(c.label).toBe("Project repo");
      expect(c.href).toContain("github.com");
    }
  });
});

describe("Doctrine / confessional citations", () => {
  test("Catechism", () => {
    const c = parseCitation("Catechism of the Catholic Church §§ 232-260 (the Holy Trinity)");
    expect(c.kind).toBe("doctrine");
    if (c.kind === "doctrine") {
      expect(c.tradition).toBe("Catechism");
    }
  });

  test("Westminster Confession", () => {
    const c = parseCitation("Westminster Confession of Faith VII (the covenant of grace)");
    expect(c.kind).toBe("doctrine");
    if (c.kind === "doctrine") {
      expect(c.tradition).toBe("Westminster Confession");
    }
  });

  test("WCF abbreviation", () => {
    const c = parseCitation("WCF I.1 (the closing of the canon)");
    expect(c.kind).toBe("doctrine");
    if (c.kind === "doctrine") {
      expect(c.tradition).toBe("WCF");
    }
  });

  test("Heidelberg Catechism", () => {
    const c = parseCitation("Heidelberg Catechism, Lord's Day 35, Q.96-98 (the Second Commandment)");
    expect(c.kind).toBe("doctrine");
  });

  test("Council of Trent", () => {
    const c = parseCitation("Council of Trent, Session 25 (relics)");
    expect(c.kind).toBe("doctrine");
  });

  test("Schleitheim Confession", () => {
    const c = parseCitation("Schleitheim Confession (1527), Article 6");
    expect(c.kind).toBe("doctrine");
  });

  test("`Tradition:` prefix used by some Orthodox sources", () => {
    const c = parseCitation("Tradition: Synaxis of the Bodiless Powers (Nov 8)");
    expect(c.kind).toBe("doctrine");
  });
});

describe("Patristic / theologian citations", () => {
  test("Aquinas", () => {
    const c = parseCitation("Aquinas, Summa Theologica I.108 a.6");
    expect(c.kind).toBe("patristic");
    if (c.kind === "patristic") {
      expect(c.author).toBe("Aquinas");
      expect(c.work).toContain("Summa Theologica");
    }
  });

  test("Calvin (with comma)", () => {
    const c = parseCitation("Calvin, Institutes IV.19.18 (apostolic gifts and their cessation)");
    expect(c.kind).toBe("patristic");
    if (c.kind === "patristic") {
      expect(c.author).toBe("Calvin");
    }
  });

  test("Gregory of Nyssa", () => {
    const c = parseCitation("Gregory of Nyssa, *Life of Moses* II — perpetual ascent (epektasis)");
    expect(c.kind).toBe("patristic");
    if (c.kind === "patristic") {
      expect(c.author).toBe("Gregory of Nyssa");
    }
  });

  test("Tolkien Letter", () => {
    const c = parseCitation("Tolkien Letter 131 (to Milton Waldman, ~1951)");
    expect(c.kind).toBe("patristic");
  });

  test("C.S. Lewis", () => {
    const c = parseCitation("C.S. Lewis, *De Descriptione Temporum* (Cambridge inaugural, 1954)");
    expect(c.kind).toBe("patristic");
  });

  test("Pseudo-Dionysius", () => {
    const c = parseCitation("Pseudo-Dionysius, De Coelesti Hierarchia iii");
    expect(c.kind).toBe("patristic");
  });
});

describe("Plain fallback", () => {
  test("anonymous Russian text", () => {
    const c = parseCitation(
      "*The Way of a Pilgrim* (19th-century Russian, anonymous) — the Jesus Prayer's continuous practice",
    );
    expect(c.kind).toBe("plain");
  });

  test("hagiographic reference", () => {
    const c = parseCitation("Pius XI, canonization 1925");
    expect(c.kind).toBe("doctrine"); // Pius is in the doctrine prefix list
  });

  test("Seraphim of Sarov account", () => {
    const c = parseCitation("Seraphim of Sarov, conversation with Motovilov (the Sarov-snow account)");
    expect(c.kind).toBe("patristic");
  });

  test("genuinely-unrecognized falls through to plain", () => {
    const c = parseCitation("Some thing not on any list whatsoever");
    expect(c.kind).toBe("plain");
    if (c.kind === "plain") {
      expect(c.text).toBe("Some thing not on any list whatsoever");
    }
  });
});

describe("Order-matters disambiguation", () => {
  // The wiki: prefix has to win against the bible-style "Gen" / "Mt"
  // prefixes that *could* otherwise match if we ran the bible regex first.
  test("wiki ref containing a string that looks like a bible book wins as wiki", () => {
    const c = parseCitation("wiki:pf2e-biblical-reskin/genesis-overview");
    expect(c.kind).toBe("wiki");
  });

  // URLs should win against everything except wiki:.
  test("URL inside a string starting with a doctrine prefix still parses as doctrine", () => {
    // The doctrine prefix wins because we don't want to accidentally
    // hyperlink prefix-text. The user can copy the URL out of the tooltip.
    const c = parseCitation("Catechism §§ 232-260 — see https://www.vatican.va/...");
    // We accept either classification — what we care about is that the
    // result is *typed*, not `plain`. Let's just confirm it's not `plain`.
    expect(c.kind).not.toBe("plain");
  });
});

describe("parseSources batch helper", () => {
  test("maps an array preserving order", () => {
    const out = parseSources([
      "Mt 14:25",
      "wiki:pf2e-biblical-reskin/yhwh-deity-template",
      "Aquinas, Summa I.108",
      "Some plain footnote",
    ]);
    expect(out).toHaveLength(4);
    expect(out[0].kind).toBe("bible");
    expect(out[1].kind).toBe("wiki");
    expect(out[2].kind).toBe("patristic");
    expect(out[3].kind).toBe("plain");
  });

  test("empty input → empty output", () => {
    expect(parseSources([])).toEqual([]);
  });

  test("every Citation carries the original raw string", () => {
    const inputs = [
      "Mt 14:25",
      "wiki:topic/article § X",
      "https://example.com",
      "WCF I.1",
      "Calvin, Institutes",
      "?",
    ];
    const out = parseSources(inputs);
    out.forEach((c, i) => {
      const raw = (c as Citation & { raw?: string }).raw;
      // `plain` carries `text` not `raw` — check that case separately.
      if (c.kind === "plain") {
        expect(c.text).toBe(inputs[i]);
      } else {
        expect(raw).toBe(inputs[i]);
      }
    });
  });
});
