import { describe, expect, test } from "bun:test";

import { FuzzySearch } from "../lib";

// ─── Core functionality ───────────────────────

describe("FuzzySearch", () => {
  test("basic exact matching (distance 0)", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 0 }],
      { wholeWords: false },
    );
    expect(fs.patternCount).toBe(1);
    expect(fs.isMatch("say hello world")).toBe(
      true,
    );
    expect(fs.isMatch("helo world")).toBe(false);
  });

  test("distance 1: substitution", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      { wholeWords: false },
    );
    const matches = fs.findIter("say helo world");
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("helo");
    expect(matches[0]!.distance).toBe(1);
  });

  test("distance 1: valid match in noisy text", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      { wholeWords: false },
    );
    const matches = fs.findIter(
      "qqq helxo qqq",
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("helxo");
    expect(matches[0]!.distance).toBe(1);
  });

  test("distance 1: deletion", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      { wholeWords: false },
    );
    const matches = fs.findIter("say helo world");
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("helo");
    expect(matches[0]!.distance).toBe(1);
  });

  test("distance 2: two substitutions", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 2 }],
      { wholeWords: false },
    );
    expect(fs.isMatch("hxlxo")).toBe(true);
  });

  test("no match beyond distance", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      { wholeWords: false },
    );
    expect(fs.isMatch("hxxlo")).toBe(false);
  });

  test("multiple patterns", () => {
    const fs = new FuzzySearch(
      [
        { pattern: "foo", distance: 1 },
        { pattern: "bar", distance: 1 },
      ],
      { wholeWords: false },
    );
    const matches = fs.findIter("fao bor");
    expect(matches).toHaveLength(2);
    expect(matches[0]!.pattern).toBe(0);
    expect(matches[0]!.text).toBe("fao");
    expect(matches[1]!.pattern).toBe(1);
    expect(matches[1]!.text).toBe("bor");
  });

  test("string shorthand patterns", () => {
    const fs = new FuzzySearch(["abc", "xyz"], {
      wholeWords: false,
    });
    expect(fs.patternCount).toBe(2);
    expect(fs.isMatch("abc")).toBe(true);
  });

  test("named patterns", () => {
    const fs = new FuzzySearch(
      [
        {
          pattern: "Novák",
          distance: 1,
          name: "surname",
        },
      ],
      { wholeWords: false },
    );
    const matches = fs.findIter("Nowák");
    expect(matches).toHaveLength(1);
    expect(matches[0]!.name).toBe("surname");
  });

  test("rejects distance >= pattern length", () => {
    expect(() => {
      new FuzzySearch([
        { pattern: "test", distance: 4 },
      ]);
    }).toThrow("Distance 4 >= pattern length 4");
    // distance 3 on a 4-char pattern is OK
    expect(() => {
      new FuzzySearch([
        { pattern: "test", distance: 3 },
      ]);
    }).not.toThrow();
    // distance 5 on a 6-char pattern is OK
    expect(() => {
      new FuzzySearch([
        { pattern: "abcdef", distance: 5 },
      ]);
    }).not.toThrow();
  });

  test("rejects empty pattern", () => {
    expect(() => {
      new FuzzySearch([
        { pattern: "", distance: 1 },
      ]);
    }).toThrow("Empty pattern");
  });
});

// ─── Czech legal names (primary use case) ────

