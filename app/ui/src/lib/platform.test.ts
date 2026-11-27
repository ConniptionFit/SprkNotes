import { describe, expect, it } from "vitest";
import { isPhone } from "./platform";

describe("isPhone", () => {
  it("knows an Android WebView", () => {
    expect(isPhone("Mozilla/5.0 (Linux; Android 15; SM-S931U Build/AP3A; wv) AppleWebKit/537.36 Chrome/131.0 Mobile Safari/537.36")).toBe(true);
  });
  it("never takes the desktop app for a phone", () => {
    expect(isPhone("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0 Safari/537.36 Edg/131.0")).toBe(false);
    expect(isPhone("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)")).toBe(false);
  });
});
