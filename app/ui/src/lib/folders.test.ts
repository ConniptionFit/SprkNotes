import { describe, expect, it } from "vitest";
import { ALL, renamed, shows, tree } from "./folders";
import type { LibraryEntry } from "./types";

const entry = (folder: string): LibraryEntry => ({
  path: `/lib/${folder}/x.md`,
  name: "x",
  title: "x",
  date: "2026-11-19",
  start: "09:00",
  duration: "",
  status: "complete",
  folder,
});

describe("tree", () => {
  it("keeps each folder right after its parent and ignores case", () => {
    expect(tree(["clients 2", "Clients/Acme", "Clients", "archive"]).map((f) => [f.path, f.depth])).toEqual([
      ["archive", 0],
      ["Clients", 0],
      ["Clients/Acme", 1],
      ["clients 2", 0],
    ]);
  });
  it("names a folder by its last part", () => {
    expect(tree(["Clients/Acme/2026"])[0].name).toBe("2026");
  });
});

describe("shows", () => {
  it("all shows everything", () => {
    expect(shows(entry("Clients"), ALL)).toBe(true);
  });
  it("the top of the library leaves subfolders out", () => {
    expect(shows(entry(""), { kind: "folder", path: "" })).toBe(true);
    expect(shows(entry("Clients"), { kind: "folder", path: "" })).toBe(false);
  });
  it("a folder includes its subfolders but not a look-alike sibling", () => {
    const f = { kind: "folder", path: "Clients" } as const;
    expect(shows(entry("Clients/Acme"), f)).toBe(true);
    expect(shows(entry("Clients 2"), f)).toBe(false);
  });
});

describe("renamed", () => {
  it("follows the folder and its children only", () => {
    expect(renamed("Clients", "Clients", "Customers")).toBe("Customers");
    expect(renamed("Clients/Acme", "Clients", "Customers")).toBe("Customers/Acme");
    expect(renamed("Clients 2", "Clients", "Customers")).toBe("Clients 2");
  });
});
