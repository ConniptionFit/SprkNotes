import { describe, expect, it } from "vitest";
import { findKeys, isFindKey, splitHits, step } from "./find";

const entries = [
  { key: "a", text: "Welcome to the intake call." },
  { key: "b", text: "Nothing here." },
  { key: "c", text: "The INTAKE   form is ready." },
];

describe("find", () => {
  it("finds entries ignoring case and extra spaces", () => {
    expect(findKeys(entries, "intake")).toEqual(["a", "c"]);
    expect(findKeys(entries, "  intake form ")).toEqual(["c"]);
    expect(findKeys(entries, "")).toEqual([]);
    expect(findKeys(entries, "missing")).toEqual([]);
  });
  it("splits text around every match, keeping the original wording", () => {
    expect(splitHits("Intake, then intake again", "intake")).toEqual([
      { text: "Intake", hit: true },
      { text: ", then ", hit: false },
      { text: "intake", hit: true },
      { text: " again", hit: false },
    ]);
    expect(splitHits("The INTAKE   form", "intake form")).toEqual([
      { text: "The ", hit: false },
      { text: "INTAKE   form", hit: true },
    ]);
    expect(splitHits("a+b (c)", "+b (")).toEqual([
      { text: "a", hit: false },
      { text: "+b (", hit: true },
      { text: "c)", hit: false },
    ]);
    expect(splitHits("plain", "")).toEqual([{ text: "plain", hit: false }]);
  });
  it("steps through matches and wraps", () => {
    expect(step(3, -1)).toBe(0);
    expect(step(3, -1, true)).toBe(2);
    expect(step(3, 2)).toBe(0);
    expect(step(3, 0, true)).toBe(2);
    expect(step(0, 0)).toBe(-1);
  });
  it("takes Ctrl+F only", () => {
    const k = (code: string, ctrl: boolean, shift = false) => ({ code, ctrlKey: ctrl, altKey: false, shiftKey: shift, metaKey: false });
    expect(isFindKey(k("KeyF", true))).toBe(true);
    expect(isFindKey(k("KeyF", false))).toBe(false);
    expect(isFindKey(k("KeyF", true, true))).toBe(false);
  });
});
