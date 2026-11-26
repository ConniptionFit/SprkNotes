import { describe, expect, it } from "vitest";
import { isMarkKey } from "./marks";

const press = (code: string, mods: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean; repeat?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
  metaKey: !!mods.meta,
  repeat: !!mods.repeat,
});

describe("isMarkKey", () => {
  it("takes Ctrl+M only", () => {
    expect(isMarkKey(press("KeyM", { ctrl: true }))).toBe(true);
    expect(isMarkKey(press("KeyM"))).toBe(false);
    expect(isMarkKey(press("KeyM", { ctrl: true, shift: true }))).toBe(false);
    expect(isMarkKey(press("KeyM", { ctrl: true, alt: true }))).toBe(false);
    expect(isMarkKey(press("KeyN", { ctrl: true }))).toBe(false);
  });

  it("ignores a held key so one press is one mark", () => {
    expect(isMarkKey(press("KeyM", { ctrl: true, repeat: true }))).toBe(false);
  });
});
