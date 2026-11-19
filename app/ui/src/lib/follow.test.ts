import { describe, expect, it } from "vitest";
import { initialFollow, isNearBottom, onLines, onScroll, returnToLive } from "./follow";

describe("live pane follow mode (SPEC 8.2)", () => {
  it("is near the bottom within 48 px", () => {
    expect(isNearBottom(952, 100, 1100)).toBe(true); // 48 px away
    expect(isNearBottom(951, 100, 1100)).toBe(false); // 49 px away
  });
  it("scrolling up turns follow off, back near the bottom turns it on and clears the count", () => {
    let s = onScroll(initialFollow, false);
    expect(s.follow).toBe(false);
    s = onLines(s, 3);
    expect(s.unseen).toBe(3);
    s = onScroll(s, true);
    expect(s).toEqual(initialFollow);
  });
  it("lines are not counted while following", () => {
    expect(onLines(initialFollow, 5).unseen).toBe(0);
  });
  it("return to live resets", () => {
    expect(returnToLive()).toEqual(initialFollow);
  });
});
