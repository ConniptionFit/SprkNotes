import { describe, expect, it } from "vitest";
import { CLOCK_START, elapsed, hms, hold, run } from "./clock";

describe("phone session clock", () => {
  it("counts only while transcribing", () => {
    let c = run(CLOCK_START, 1_000);
    expect(elapsed(c, 6_000)).toBe(5_000);
    c = hold(c, 6_000);
    expect(elapsed(c, 60_000)).toBe(5_000);
    c = run(c, 60_000);
    expect(elapsed(c, 62_500)).toBe(7_500);
  });
  it("ignores a second Start or Pause", () => {
    const c = run(run(CLOCK_START, 0), 5_000);
    expect(c.since).toBe(0);
    expect(hold(CLOCK_START, 9)).toEqual(CLOCK_START);
  });
  it("formats like the desktop timestamps", () => {
    expect(hms(0)).toBe("00:00:00");
    expect(hms(1_085_400)).toBe("00:18:05");
    expect(hms(3_600_000 * 2 + 61_000)).toBe("02:01:01");
  });
});
