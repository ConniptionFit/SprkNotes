import { describe, expect, it } from "vitest";
import { deviceLabel, menuOptions } from "./sources";

describe("source device menu", () => {
  it("starts with the system default, then each device once", () => {
    const o = menuOptions(["USB headset", "Built-in microphone", "USB headset"], null);
    expect(o.map((x) => x.label)).toEqual(["System default", "USB headset", "Built-in microphone"]);
    expect(o[0].value).toBeNull();
  });
  it("keeps an unplugged pick, marked as not connected", () => {
    const o = menuOptions(["Built-in microphone"], "USB headset");
    expect(o.at(-1)).toEqual({ value: "USB headset", label: "USB headset (not connected)", missing: true });
  });
  it("labels the default when nothing is picked", () => {
    expect(deviceLabel(null)).toBe("System default");
    expect(deviceLabel("USB headset")).toBe("USB headset");
  });
});
