import { describe, expect, it } from "vitest";
import {
  BARS,
  GATE,
  REST,
  ROLL_STEP_MS,
  SLOTS,
  ease,
  flat,
  gated,
  pushLevel,
  rollHeights,
  rollPush,
  rollShift,
  rollStart,
  rollStep,
  rollStill,
  settled,
  waveform,
} from "./waveform";

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

describe("rolling waveform", () => {
  it("starts flat and still", () => {
    const r = rollStart();
    expect(rollHeights(r)).toEqual(new Array(BARS + 1).fill(REST));
    expect(rollStill(r)).toBe(true);
  });

  it("brings new sound in at the right edge and moves it left", () => {
    let r = rollStart();
    for (let t = 0; t < ROLL_STEP_MS * 2; t += 16) r = rollStep(r, 0.9, 16);
    const h = rollHeights(r);
    expect(h[h.length - 1]).toBeGreaterThan(REST);
    expect(h[0]).toBe(REST);
    const before = h.findIndex((x) => x > REST);
    for (let t = 0; t < ROLL_STEP_MS * 3; t += 16) r = rollStep(r, 0, 16);
    expect(rollHeights(r).findIndex((x) => x > REST)).toBeLessThan(before);
  });

  it("slides smoothly between columns", () => {
    let r = rollStart();
    r = rollStep(r, 0.9, ROLL_STEP_MS);
    expect(rollShift(r)).toBeCloseTo(1);
    r = rollStep(r, 0.9, ROLL_STEP_MS / 2);
    expect(rollShift(r)).toBeCloseTo(0.5);
  });

  it("goes flat and still after dead air, hiss included", () => {
    let r = rollStart();
    for (let t = 0; t < 1000; t += 16) r = rollStep(r, 0.9, 16);
    expect(rollStill(r)).toBe(false);
    for (let t = 0; t < 3000; t += 16) r = rollStep(r, 0.2, 16);
    expect(rollStill(r)).toBe(true);
    expect(rollHeights(r).every((x) => Math.abs(x - REST) < 0.01)).toBe(true);
  });

  it("with reduced motion, adds a column per level", () => {
    let r = rollStart();
    r = rollPush(r, 1);
    expect(rollHeights(r)[BARS]).toBe(1);
    expect(rollShift(r)).toBe(1);
    r = rollPush(r, gated(0.1));
    expect(rollHeights(r)[BARS]).toBe(REST);
    expect(rollHeights(r)[BARS - 1]).toBe(1);
  });
});
