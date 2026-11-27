import { describe, expect, it } from "vitest";
import { aboutPrefix, collectLineNotes, collectMarks, isLineMarkDefault, isMarkKey, lineMarkName, markLabel, markedLine, parseAbout, parseMarkLabel, withIds } from "./marks";
import type { Line } from "./types";

const press = (code: string, mods: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean; repeat?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
  metaKey: !!mods.meta,
  repeat: !!mods.repeat,
});

describe("isMarkKey", () => {
  it("takes Ctrl+M only", () => {
    expect(isMarkKey(press("KeyM", { ctrl: true }))).toBe(true);
    expect(isMarkKey(press("KeyM"))).toBe(false);
    expect(isMarkKey(press("KeyM", { ctrl: true, shift: true }))).toBe(false);
    expect(isMarkKey(press("KeyM", { ctrl: true, alt: true }))).toBe(false);
    expect(isMarkKey(press("KeyN", { ctrl: true }))).toBe(false);
  });

  it("ignores a held key so one press is one mark", () => {
    expect(isMarkKey(press("KeyM", { ctrl: true, repeat: true }))).toBe(false);
  });
});


describe("mark names", () => {
  it("are notes that name the time they belong to", () => {
    expect(markLabel("00:12:41", "  Driver   swap ")).toBe("Mark 00:12:41: Driver swap");
    expect(parseMarkLabel("Mark 00:12:41: Driver swap")).toEqual({ t: "00:12:41", name: "Driver swap" });
    expect(parseMarkLabel("Mark 00:12:41: ")).toBeNull();
    expect(parseMarkLabel("Remember the mark at noon")).toBeNull();
  });
  it("a marked line is named by its first words", () => {
    expect(lineMarkName("Others", "one two three")).toBe("Others: one two three");
    expect(lineMarkName("Me", "a b c d e f g h i j")).toBe("Me: a b c d e f g h…");
  });
});

describe("collectMarks", () => {
  const lines: Line[] = [
    { kind: "statement", id: 1, t: "00:00:05", speaker: "Others", text: "Welcome to the intake call." },
    { kind: "mark", id: 2, t: "00:01:00" },
    { kind: "statement", id: 3, t: "00:01:10", speaker: "Me", text: "We should swap drivers." },
    { kind: "note", id: 4, t: "00:02:00", text: "Mark 00:01:00: First name" },
    { kind: "note", id: 5, t: "00:02:30", text: "Mark 00:01:00: Driver swap" },
    { kind: "note", id: 6, t: "00:03:00", text: "Mark 00:00:05: Others: Welcome to the intake call." },
    { kind: "note", id: 7, t: "00:03:10", text: "Bring the sample sheet" },
    { kind: "note", id: 8, t: "00:03:20", text: "Mark 00:09:59: Nothing said then" },
  ];
  const m = collectMarks(lines);
  it("lists marks in time order with their newest name", () => {
    expect(m.items).toEqual([
      { id: 1, t: "00:00:05", name: "Others: Welcome to the intake call." },
      { id: 2, t: "00:01:00", name: "Driver swap" },
    ]);
  });
  it("hides naming notes that show on their mark, and keeps every other note", () => {
    expect([...m.hidden].sort()).toEqual([4, 5, 6]);
  });
  it("finds the mark for a line", () => {
    expect(m.byLine.get(2)?.name).toBe("Driver swap");
    expect(m.byLine.get(1)?.t).toBe("00:00:05");
    expect(m.byLine.has(3)).toBe(false);
  });
  it("an unnamed mark has an empty name", () => {
    expect(collectMarks([{ kind: "mark", id: 1, t: "00:00:01" }]).items).toEqual([{ id: 1, t: "00:00:01", name: "" }]);
  });
});

