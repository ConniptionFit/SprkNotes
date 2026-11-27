import { describe, expect, it } from "vitest";
import { canCheckSpeed, speedText } from "./speed";

describe("speed line", () => {
  it("says how fast and on how many threads", () => {
    expect(speedText({ state: "done", speed: 3.2, threads: 4 })).toBe("Speed: 3.2x real time on 4 threads");
    expect(speedText({ state: "done", speed: 5, threads: 1 })).toBe("Speed: 5.0x real time on 1 thread");
  });

  it("covers the other states", () => {
    expect(speedText({ state: "none", speed: null, threads: null })).toBe("Speed: not checked yet");
    expect(speedText({ state: "checking", speed: null, threads: null })).toBe("Checking the speed on this PC…");
    expect(speedText({ state: "env", speed: null, threads: 6 })).toBe("Speed: set to 6 threads on this PC");
    expect(speedText({ state: "unavailable", speed: null, threads: null })).toBe("");
    expect(speedText(null)).toBe("");
  });

  it("offers Check again only when a check can run", () => {
    expect(canCheckSpeed({ state: "done", speed: 3, threads: 4 })).toBe(true);
    expect(canCheckSpeed({ state: "none", speed: null, threads: null })).toBe(true);
    expect(canCheckSpeed({ state: "checking", speed: null, threads: null })).toBe(false);
    expect(canCheckSpeed({ state: "env", speed: null, threads: 4 })).toBe(false);
    expect(canCheckSpeed(null)).toBe(false);
  });
});
