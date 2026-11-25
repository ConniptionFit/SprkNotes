import { describe, expect, it } from "vitest";
import {
  BARS,
  GATE,
  REST,
  SAMPLE_MS,
  SLOTS,
  WAVE_STYLES,
  barRects,
  ease,
  flat,
  gated,
  linePoints,
  meterRest,
  meterStart,
  meterStep,
  meterStill,
  pushLevel,
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

describe("meter clock", () => {
  // Run the meter at display frames while the level holds `level` for `ms`.
  const run = (m: ReturnType<typeof meterStart>, level: number, ms: number, snap = false) => {
    for (let t = 0; t < ms; t += 16) m = meterStep(m, level, 16, snap);
    return m;
  };

  it("walks every bar down to the flat line when silence repeats the same 0", () => {
    // The window hears a level only when it changes, so after speech it may get one 0 and no more.
    let m = run(meterStart(), 1, 400);
    expect(Math.max(...m.bars)).toBeGreaterThan(0.5);
    m = run(m, 0, 1500);
    expect(m.hist.every((h) => h === 0)).toBe(true);
    expect(settled(m.bars, flat())).toBe(true);
    expect(meterStill(m, 0)).toBe(true);
    expect(meterRest(m).bars).toEqual(flat());
  });

  it("samples about twenty times a second, not once per frame", () => {
    const m = meterStep(meterStart(), 1, SAMPLE_MS - 1);
    expect(m.hist[0]).toBe(0);
    expect(meterStep(m, 1, 1).hist[0]).toBe(1);
  });

  it("is not still while sound is coming in or the bars are still falling", () => {
    let m = run(meterStart(), 0.9, 200);
    expect(meterStill(m, 0.9)).toBe(false);
    m = meterStep(m, 0, 60);
    expect(meterStill(m, 0)).toBe(false);
  });

  it("treats hiss below the gate as silence", () => {
    const m = run(meterStart(), GATE - 0.05, 1000);
    expect(meterStill(m, GATE - 0.05)).toBe(true);
  });

  it("with reduced motion, jumps straight to each shape and the line does not drift", () => {
    let m = run(meterStart(), 1, 100, true);
    expect(m.bars).toEqual(waveform(m.hist));
    expect(m.env).toBe(1);
    expect(m.phase).toBe(0);
    m = run(m, 0, 300, true);
    expect(m.bars).toEqual(flat());
  });
});

describe("bar layout", () => {
  for (const dpr of [1, 1.25, 1.5, 1.75, 2, 3]) {
    it(`keeps bars and gaps even and on whole pixels at ${dpr * 100}%`, () => {
      const W = Math.round(34 * dpr);
      const H = Math.round(16 * dpr);
      const r = barRects(waveform([1, 0.6, 0.3, 0.1, 0]), W, H, dpr);
      expect(r.length).toBe(BARS);
      const steps = r.slice(1).map((b, i) => b.x - r[i].x);
      expect(new Set(steps).size).toBe(1);
      expect(new Set(r.map((b) => b.w)).size).toBe(1);
      for (const b of r) {
        expect(Number.isInteger(b.x) && Number.isInteger(b.y) && Number.isInteger(b.h)).toBe(true);
        expect(b.x).toBeGreaterThanOrEqual(0);
        expect(b.x + b.w).toBeLessThanOrEqual(W);
        expect(b.y).toBeGreaterThanOrEqual(0);
        expect(b.y + b.h).toBeLessThanOrEqual(H);
      }
      // Centered: the space left and right differs by at most one device pixel.
      const right = W - (r[BARS - 1].x + r[BARS - 1].w);
      expect(Math.abs(right - r[0].x)).toBeLessThanOrEqual(1);
    });
  }

  it("draws silence as one even row of small dots", () => {
    const r = barRects(flat(), 34, 16, 1);
    expect(new Set(r.map((b) => `${b.y}/${b.h}`)).size).toBe(1);
    expect(r[0].h).toBeLessThanOrEqual(3);
  });

  it("gives louder bars more height", () => {
    const r = barRects(waveform([1]), 34, 16, 1);
    const mid = (BARS - 1) / 2;
    expect(r[mid].h).toBeGreaterThan(r[0].h);
  });
});

describe("line wave", () => {
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

  it("offers bars first, then the line", () => {
    expect(WAVE_STYLES).toEqual(["bars", "line"]);
  });
});
