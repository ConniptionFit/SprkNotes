import { describe, expect, it } from "vitest";
import { BARS, GATE, REST, SLOTS, ease, flat, gated, pushLevel, settled, waveform } from "./waveform";

describe("waveform", () => {
  it("is a flat line when silent", () => {
    expect(waveform([])).toEqual(flat());
    expect(flat()).toEqual(new Array(BARS).fill(REST));
    expect(BARS).toBeGreaterThan(5);
  });

  it("treats room hiss below the gate as silence", () => {
    let h: number[] = [];
    for (const l of [0.1, 0.2, GATE, 0.05]) h = pushLevel(h, l);
    expect(waveform(h)).toEqual(flat());
    expect(gated(1)).toBe(1);
    expect(gated(GATE + (1 - GATE) / 2)).toBeCloseTo(0.5);
  });

  it("puts the newest level in the middle, tallest, symmetric", () => {
    let h: number[] = [];
    h = pushLevel(h, 0.5);
    h = pushLevel(h, 1);
    const w = waveform(h);
    const mid = (BARS - 1) / 2;
    expect(w[mid]).toBe(1);
    expect(w[mid]).toBeGreaterThan(w[mid - 1]);
    expect(w[0]).toBeCloseTo(w[BARS - 1]);
  });

  it("keeps only the last few levels and clamps bad input", () => {
    let h: number[] = [];
    for (const l of [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 2, NaN]) h = pushLevel(h, l);
    expect(h.length).toBe(SLOTS);
    expect(h[0]).toBe(0);
    expect(h[1]).toBe(1);
  });

  it("rises quickly and falls gently", () => {
    const up = ease(flat(), new Array(BARS).fill(1), 50)[0];
    const down = 1 - ease(new Array(BARS).fill(1), flat(), 50)[0];
    expect(up).toBeGreaterThan(down);
    expect(up).toBeLessThan(1);
  });

  it("falls back to the flat line and settles after silence", () => {
    let c = new Array(BARS).fill(1);
    for (let t = 0; t < 1500; t += 16) c = ease(c, flat(), 16);
    expect(settled(c, flat())).toBe(true);
    expect(Math.max(...c)).toBeLessThan(REST + 0.004);
  });

  it("ignores a long pause between frames instead of jumping past the target", () => {
    const c = ease(flat(), new Array(BARS).fill(1), 10_000);
    expect(Math.max(...c)).toBeLessThanOrEqual(1);
  });
});