describe("Czech names", () => {
  test("OCR error: l→1", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Gaislerová", distance: 1 }],
      { wholeWords: true },
    );
    const matches = fs.findIter(
      "Smlouva s Gais1erová o pronájmu.",
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("Gais1erová");
    expect(matches[0]!.distance).toBe(1);
  });

  test("missing háček (diacritics norm)", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Gaislerová", distance: 0 }],
      {
        normalizeDiacritics: true,
        wholeWords: true,
      },
    );
    const matches = fs.findIter(
      "Podpis: Gaislerova",
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("Gaislerova");
    expect(matches[0]!.distance).toBe(0);
  });

  test("typo: w→v", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Novák", distance: 1 }],
      { wholeWords: true },
    );
    const matches = fs.findIter(
      "Pan Nowák podepsal smlouvu.",
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("Nowák");
  });

  test("transliteration: Příbram → Pribram", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Příbram", distance: 0 }],
      {
        normalizeDiacritics: true,
        wholeWords: true,
      },
    );
    expect(fs.isMatch("Město Pribram")).toBe(true);
  });

  test("multiple names in one document", () => {
    const fs = new FuzzySearch(
      [
        { pattern: "Gaislerová", distance: 1 },
        { pattern: "Šnytrová", distance: 1 },
        { pattern: "Novák", distance: 1 },
      ],
      { wholeWords: true },
    );
    const text =
      "Gais1erová a Snytrová podepsali " +
      "smlouvu s Nowák.";
    const matches = fs.findIter(text);
    expect(matches).toHaveLength(3);
    expect(matches[0]!.text).toBe("Gais1erová");
    expect(matches[1]!.text).toBe("Snytrová");
    expect(matches[2]!.text).toBe("Nowák");
  });
});

// ─── Diacritics normalization ────────────────

describe("diacritics normalization", () => {
  test("strips combining marks (NFD)", () => {
    const fs = new FuzzySearch(
      [{ pattern: "café", distance: 0 }],
      {
        normalizeDiacritics: true,
        wholeWords: false,
      },
    );
    expect(fs.isMatch("cafe")).toBe(true);
    expect(fs.isMatch("café")).toBe(true);
  });

  test("combined with distance", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Šnytrová", distance: 1 }],
      {
        normalizeDiacritics: true,
        wholeWords: true,
      },
    );
    // "Snytrova" is: strip diacritics (dist 0)
    // + no háček (another strip) → matches
    expect(fs.isMatch("Snytrova")).toBe(true);
  });
});

// ─── Case insensitive ────────────────────────

describe("case insensitive", () => {
  test("basic case folding", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Hello", distance: 0 }],
      {
        caseInsensitive: true,
        wholeWords: false,
      },
    );
    expect(fs.isMatch("hello")).toBe(true);
    expect(fs.isMatch("HELLO")).toBe(true);
    expect(fs.isMatch("hElLo")).toBe(true);
  });

  test("case insensitive + distance", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Novák", distance: 1 }],
      {
        caseInsensitive: true,
        wholeWords: true,
      },
    );
    expect(fs.isMatch("NOWÁK")).toBe(true);
  });
});

// ─── Whole words ─────────────────────────────

describe("whole words", () => {
  test("rejects substring matches", () => {
    const fs = new FuzzySearch(
      [{ pattern: "cat", distance: 1 }],
      { wholeWords: true },
    );
    // "category" contains "cat" at dist 0, but
    // it's not a whole word.
    expect(fs.isMatch("category")).toBe(false);
    expect(fs.isMatch("the cat sat")).toBe(true);
  });

  test("word boundary at punctuation", () => {
    const fs = new FuzzySearch(
      [{ pattern: "cat", distance: 0 }],
      { wholeWords: true },
    );
    expect(fs.isMatch("(cat)")).toBe(true);
    expect(fs.isMatch("cat.")).toBe(true);
    expect(fs.isMatch("cat,dog")).toBe(true);
  });

  test("wholeWords: false allows substrings", () => {
    const fs = new FuzzySearch(
      [{ pattern: "cat", distance: 1 }],
      { wholeWords: false },
    );
    expect(fs.isMatch("category")).toBe(true);
  });
});

// ─── Unicode ─────────────────────────────────

describe("Unicode", () => {
  test("supplementary plane (emoji)", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      { wholeWords: false },
    );
    const matches = fs.findIter("😀 helo 😀");
    expect(matches).toHaveLength(1);
    // Emoji is 2 UTF-16 code units
    expect(matches[0]!.start).toBe(3);
    expect(matches[0]!.text).toBe("helo");
  });

  test("Cyrillic names", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Иванов", distance: 1 }],
      { wholeWords: true },
    );
    expect(fs.isMatch("Иванов подписал")).toBe(
      true,
    );
    // Substitution: о→а
    expect(fs.isMatch("Иванав подписал")).toBe(
      true,
    );
  });
});

