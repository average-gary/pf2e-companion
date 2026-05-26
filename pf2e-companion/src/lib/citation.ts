// Source-citation parser. Pure functions; no Svelte / Tauri / DOM deps.
//
// Sources frontmatter strings come in 6 flavors. We classify each one
// so the UI can link Bible refs out, hyperlink URLs, and style
// magisterial / patristic / wiki citations distinctively. Any string we
// can't classify falls through to `plain` — visible but unstyled, identical
// to the previous behavior.

export type Citation =
  | { kind: "bible"; reference: string; gatewayUrl: string; raw: string }
  | {
      kind: "wiki";
      topic: string;
      article: string;
      section: string | null;
      raw: string;
    }
  | { kind: "url"; href: string; label: string; raw: string }
  | { kind: "doctrine"; tradition: string; label: string; raw: string }
  | { kind: "patristic"; author: string; work: string; raw: string }
  | { kind: "plain"; text: string; raw: string };

// === Bible ================================================================
//
// Book list covers the 66-book canon + the deuterocanon used in the
// Catholic and Orthodox packs + 1 Enoch (cited from the Watchers entries).
// Order matters: longer / more-specific names first so e.g. "1 John" wins
// against a "John" prefix.
const BIBLE_BOOKS: Array<[string, string]> = [
  // Numbered books — match the digit + space + name.
  ["1 Samuel", "1 Samuel"], ["2 Samuel", "2 Samuel"],
  ["1 Kings", "1 Kings"], ["2 Kings", "2 Kings"],
  ["1 Chronicles", "1 Chronicles"], ["2 Chronicles", "2 Chronicles"],
  ["1 Corinthians", "1 Corinthians"], ["2 Corinthians", "2 Corinthians"],
  ["1 Thessalonians", "1 Thessalonians"], ["2 Thessalonians", "2 Thessalonians"],
  ["1 Timothy", "1 Timothy"], ["2 Timothy", "2 Timothy"],
  ["1 Peter", "1 Peter"], ["2 Peter", "2 Peter"],
  ["1 John", "1 John"], ["2 John", "2 John"], ["3 John", "3 John"],
  ["1 Maccabees", "1 Maccabees"], ["2 Maccabees", "2 Maccabees"],
  ["3 Maccabees", "3 Maccabees"],
  ["1 Cor", "1 Corinthians"], ["2 Cor", "2 Corinthians"],
  ["1 Sam", "1 Samuel"], ["2 Sam", "2 Samuel"],
  ["1 Kgs", "1 Kings"], ["2 Kgs", "2 Kings"],
  ["1 Chr", "1 Chronicles"], ["2 Chr", "2 Chronicles"],
  ["1 Thess", "1 Thessalonians"], ["2 Thess", "2 Thessalonians"],
  ["1 Tim", "1 Timothy"], ["2 Tim", "2 Timothy"],
  ["1 Pet", "1 Peter"], ["2 Pet", "2 Peter"],
  ["1 Jn", "1 John"], ["2 Jn", "2 John"], ["3 Jn", "3 John"],
  ["1 Macc", "1 Maccabees"], ["2 Macc", "2 Maccabees"],
  // OT (alphabetical within the historic + prophetic ordering).
  ["Genesis", "Genesis"], ["Gen", "Genesis"],
  ["Exodus", "Exodus"], ["Ex", "Exodus"],
  ["Leviticus", "Leviticus"], ["Lev", "Leviticus"],
  ["Numbers", "Numbers"], ["Num", "Numbers"],
  ["Deuteronomy", "Deuteronomy"], ["Deut", "Deuteronomy"],
  ["Joshua", "Joshua"], ["Josh", "Joshua"],
  ["Judges", "Judges"], ["Judg", "Judges"],
  ["Ruth", "Ruth"],
  ["Nehemiah", "Nehemiah"], ["Neh", "Nehemiah"],
  ["Esther", "Esther"], ["Esth", "Esther"],
  ["Job", "Job"],
  ["Psalms", "Psalms"], ["Psalm", "Psalms"], ["Pss", "Psalms"], ["Ps", "Psalms"],
  ["Proverbs", "Proverbs"], ["Prov", "Proverbs"],
  ["Ecclesiastes", "Ecclesiastes"], ["Eccl", "Ecclesiastes"],
  ["Song of Solomon", "Song of Solomon"], ["Song", "Song of Solomon"],
  ["Isaiah", "Isaiah"], ["Isa", "Isaiah"],
  ["Jeremiah", "Jeremiah"], ["Jer", "Jeremiah"],
  ["Lamentations", "Lamentations"], ["Lam", "Lamentations"],
  ["Ezekiel", "Ezekiel"], ["Ezek", "Ezekiel"], ["Ez", "Ezekiel"],
  ["Daniel", "Daniel"], ["Dan", "Daniel"],
  ["Hosea", "Hosea"], ["Hos", "Hosea"],
  ["Joel", "Joel"],
  ["Amos", "Amos"],
  ["Obadiah", "Obadiah"], ["Obad", "Obadiah"],
  ["Jonah", "Jonah"], ["Jon", "Jonah"],
  ["Micah", "Micah"], ["Mic", "Micah"],
  ["Nahum", "Nahum"], ["Nah", "Nahum"],
  ["Habakkuk", "Habakkuk"], ["Hab", "Habakkuk"],
  ["Zephaniah", "Zephaniah"], ["Zeph", "Zephaniah"],
  ["Haggai", "Haggai"], ["Hag", "Haggai"],
  ["Zechariah", "Zechariah"], ["Zech", "Zechariah"],
  ["Malachi", "Malachi"], ["Mal", "Malachi"],
  // NT.
  ["Matthew", "Matthew"], ["Matt", "Matthew"], ["Mt", "Matthew"],
  ["Mark", "Mark"], ["Mk", "Mark"],
  ["Luke", "Luke"], ["Lk", "Luke"],
  ["John", "John"], ["Jn", "John"],
  ["Acts", "Acts"],
  ["Romans", "Romans"], ["Rom", "Romans"],
  ["Galatians", "Galatians"], ["Gal", "Galatians"],
  ["Ephesians", "Ephesians"], ["Eph", "Ephesians"],
  ["Philippians", "Philippians"], ["Phil", "Philippians"],
  ["Colossians", "Colossians"], ["Col", "Colossians"],
  ["Titus", "Titus"],
  ["Philemon", "Philemon"], ["Phlm", "Philemon"],
  ["Hebrews", "Hebrews"], ["Heb", "Hebrews"],
  ["James", "James"], ["Jas", "James"],
  ["Jude", "Jude"],
  ["Revelation", "Revelation"], ["Rev", "Revelation"],
  // Deuterocanon (Catholic + Orthodox; Tobit names Raphael).
  ["Tobit", "Tobit"], ["Tob", "Tobit"],
  ["Judith", "Judith"], ["Jdt", "Judith"],
  ["Wisdom of Solomon", "Wisdom"], ["Wisdom", "Wisdom"], ["Wis", "Wisdom"],
  ["Sirach", "Sirach"], ["Ecclesiasticus", "Sirach"], ["Sir", "Sirach"],
  ["Baruch", "Baruch"], ["Bar", "Baruch"],
  // Bel and the Dragon (the Habakkuk transport is in Catholic / Orthodox
  // canons; cited in the Watchers / miracles entries).
  ["Bel & Dragon", "Bel and the Dragon"],
  ["Bel and the Dragon", "Bel and the Dragon"],
  // 1 Enoch — non-canonical to most Christians, canonical to Ethiopian
  // Orthodox; cited in the Watchers entries. BibleGateway doesn't host it,
  // so we still classify as `bible` but the URL points at sefaria's English
  // translation of Charles 1917.
  ["1 Enoch", "1 Enoch"], ["1 En", "1 Enoch"],
];

