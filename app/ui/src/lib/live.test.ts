import { describe, expect, it } from "vitest";
import { Partials, Rows, Typist, commonPrefix, droppedLead, gate, hold, sentences, typed, type GateWord } from "./live";

const cap = (o: Partial<Parameters<Rows["caption"]>[0]>) => ({ speaker: "Others", text: "", stable: 0, final: false, ...o });

describe("locked rows", () => {
  it("sealed sentences become rows in order", () => {
    const r = new Rows();
    r.caption(cap({ utt: 1, sealed: ["One two three.", "Four five six."] }), 0);
    expect(r.rows.map((x) => x.text)).toEqual(["One two three.", "Four five six."]);
  });
  it("the final caption silently replaces its utterance's rows, sentence by sentence", () => {
    const r = new Rows();
    r.caption(cap({ utt: 1, sealed: ["they might add a photo of the roof"] }), 0);
    r.caption(cap({ utt: 2, sealed: ["next thing"] }), 100);
    r.caption(cap({ utt: 1, final: true, text: "They might add a photo of the roof, then more. Another one." }), 200);
    expect(r.rows.map((x) => [x.utt, x.text])).toEqual([
      [2, "next thing"],
      [1, "They might add a photo of the roof, then more."],
      [1, "Another one."],
    ]);
  });
  it("a committed line removes the rows it covers and keeps unrelated ones", () => {
    const r = new Rows();
    r.caption(cap({ utt: 1, sealed: ["Now the insurer needs to verify.", "Something else entirely."] }), 0);
    r.line("Others", "Now, the insurer needs to verify.");
    expect(r.rows.map((x) => x.text)).toEqual(["Something else entirely."]);
  });
  it("a line from Me does not remove Others rows", () => {
    const r = new Rows();
    r.caption(cap({ utt: 1, sealed: ["same words here"] }), 0);
    r.line("Me", "same words here");
    expect(r.rows).toHaveLength(1);
  });
  it("old rows expire", () => {
    const r = new Rows();
    r.caption(cap({ utt: 1, sealed: ["old"] }), 0);
    r.caption(cap({ utt: 2, sealed: ["new"] }), 60_000);
    expect(r.rows.map((x) => x.text)).toEqual(["new"]);
  });
  it("splits sentences", () => {
    expect(sentences("Hi there. How are you? Fine")).toEqual(["Hi there.", "How are you?", "Fine"]);
  });
});

describe("display delay gate", () => {
  it("settled words show at once, tentative words wait", () => {
    let g = gate([], ["a", "b", "c"], 2, 0, 400);
    expect(g.visible).toBe(2);
    g = gate(g.next, ["a", "b", "c"], 2, 399, 400);
    expect(g.visible).toBe(2);
    g = gate(g.next, ["a", "b", "c"], 2, 400, 400);
    expect(g.visible).toBe(3);
  });
  it("a word that flips never shows and restarts its wait", () => {
    let g = gate([], ["a", "ice"], 1, 0, 400);
    g = gate(g.next, ["a", "eyes"], 1, 300, 400);
    g = gate(g.next, ["a", "eyes"], 1, 600, 400);
    expect(g.visible).toBe(1); // 300 ms since it became "eyes"
    g = gate(g.next, ["a", "eyes"], 1, 700, 400);
    expect(g.visible).toBe(2);
  });
});

describe("typing", () => {
  it("types at a steady pace", () => {
    expect(typed(0, 10, 14)).toBeCloseTo(1, 5);
  });
  it("catches up when far behind: most of a long backlog within half a second", () => {
    let shown = 0;
    for (let t = 0; t < 500; t += 16) shown = typed(shown, 300, 16);
    expect(shown).toBeGreaterThan(200);
    for (let t = 0; t < 1500; t += 16) shown = typed(shown, 300, 16);
    expect(shown).toBeGreaterThan(290);
  });
  it("never passes the target", () => {
    expect(typed(9.9, 10, 1000)).toBe(10);
  });
  it("finds where a correction starts", () => {
    expect(commonPrefix("they might add", "they might ask")).toBe(12);
  });
});

