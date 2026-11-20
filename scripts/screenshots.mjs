// Regenerates docs/screenshots from the UI running against its browser mock (synthetic text only).
// Usage: cd app/ui && npm ci && npx vite --port 1420 &   then   node scripts/screenshots.mjs
// Needs Playwright with Chromium. Re-run after any visual change (for example a rebrand).
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const base = process.env.UI_URL ?? "http://localhost:1420/";
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "docs", "screenshots");
mkdirSync(out, { recursive: true });
const browser = await chromium.launch();
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function page(scheme, { w = 900, h = 620, hash = "", query = "", prefs } = {}) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: 2, colorScheme: scheme });
  if (prefs) await ctx.addInitScript((p) => localStorage.setItem("sprknotes.prefs", JSON.stringify(p)), prefs);
  const p = await ctx.newPage();
  await p.goto(`${base}${query}${hash}`);
  await wait(400);
  return p;
}
const shot = (p, name, target) => (target ?? p).screenshot({ path: join(out, name) });

for (const scheme of ["light", "dark"]) {
  // Main window: ready, transcribing (live line), paused.
  let p = await page(scheme);
  await shot(p, `main-ready-${scheme}.png`);
  await p.getByRole("button", { name: /start/i }).click();
  await wait(4200);
  await p.waitForSelector(".line.live .text"); // a line still being spoken, shown word by word
  await shot(p, `main-transcribing-${scheme}.png`);
  await p.getByRole("button", { name: /pause/i }).click();
  await wait(500);
  await shot(p, `main-paused-${scheme}.png`);
  await p.context().close();

  // After a crash: the unexpected-close prompt (offered once at launch).
  p = await page(scheme, { query: "?recover" });
  await wait(500);
  await shot(p, `recovery-${scheme}.png`);
  await p.context().close();

  // Naming a session and turning a source off.
  p = await page(scheme);
  await p.getByLabel("Session name").fill("Budget sync");
  await p.locator(".chip.me input").uncheck({ force: true });
  await wait(300);
  await shot(p, `main-named-${scheme}.png`);
  await p.context().close();

  // Scrolled up while lines keep arriving: the "Return to live" pill.
  p = await page(scheme, { h: 360 });
  await p.getByRole("button", { name: /start/i }).click();
  await wait(9000);
  await p.evaluate(() => {
    const el = [...document.querySelectorAll("*")].find((e) => e.scrollHeight > e.clientHeight + 20 && /auto|scroll/.test(getComputedStyle(e).overflowY));
    if (el) el.scrollTop = 0;
  });
  await wait(2500);
  await shot(p, `main-return-to-live-${scheme}.png`);
  await p.context().close();

  // Library list, an open transcript, and the Summarize outcomes.
  p = await page(scheme);
  await p.getByRole("button", { name: "Library" }).click();
  await wait(500);
  await shot(p, `library-${scheme}.png`);
  await p.locator("button.item").first().click();
  await wait(500);
  await shot(p, `library-open-${scheme}.png`);
  await p.context().close();
  for (const apps of ["both", "none"]) {
    p = await page(scheme, { query: `?apps=${apps}` });
    await wait(400);
    await p.getByRole("button", { name: "Summarize" }).click();
    await wait(500);
    await shot(p, `summarize-${apps === "both" ? "choose" : "no-app"}-${scheme}.png`);
    await p.context().close();
  }

  // Throwback Mode (Settings, Appearance): the first look.
  const tb = { throwback: true };
  p = await page(scheme, { prefs: tb });
  await shot(p, `throwback-ready-${scheme}.png`);
  await p.getByRole("button", { name: /start/i }).click();
  await wait(4200);
  await p.waitForSelector(".line.live .text");
  await shot(p, `throwback-transcribing-${scheme}.png`);
  await p.context().close();
  p = await page(scheme, { h: 900, prefs: tb });
  await p.getByRole("button", { name: "Settings" }).click();
  await wait(300);
  await shot(p, `settings-throwback-${scheme}.png`);
  await p.context().close();

  // Settings: model ready, missing, downloading, update available, managed by policy, captions.
  for (const [model, tag] of [["", "ready"], ["missing", "model-missing"], ["update", "model-update"], ["managed", "model-managed"]]) {
    p = await page(scheme, { query: model ? `?model=${model}` : "" });
    await p.getByRole("button", { name: "Settings" }).click();
    await wait(400);
    await shot(p, `settings-${tag}-${scheme}.png`);
    if (model === "missing") {
      await p.getByRole("button", { name: /download/i }).first().click();
      await wait(1600);
      await shot(p, `settings-model-downloading-${scheme}.png`);
    }
    await p.context().close();
  }
  p = await page(scheme, { h: 900 });
  await p.getByRole("button", { name: "Settings" }).click();
  await wait(300);
  await shot(p, `settings-full-${scheme}.png`);
  await p.context().close();

  // First run: no model yet.
  p = await page(scheme, { query: "?model=missing" });
  await shot(p, `first-run-model-${scheme}.png`);
  await p.context().close();

  // Overlay: collapsed, hover (buttons), captions drawer (Others only), flipped, zoomed text.
  const ov = async (extra, opts = {}) => {
    const q = await page(scheme, { w: 760, h: 420, hash: "#mini", prefs: { captionsOn: true, ...extra }, ...opts });
    return q;
  };
  p = await page(scheme, { w: 420, h: 120, hash: "#mini", prefs: { captionsOn: false } });
  await shot(p, `overlay-ready-${scheme}.png`, p.locator(".frame"));
  await p.hover(".frame");
  await wait(300);
  await shot(p, `overlay-hover-${scheme}.png`, p.locator(".frame"));
  await p.context().close();

  p = await ov({});
  await p.hover(".orb");
  await p.getByRole("button", { name: "Start" }).click();
  await wait(5200);
  await shot(p, `overlay-captions-${scheme}.png`, p.locator(".frame"));
  await p.context().close();

  p = await page(scheme, { w: 420, h: 120, hash: "#mini", prefs: { captionsOn: false } });
  await p.hover(".orb");
  await p.getByRole("button", { name: "Start" }).click();
  await wait(2500);
  await p.hover(".orb");
  await p.getByRole("button", { name: "Pause" }).click();
  await wait(500);
  await p.hover(".orb");
  await wait(300);
  await shot(p, `overlay-paused-${scheme}.png`, p.locator(".frame"));
  await p.context().close();

  p = await ov({ captionsWho: "both" });
  await p.hover(".orb");
  await p.getByRole("button", { name: "Start" }).click();
  await wait(5200);
  await shot(p, `overlay-captions-both-${scheme}.png`, p.locator(".frame"));
  await p.context().close();

  p = await ov({ captionText: 5, captionW: 640, captionH: 200 });
  await p.hover(".orb");
  await p.getByRole("button", { name: "Start" }).click();
  await wait(5200);
  await shot(p, `overlay-captions-large-${scheme}.png`, p.locator(".frame"));
  await p.context().close();

  p = await ov({}, { query: "?flip" });
  await p.hover(".orb");
  await p.getByRole("button", { name: "Start" }).click();
  await wait(5200);
  await shot(p, `overlay-captions-flipped-${scheme}.png`, p.locator(".frame"));
  await p.context().close();
}
await browser.close();