// Build the regex at module-load time. We anchor on word boundary, then
// match the book name exactly, then a chapter:verse expression that may
// include ranges (4-7), discontinuous lists (10:13, 20), and semicolons
// joining multiple references into one citation.
const BIBLE_BOOKS_BY_LENGTH = [...BIBLE_BOOKS].sort(
  (a, b) => b[0].length - a[0].length,
);
const BOOK_PATTERN = BIBLE_BOOKS_BY_LENGTH
  .map(([alias]) => alias.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
  .join("|");
// Chapter:verse expression: \d+(:\d+(-\d+)?(,\s*\d+(-\d+)?)*)? + ; chains.
const CHAPV = String.raw`\d+(?::\d+(?:-\d+)?(?:[,;]\s*\d+(?::\d+(?:-\d+)?)?)*)?`;
const BIBLE_RE = new RegExp(
  `^\\s*(${BOOK_PATTERN})\\s+(${CHAPV})(?:\\s*[;,]\\s*${CHAPV})*`,
  "i",
);

function buildBibleRef(raw: string): Citation | null {
  const m = raw.match(BIBLE_RE);
  if (!m) return null;
  // The reference is everything from the start of the match up to either the
  // first parenthesis (which contains a gloss) or end-of-string.
  const matched = m[0];
  let ref = matched;
  const parenIdx = raw.indexOf("(");
  if (parenIdx >= 0 && parenIdx < raw.length) {
    // Take the leading reference, strip any trailing "(...)" annotation.
    ref = raw.slice(0, parenIdx).trim().replace(/[,;]\s*$/, "");
  }
  // Map the matched alias to its canonical name for BibleGateway.
  const book = BIBLE_BOOKS_BY_LENGTH.find(
    ([alias]) => alias.toLowerCase() === m[1].toLowerCase(),
  );
  const canonicalBook = book ? book[1] : m[1];
  // 1 Enoch's translations live elsewhere; everything else goes through
  // BibleGateway with the NRSV (default available canon).
  let gatewayUrl: string;
  if (canonicalBook === "1 Enoch") {
    gatewayUrl = `https://www.sefaria.org/I_Enoch?lang=en`;
  } else {
    const refForUrl = ref.replace(canonicalBook, canonicalBook);
    gatewayUrl =
      `https://www.biblegateway.com/passage/?search=` +
      encodeURIComponent(refForUrl) +
      `&version=NRSVUE`;
  }
  return { kind: "bible", reference: ref.trim(), gatewayUrl, raw };
}

// === Wiki ref =============================================================
//
//   wiki:pf2e-biblical-reskin/yhwh-deity-template
//   wiki:pf2e-biblical-reskin/magic-theology-approaches § Lewisian
const WIKI_RE = /^wiki:([^/\s]+)\/([^\s§]+)(?:\s*§\s*(.+))?$/;
function buildWikiRef(raw: string): Citation | null {
  const m = raw.match(WIKI_RE);
  if (!m) return null;
  return {
    kind: "wiki",
    topic: m[1],
    article: m[2],
    section: m[3] ? m[3].trim() : null,
    raw,
  };
}

// === Bare URL =============================================================
const URL_RE = /(https?:\/\/[^\s)]+)/;
function buildUrlRef(raw: string): Citation | null {
  const m = raw.match(URL_RE);
  if (!m) return null;
  // Use everything before the URL as the label, falling back to the host.
  const before = raw.slice(0, m.index ?? 0).trim().replace(/[-—–:]\s*$/, "");
  let label = before;
  if (!label) {
    try {
      label = new URL(m[1]).hostname;
    } catch {
      label = m[1];
    }
  }
  return { kind: "url", href: m[1], label, raw };
}