describe("calm live row", () => {
  const gw = (words: string[], since = 0): GateWord[] => words.map((word) => ({ word, since }));
  it("a flipping word keeps its old wording until the new one settles, and the tail stays", () => {
    const next = [...gw(["we", "went", "to"]), { word: "town", since: 1000 }, ...gw(["today"], 0)];
    const r = hold(["we", "went", "to", "tone", "today"], next, 0, 1100, 400, null);
    expect(r.shown).toEqual(["we", "went", "to", "tone", "today"]);
    expect(hold(r.shown, next, 0, 1500, 400, null).shown[3]).toBe("town");
  });
  it("shrinks only after a shorter hypothesis persists", () => {
    const next = gw(["we", "went"]);
    const a = hold(["we", "went", "to", "town"], next, 0, 1000, 400, null);
    expect(a.shown).toHaveLength(4);
    expect(hold(a.shown, next, 0, 1700, 400, a.shorterSince).shown).toHaveLength(2);
  });
  it("new words append only once past the gate", () => {
    const next = [...gw(["a", "b"]), { word: "c", since: 900 }];
    expect(hold(["a", "b"], next, 0, 1000, 400, null).shown).toEqual(["a", "b"]);
    expect(hold(["a", "b"], next, 0, 1400, 400, null).shown).toEqual(["a", "b", "c"]);
  });
  it("finds words that locked out of the live row", () => {
    expect(droppedLead(["so", "see", "what", "sox", "wants"], ["sox", "wants", "to"])).toBe(3);
    expect(droppedLead(["so", "see"], ["so", "see", "what"])).toBe(0);
  });
});

describe("one live row per speaker", () => {
  const cap = (speaker: string, utt: number, text: string, final = false) => ({ speaker, utt, text, stable: 0, final });

  it("keeps both speakers when they talk at once, in the order they started", () => {
    const p = new Partials();
    p.caption(cap("Others", 1, "so the plan"));
    p.caption(cap("Me", 2, "right"));
    p.caption(cap("Others", 1, "so the plan is"));
    expect(p.list.map((c) => [c.speaker, c.text])).toEqual([
      ["Others", "so the plan is"],
      ["Me", "right"],
    ]);
  });

  it("ends a speaker's row on their final caption and leaves the other alone", () => {
    const p = new Partials();
    p.caption(cap("Others", 1, "so the plan"));
    p.caption(cap("Me", 2, "right"));
    p.caption(cap("Others", 1, "So the plan.", true));
    expect(p.list.map((c) => c.speaker)).toEqual(["Me"]);
  });

  it("moves a speaker's new utterance under whoever is already speaking", () => {
    const p = new Partials();
    p.caption(cap("Me", 1, "hello"));
    p.caption(cap("Others", 2, "hi there"));
    p.caption(cap("Me", 3, "anyway"));
    expect(p.list.map((c) => c.speaker)).toEqual(["Others", "Me"]);
  });
});

describe("typing one live row", () => {
  it("types a caption out and stays put after it empties, briefly", () => {
    const t = new Typist();
    const c = { speaker: "Me", utt: 1, text: "hello there", stable: 2, final: false };
    t.step(c, 0, 16, 0, true);
    expect(t.shown.map((w) => w.w).join(" ")).toBe("hello there");
    expect(t.recent).toBe(true);
    t.step(null, 50, 16, 0, true);
    expect(t.shown).toEqual([]);
    expect(t.recent).toBe(false);
  });

  it("keeps two typists independent", () => {
    const a = new Typist();
    const b = new Typist();
    a.step({ speaker: "Me", utt: 1, text: "one two", stable: 2, final: false }, 0, 16, 0, true);
    b.step({ speaker: "Others", utt: 2, text: "three", stable: 1, final: false }, 0, 16, 0, true);
    expect(a.shown.map((w) => w.w)).toEqual(["one", "two"]);
    expect(b.shown.map((w) => w.w)).toEqual(["three"]);
  });
});
