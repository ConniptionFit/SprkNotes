import { describe, expect, it } from "vitest";
import { chosenKeys, effective, parseManaged } from "./managed";

const builtIn = { theme: "system", captionsOn: false, liveLine: true, hotkeyKeys: "Ctrl+Alt+S" };

describe("managed preferences", () => {
  it("reads policy text and drops what does not fit", () => {
    const m = parseManaged({
      locked: { LiveCaptions: "1", captionspeakers: "both", Theme: "purple", HotkeyKeys: "S", SummarizeEnabled: "0" },
      defaults: { Theme: "dark", LiveLine: "off" },
    });
    expect(m.locked).toEqual({ captionsOn: true, captionsWho: "both" });
    expect(m.defaults).toEqual({ theme: "dark", liveLine: false });
    expect(m.summarizeOff).toBe(true);
    expect(parseManaged(null).summarizeOff).toBe(false);
  });

  it("locked beats the person's choice, which beats a starting value", () => {
    const m = parseManaged({ locked: { LiveCaptions: "1" }, defaults: { Theme: "dark", LiveLine: "0" } });
    const stored = { theme: "light", captionsOn: false, liveLine: true, chosen: ["theme", "captionsOn"] };
    const p = effective(builtIn, stored, chosenKeys(stored, builtIn), m);
    expect(p.theme).toBe("light"); // chosen by the person
    expect(p.liveLine).toBe(false); // never chosen: the starting value
    expect(p.captionsOn).toBe(true); // locked
  });

  it("reads the sound meter style and keeps a lock on the retired rolling style, as bars", () => {
    expect(parseManaged({ locked: { WaveformStyle: "Line" }, defaults: {} }).locked).toEqual({ waveStyle: "line" });
    expect(parseManaged({ locked: { WaveformStyle: "rolling" }, defaults: {} }).locked).toEqual({ waveStyle: "bars" });
    expect(parseManaged({ locked: { WaveformStyle: "wavy" }, defaults: {} }).locked).toEqual({});
  });

  it("an older store counts only changed values as chosen", () => {
    const stored = { theme: "system", liveLine: false };
    expect([...chosenKeys(stored, builtIn)]).toEqual(["liveLine"]);
  });
});