// ─── replaceAll ──────────────────────────────

describe("replaceAll", () => {
  test("basic replacement", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Novák", distance: 1 }],
      { wholeWords: true },
    );
    const result = fs.replaceAll(
      "Pan Nowák podepsal.",
      ["[REDACTED]"],
    );
    expect(result).toBe(
      "Pan [REDACTED] podepsal.",
    );
  });

  test("multiple pattern replacement", () => {
    const fs = new FuzzySearch(
      [
        { pattern: "Alice", distance: 1 },
        { pattern: "Bob", distance: 1 },
      ],
      { wholeWords: true },
    );
    const result = fs.replaceAll(
      "Alice met Bob today.",
      ["[A]", "[B]"],
    );
    expect(result).toBe("[A] met [B] today.");
  });

  test("wrong replacement count throws", () => {
    const fs = new FuzzySearch(
      [{ pattern: "test", distance: 1 }],
      { wholeWords: false },
    );
    expect(() => {
      fs.replaceAll("test", ["a", "b"]);
    }).toThrow("Expected 1 replacements, got 2");
  });
});

// ─── UTF-16 offset correctness ──────────────

describe("UTF-16 offsets", () => {
  test("slice(start, end) === text field", () => {
    const fs = new FuzzySearch(
      [
        { pattern: "Příbram", distance: 1 },
        { pattern: "Novák", distance: 1 },
      ],
      { wholeWords: true },
    );
    const hay =
      "Město Pribram a Nowák podepsali.";
    const matches = fs.findIter(hay);
    for (const m of matches) {
      expect(hay.slice(m.start, m.end)).toBe(
        m.text,
      );
    }
  });
});

// ─── Edge cases ──────────────────────────────

describe("edge cases", () => {
  test("empty haystack", () => {
    const fs = new FuzzySearch(
      [{ pattern: "abc", distance: 1 }],
      { wholeWords: false },
    );
    expect(fs.findIter("")).toHaveLength(0);
    expect(fs.isMatch("")).toBe(false);
  });

  test("pattern longer than haystack", () => {
    const fs = new FuzzySearch(
      [{ pattern: "abcdef", distance: 1 }],
      { wholeWords: false },
    );
    expect(fs.isMatch("abc")).toBe(false);
  });

  test("exact match at distance 0", () => {
    const fs = new FuzzySearch(
      [{ pattern: "exact", distance: 0 }],
      { wholeWords: false },
    );
    const matches = fs.findIter("an exact match");
    expect(matches).toHaveLength(1);
    expect(matches[0]!.distance).toBe(0);
    expect(matches[0]!.text).toBe("exact");
  });

  test("match at start of text", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      { wholeWords: false },
    );
    const matches = fs.findIter("helo world");
    expect(matches).toHaveLength(1);
    expect(matches[0]!.start).toBe(0);
  });

  test("match at end of text", () => {
    const fs = new FuzzySearch(
      [{ pattern: "world", distance: 1 }],
      { wholeWords: false },
    );
    const matches = fs.findIter("hello worlb");
    expect(matches).toHaveLength(1);
    expect(matches[0]!.text).toBe("worlb");
  });

  test("single character pattern (exact)", () => {
    // Single char at dist 0 (dist >= len rejected)
    const fs = new FuzzySearch(
      [{ pattern: "a", distance: 0 }],
      { wholeWords: false },
    );
    expect(fs.isMatch("a")).toBe(true);
    expect(fs.isMatch("b")).toBe(false);
  });

  test("short pattern with valid distance", () => {
    const fs = new FuzzySearch(
      [{ pattern: "ab", distance: 1 }],
      { wholeWords: false },
    );
    expect(fs.isMatch("ab")).toBe(true);
    expect(fs.isMatch("ac")).toBe(true);
    expect(fs.isMatch("xz")).toBe(false);
  });
});

