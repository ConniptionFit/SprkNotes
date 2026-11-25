import { describe, expect, it } from "vitest";
import { GATE, ease, gated, linePoints, meterRest, meterStart, meterStep, meterStill } from "./waveform";

// Run the meter at display frames while the level holds `level` for `ms`.
const run = (m: ReturnType<typeof meterStart>, level: number, ms: number, snap = false) => {
  for (let t = 0; t < ms; t += 16) m = meterStep(m, level, 16, snap);
  return m;
};

describe("sound meter", () => {
  it("treats room hiss below the gate as silence and stretches the rest to 0..1", () => {
    expect(gated(GATE)).toBe(0);
    expect(gated(0.1)).toBe(0);
    expect(gated(1)).toBe(1);
    expect(gated(GATE + (1 - GATE) / 2)).toBeCloseTo(0.5);
    expect(gated(NaN)).toBe(0);
    expect(gated(2)).toBe(1);
  });

  it("rises quickly and falls gently", () => {
    const up = ease(0, 1, 50);
    const down = 1 - ease(1, 0, 50);
    expect(up).toBeGreaterThan(down);
    expect(up).toBeLessThan(1);
    expect(ease(0, 1, 10_000)).toBeLessThanOrEqual(1);
  });

  it("goes flat and still when silence repeats the same 0", () => {
    // The window hears a level only when it changes, so after speech it may get one 0 and no more.
    let m = run(meterStart(), 1, 400);
    expect(m.env).toBeGreaterThan(0.9);
    expect(meterStill(m, 1)).toBe(false);
    m = run(m, 0, 1500);
    expect(meterStill(m, 0)).toBe(true);
    expect(meterRest(m).env).toBe(0);
  });

  it("is not still while it is still falling", () => {
    const m = meterStep(run(meterStart(), 0.9, 200), 0, 60);
    expect(meterStill(m, 0)).toBe(false);
  });

  it("stays still on hiss below the gate", () => {
    expect(meterStill(run(meterStart(), GATE - 0.05, 1000), GATE - 0.05)).toBe(true);
  });

  it("with reduced motion, jumps straight to the level and does not drift", () => {
    let m = run(meterStart(), 1, 100, true);
    expect(m.env).toBe(1);
    expect(m.phase).toBe(0);
    m = run(m, 0, 50, true);
    expect(meterStill(m, 0)).toBe(true);
  });

  it("drifts only while there is sound", () => {
    const m = run(meterStart(), 1, 200);
    expect(m.phase).toBeGreaterThan(0);
    const quiet = run(meterRest(m), 0, 200);
    expect(quiet.phase).toBe(m.phase);
  });
});

describe("line", () => {
  it("is exactly flat in silence", () => {
    expect(linePoints(0, 1234, 20).every((y) => y === 0)).toBe(true);
  });

  it("swells with loudness, tapers to the ends and stays inside the box", () => {
    const quiet = linePoints(0.2, 500, 40);
    const loud = linePoints(1, 500, 40);
    const peak = (a: number[]) => Math.max(...a.map(Math.abs));
    expect(peak(loud)).toBeGreaterThan(peak(quiet));
    expect(peak(loud)).toBeLessThanOrEqual(1);
    expect(Math.abs(loud[0])).toBeLessThan(1e-9);
    expect(Math.abs(loud[loud.length - 1])).toBeLessThan(1e-9);
  });

  it("moves over time while there is sound", () => {
    expect(linePoints(1, 0, 20)).not.toEqual(linePoints(1, 120, 20));
  });
});