describe("notes about a line", () => {
  it("name the time of the line they are about", () => {
    expect(aboutPrefix("00:12:41")).toBe("About 00:12:41: ");
    expect(parseAbout("About 00:12:41:  check the dose ")).toEqual({ t: "00:12:41", text: "check the dose" });
    expect(parseAbout("About 00:12:41: ")).toBeNull();
    expect(parseAbout("Ask about the dose")).toBeNull();
  });

  const lines: Line[] = [
    { kind: "statement", id: 1, t: "00:00:05", speaker: "Others", text: "Welcome to the intake call." },
    { kind: "statement", id: 2, t: "00:00:05", speaker: "Me", text: "Thanks." },
    { kind: "statement", id: 3, t: "00:01:10", speaker: "Me", text: "We should swap drivers." },
    { kind: "note", id: 4, t: "00:02:00", text: "About 00:01:10: Ask IT first" },
    { kind: "note", id: 5, t: "00:02:30", text: "Bring the sample sheet" },
    { kind: "note", id: 6, t: "00:03:00", text: "About 00:01:10: Then tell Sam" },
    { kind: "note", id: 7, t: "00:03:10", text: "About 00:00:05: First line" },
    { kind: "note", id: 8, t: "00:03:20", text: "About 00:09:59: Nothing said then" },
  ];
  const n = collectLineNotes(lines);
  it("sit under their line in the order they were typed", () => {
    expect(n.under.get(3)).toEqual([
      { id: 4, t: "00:02:00", text: "Ask IT first" },
      { id: 6, t: "00:03:00", text: "Then tell Sam" },
    ]);
    expect(n.under.get(1)).toEqual([{ id: 7, t: "00:03:10", text: "First line" }]);
    expect(n.under.has(2)).toBe(false);
  });
  it("leave plain notes, and notes about no line, as entries of their own", () => {
    expect([...n.hidden].sort()).toEqual([4, 6, 7]);
  });
});

describe("markedLine", () => {
  const lines: Line[] = [
    { kind: "statement", id: 1, t: "00:00:05", speaker: "Others", text: "Welcome." },
    { kind: "mark", id: 2, t: "00:01:20" },
    // Written after the mark: it started before the mark and was still being said.
    { kind: "statement", id: 3, t: "00:01:10", speaker: "Me", text: "We should swap drivers." },
    { kind: "statement", id: 4, t: "00:02:00", speaker: "Others", text: "Later." },
  ];
  it("is the marked line itself", () => {
    expect(markedLine(lines, { id: 4, t: "00:02:00", name: "" })?.id).toBe(4);
  });
  it("is the line being said when Mark was pressed", () => {
    expect(markedLine(lines, { id: 2, t: "00:01:20", name: "" })?.id).toBe(3);
  });
  it("is nothing before the first line", () => {
    expect(markedLine(lines, { id: 9, t: "00:00:01", name: "" })).toBeNull();
  });
  it("knows a line mark's first name", () => {
    const l = { speaker: "Me", text: "We should swap drivers." };
    expect(isLineMarkDefault({ id: 3, t: "00:01:10", name: "Me: We should swap drivers." }, l)).toBe(true);
    expect(isLineMarkDefault({ id: 3, t: "00:01:10", name: "Driver swap" }, l)).toBe(false);
    expect(isLineMarkDefault({ id: 3, t: "00:01:10", name: "Me: We should swap drivers." }, null)).toBe(false);
  });
});

describe("withIds", () => {
  it("numbers saved lines by position", () => {
    expect(
      withIds([
        { kind: "statement", t: "00:00:01", speaker: "Me", text: "Hi" },
        { kind: "mark", t: "00:00:02", speaker: "", text: "" },
        { kind: "note", t: "00:00:03", speaker: "", text: "About 00:00:01: hello" },
      ]),
    ).toEqual([
      { kind: "statement", id: 0, t: "00:00:01", speaker: "Me", text: "Hi" },
      { kind: "mark", id: 1, t: "00:00:02" },
      { kind: "note", id: 2, t: "00:00:03", text: "About 00:00:01: hello" },
    ]);
  });
});
