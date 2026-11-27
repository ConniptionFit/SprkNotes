import { describe, expect, it } from "vitest";
import { changed, lastNote, lines } from "./customWords";

describe("lines", () => {
  it("keeps one entry per line and drops blank ones", () => {
    expect(lines("  metoprolol \r\n\n Charlie Mertens\n   \n")).toEqual(["metoprolol", "Charlie Mertens"]);
    expect(lines("")).toEqual([]);
  });
});

describe("changed", () => {
  it("ignores blank lines and spacing", () => {
    expect(changed("metoprolol\n\n", ["metoprolol"])).toBe(false);
    expect(changed("metoprolol\nLasix", ["metoprolol"])).toBe(true);
    expect(changed("", [])).toBe(false);
  });
});

describe("lastNote", () => {
  it("says nothing before a session or with an empty list", () => {
    expect(lastNote(null)).toBe("");
    expect(lastNote({ user: [], admin: [], last: null })).toBe("");
    expect(lastNote({ user: [], admin: [], last: { words: 0, hints: false } })).toBe("");
  });
  it("says whether the model took hints, with counts only", () => {
    expect(lastNote({ user: ["a"], admin: [], last: { words: 1, hints: true } })).toMatch(/^The last session used 1 custom word as hints/);
    const note = lastNote({ user: ["secret name"], admin: [], last: { words: 3, hints: false } });
    expect(note).toMatch(/3 custom words to fix near misses/);
    expect(note).not.toContain("secret");
  });
});