// === Doctrine =============================================================
const DOCTRINE_PREFIXES = [
  "Catechism",
  "CCC",
  "Westminster Confession",
  "Westminster Larger Catechism",
  "Westminster Shorter Catechism",
  "WCF",
  "Heidelberg Catechism",
  "Heidelberg",
  "Council of Trent",
  "Trent",
  "Council of Florence",
  "Florence",
  "Council of Nicaea",
  "Nicaea",
  "Athanasian Creed",
  "Athanasian",
  "Nicene Creed",
  "Schleitheim",
  "39 Articles",
  "Thirty-Nine Articles",
  "Constantinople 1341",
  "Constantinople 1347",
  "Constantinople 1351",
  "Lateran",
  "Vatican I",
  "Vatican II",
  "Vatican 2001 Directory",
  "Pius",
  "Tradition:",
];
function buildDoctrineRef(raw: string): Citation | null {
  for (const prefix of DOCTRINE_PREFIXES) {
    if (raw.startsWith(prefix)) {
      return {
        kind: "doctrine",
        tradition: prefix.replace(/:$/, ""),
        label: raw,
        raw,
      };
    }
  }
  return null;
}

// === Patristic / theologian / Inkling =====================================
const PATRISTIC_PREFIXES = [
  "Aquinas",
  "Augustine",
  "Athanasius",
  "Gregory of Nyssa",
  "Gregory of Nazianzus",
  "Gregory the Great",
  "Gregory Palamas",
  "Palamas",
  "John of Damascus",
  "John Chrysostom",
  "Chrysostom",
  "Symeon",
  "Maximus the Confessor",
  "Maximus",
  "Basil",
  "Origen",
  "Calvin",
  "Luther",
  "Wesley",
  "Knox",
  "C.S. Lewis",
  "C. S. Lewis",
  "Lewis,",
  "Tolkien,",
  "Tolkien Letter",
  "Hooker",
  "Cyril of Alexandria",
  "Cyril of Jerusalem",
  "Seraphim of Sarov",
  "Pseudo-Dionysius",
  "Dionysius",
];
function buildPatristicRef(raw: string): Citation | null {
  for (const prefix of PATRISTIC_PREFIXES) {
    if (raw.startsWith(prefix)) {
      // Strip the author from the label so the work shows clean.
      const work = raw.slice(prefix.length).replace(/^[\s,—–:]+/, "").trim();
      return {
        kind: "patristic",
        author: prefix.replace(/[,]$/, ""),
        work: work || raw,
        raw,
      };
    }
  }
  return null;
}

// === Top-level dispatch ===================================================
//
// Order matters. wiki: prefix is unambiguous; check it before bible-style
// regex (which would otherwise match `Gen` if the wiki path happened to
// contain it). URLs come after wiki because we don't want `wiki:` to be
// mistaken for a URL-having string. Bible / doctrine / patristic are
// effectively disjoint by their leading-token patterns.
export function parseCitation(raw: string): Citation {
  const trimmed = raw.trim();
  return (
    buildWikiRef(trimmed) ??
    buildUrlRef(trimmed) ??
    buildBibleRef(trimmed) ??
    buildDoctrineRef(trimmed) ??
    buildPatristicRef(trimmed) ?? { kind: "plain", text: trimmed, raw: trimmed }
  );
}

export function parseSources(sources: string[]): Citation[] {
  return sources.map((s) => parseCitation(s));
}