// ─── Damerau-Levenshtein ─────────────────────

describe("Damerau-Levenshtein", () => {
  test("transposition costs 1 (not 2)", () => {
    // Standard Levenshtein: "abcd" → "abdc" = 2
    //   (sub c→d + sub d→c)
    // Damerau: "abcd" → "abdc" = 1 (transpose)
    const lev = new FuzzySearch(
      [{ pattern: "abcd", distance: 1 }],
      {
        wholeWords: false,
        metric: "levenshtein",
      },
    );
    const dam = new FuzzySearch(
      [{ pattern: "abcd", distance: 1 }],
      {
        wholeWords: false,
        metric: "damerau-levenshtein",
      },
    );
    // Levenshtein can't find "abdc" at dist 1
    const levM = lev.findIter("xxabdcxx");
    expect(
      levM.some((m) => m.text === "abdc"),
    ).toBe(false);
    // Damerau finds it
    const damM = dam.findIter("xxabdcxx");
    expect(
      damM.some(
        (m) => m.text === "abdc" && m.distance === 1,
      ),
    ).toBe(true);
  });

  test("transposition in real name", () => {
    const fs = new FuzzySearch(
      [{ pattern: "Novák", distance: 1 }],
      {
        wholeWords: true,
        metric: "damerau-levenshtein",
      },
    );
    // "Nvoák" = transposition of o and v
    expect(fs.isMatch("Nvoák podepsal")).toBe(
      true,
    );
    // Standard Levenshtein would need dist 2
    const lev = new FuzzySearch(
      [{ pattern: "Novák", distance: 1 }],
      { wholeWords: true, metric: "levenshtein" },
    );
    expect(lev.isMatch("Nvoák podepsal")).toBe(
      false,
    );
  });

  test("non-transposition edits unchanged", () => {
    const fs = new FuzzySearch(
      [{ pattern: "hello", distance: 1 }],
      {
        wholeWords: false,
        metric: "damerau-levenshtein",
      },
    );
    // Substitution: still dist 1
    const m1 = fs.findIter("hallo");
    expect(m1).toHaveLength(1);
    expect(m1[0]!.distance).toBe(1);

    // Deletion: still dist 1
    const m2 = fs.findIter("helo");
    expect(m2).toHaveLength(1);
    expect(m2[0]!.distance).toBe(1);

    // Exact: still dist 0
    const m3 = fs.findIter("hello");
    expect(m3).toHaveLength(1);
    expect(m3[0]!.distance).toBe(0);
  });

  test("distance field reflects Damerau metric", () => {
    const fs = new FuzzySearch(
      [{ pattern: "abcd", distance: 1 }],
      {
        wholeWords: false,
        metric: "damerau-levenshtein",
      },
    );
    const matches = fs.findIter("abdc");
    expect(matches).toHaveLength(1);
    // "abdc" is a transposition of "abcd" → dist 1
    expect(matches[0]!.distance).toBe(1);
    expect(matches[0]!.text).toBe("abdc");
  });

  test("default metric is levenshtein", () => {
    const fs = new FuzzySearch(
      [{ pattern: "abcd", distance: 1 }],
      { wholeWords: false },
    );
    // "abdc" is dist 2 in standard Levenshtein
    // (two substitutions), not findable at dist 1.
    const m = fs.findIter("xxabdcxx");
    expect(
      m.some((x) => x.text === "abdc"),
    ).toBe(false);
  });

  test("replaceAll with Damerau", () => {
    const fs = new FuzzySearch(
      [{ pattern: "test", distance: 1 }],
      {
        wholeWords: true,
        metric: "damerau-levenshtein",
      },
    );
    // "tset" = transposition → dist 1 in Damerau
    const result = fs.replaceAll(
      "the tset passed",
      ["[OK]"],
    );
    expect(result).toBe("the [OK] passed");
  });
});
