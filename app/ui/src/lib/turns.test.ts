import { describe, expect, it } from "vitest";
import { lastSpeaker, turnStarts } from "./turns";

describe("speaker turns", () => {
  it("names the speaker once per run of lines", () => {
    expect(turnStarts(["Others", "Others", "Me", "Me", "Others"])).toEqual([true, false, true, false, true]);
  });
  it("a mark, note or event ends the turn", () => {
    expect(turnStarts(["Me", null, "Me"])).toEqual([true, false, true]);
  });
  it("carries the turn on from the list above", () => {
    expect(turnStarts(["Me", "Others"], "Me")).toEqual([false, true]);
    expect(turnStarts(["Me"], null)).toEqual([true]);
  });
  it("hands the last speaker to the next list", () => {
    expect(lastSpeaker(["Me", "Others"])).toBe("Others");
    expect(lastSpeaker([], "Me")).toBe("Me");
    expect(lastSpeaker(["Me", null])).toBeNull();
  });
});
