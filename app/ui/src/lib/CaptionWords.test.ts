import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import CaptionWords from "./CaptionWords.svelte";

const text = (html: string) => html.replace(/<!--[^]*?-->/g, "").replace(/<[^>]+>/g, "");

describe("overlay live words", () => {
  it("keeps a space between every word, like the main window", () => {
    const words = ["She'll", "pull", "your", "corn", "from", "the", "breaks", "again."];
    const { body } = render(CaptionWords, { props: { words, stable: 3, delay: () => 0 } });
    expect(text(body).trim()).toBe(words.join(" "));
  });
  it("marks only the unsettled words as tentative", () => {
    const { body } = render(CaptionWords, { props: { words: ["a", "b", "c"], stable: 2, delay: () => 0 } });
    expect((body.match(/class="w[^"]*tent/g) ?? []).length).toBe(1);
  });
});
