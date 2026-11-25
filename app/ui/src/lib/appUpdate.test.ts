import { describe, expect, it } from "vitest";
import { progressText, progressValue } from "./appUpdate";

describe("update now progress", () => {
  it("shows nothing before it starts", () => {
    expect(progressText(null)).toBe("");
    expect(progressValue(null)).toBeNull();
  });
  it("shows the download percent when the size is known", () => {
    expect(progressText({ stage: "download", percent: 42 })).toBe("Downloading the update… 42%");
    expect(progressValue({ stage: "download", percent: 42 })).toBe(42);
  });
  it("shows a plain download line when the size is unknown", () => {
    expect(progressText({ stage: "download" })).toBe("Downloading the update…");
    expect(progressValue({ stage: "download", percent: null })).toBeNull();
  });
  it("says the app closes and comes back while installing", () => {
    expect(progressText({ stage: "install" })).toMatch(/closes now and opens again/);
    expect(progressValue({ stage: "install" })).toBe(100);
  });
  it("keeps the bar in range", () => {
    expect(progressValue({ stage: "download", percent: 140 })).toBe(100);
  });
});
