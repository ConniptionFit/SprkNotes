import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../app.css", import.meta.url), "utf8");
const prefsSource = readFileSync(new URL("./prefs.svelte.ts", import.meta.url), "utf8");

// Throwback Mode swaps only the app icon (tray and window). Any style that depends on it would
// change fonts, colors or spacing and shift the window when the switch is flipped.
describe("Throwback Mode", () => {
  it("has no styles of its own", () => {
    expect(css).not.toMatch(/data-brand/);
    expect(css).not.toMatch(/throwback/i);
  });

  it("does not mark the page", () => {
    expect(prefsSource).not.toMatch(/data-brand/);
  });

  it("defines the font once, for both looks", () => {
    expect(css.match(/--font\s*:/g)?.length).toBe(1);
  });
});
