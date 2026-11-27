import { describe, expect, it } from "vitest";
import { Elapsed, clockText, stampMs, stampText } from "./clock";

describe("stamps", () => {
  it("read and write HH:MM:SS", () => {
    expect(stampMs("00:12:41")).toBe(761_000);
    expect(stampMs("01:00:00")).toBe(3_600_000);
    expect(stampMs("12:41")).toBeNull();
    expect(stampMs(undefined)).toBeNull();
    expect(stampText(761_999)).toBe("00:12:41");
    expect(stampText(-5)).toBe("00:00:00");
  });
  it("the chip shows minutes and seconds, with hours only past an hour", () => {
    expect(clockText(0)).toBe("00:00");
    expect(clockText(761_000)).toBe("12:41");
    expect(clockText(3_723_000)).toBe("1:02:03");
  });
});

describe("elapsed time", () => {
  it("counts only while running", () => {
    const e = new Elapsed();
    e.run(1000);
    expect(e.at(6000)).toBe(5000);
    e.hold(6000);
    expect(e.at(60_000)).toBe(5000);
    e.run(60_000);
    expect(e.at(61_000)).toBe(6000);
  });
  it("running twice does not restart the count", () => {
    const e = new Elapsed();
    e.run(0);
    e.run(4000);
    expect(e.at(5000)).toBe(5000);
  });
  it("a later stamp moves it forward, an earlier one does not move it back", () => {
    const e = new Elapsed();
    e.run(0);
    e.seen("00:10:00", 1000);
    expect(e.at(3000)).toBe(602_000);
    e.seen("00:00:01", 3000);
    expect(e.at(3000)).toBe(602_000);
  });
  it("reset goes back to zero", () => {
    const e = new Elapsed();
    e.run(0);
    e.reset();
    expect(e.at(9000)).toBe(0);
  });
});
