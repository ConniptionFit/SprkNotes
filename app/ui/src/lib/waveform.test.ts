import { describe, expect, it } from "vitest";
import { BARS, FLOOR, pushLevel, waveform } from "./waveform";

describe("waveform", () => {
  it("is five short bars when silent", () => {
    expect(waveform([])).toEqual(new Array(BARS).fill(FLOOR));
  });

  it("puts the newest level in the middle, tallest", () => {
    let h: number[] = [];
    h = pushLevel(h, 0.2);
    h = pushLevel(h, 1);
    const w = waveform(h);
    expect(w[2]).toBe(1);
    expect(w[2]).toBeGreaterThan(w[1]);
    expect(w[0]).toBeCloseTo(w[4]);
  });

  it("keeps only the last few levels and clamps bad input", () => {
    let h: number[] = [];
    for (const l of [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 2, NaN]) h = pushLevel(h, l);
    expect(h.length).toBe(BARS);
    expect(h[0]).toBe(0);
    expect(h[1]).toBe(1);
  });
});
