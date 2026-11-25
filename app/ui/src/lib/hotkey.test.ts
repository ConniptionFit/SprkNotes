import { describe, expect, it } from "vitest";
import { comboFromEvent, hotkeyProblem } from "./hotkey";

const press = (code: string, mods: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean } = {}) => ({
  key: "",
  code,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
  metaKey: !!mods.meta,
});

describe("shortcut capture", () => {
  it("reads a combination from the keys pressed", () => {
    expect(comboFromEvent(press("KeyS", { ctrl: true, alt: true }))).toBe("Ctrl+Alt+S");
    expect(comboFromEvent(press("F9", { ctrl: true, shift: true }))).toBe("Ctrl+Shift+F9");
    expect(comboFromEvent(press("Digit7", { meta: true }))).toBe("Win+7");
    expect(comboFromEvent(press("Space", { alt: true }))).toBe("Alt+Space");
  });

  it("waits while only modifiers are held, and refuses keys that would get in the way of typing", () => {
    expect(comboFromEvent(press("ControlLeft", { ctrl: true }))).toBeNull();
    expect(comboFromEvent(press("KeyS"))).toBeNull();
    expect(comboFromEvent(press("KeyS", { shift: true }))).toBeNull();
    expect(comboFromEvent(press("Tab", { ctrl: true }))).toBeNull();
  });

  it("explains why keys were refused", () => {
    expect(hotkeyProblem("taken")).toMatch(/Another app/);
    expect(hotkeyProblem(new Error("x"))).toMatch(/did not accept/);
  });
});
