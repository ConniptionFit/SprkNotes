import { describe, expect, it } from "vitest";
import { initialFollow, isNearBottom, onLines, onScroll, returnToLive } from "./follow";

describe("live pane follow mode (SPEC 8.2)", () => {
  it("is near the bottom within 48 px", () => {
    expect(isNearBottom(952, 100, 1100)).toBe(true); // 48 px away
    expect(isNearBottom(951, 100, 1100)).toBe(false); // 49 px away
  });
  it("the reader scrolling up turns follow off, back near the bottom turns it on and clears the count", () => {
    let s = onScroll(initialFollow, false, true);
    expect(s.follow).toBe(false);
    s = onLines(s, 3);
    expect(s.unseen).toBe(3);
    s = onScroll(s, true, true);
    expect(s).toEqual(initialFollow);
  });
  it("content growing under the pane never turns follow off", () => {
    // New text made the pane taller, so the scroll position is suddenly far from the bottom.
    expect(onScroll(initialFollow, false, false)).toEqual(initialFollow);
  });
  it("app scrolling that has not reached the bottom yet does not turn follow off", () => {
    let s = initialFollow;
    for (let i = 0; i < 20; i++) s = onScroll(s, false, false);
    expect(s.follow).toBe(true);
  });
  it("lines are not counted while following", () => {
    expect(onLines(initialFollow, 5).unseen).toBe(0);
  });
  it("return to live resets", () => {
    expect(returnToLive()).toEqual(initialFollow);
  });
});
