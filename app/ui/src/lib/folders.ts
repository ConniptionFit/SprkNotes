import type { LibraryEntry } from "./types";

/** Which transcripts the list shows: every one, the top of the library, or one folder (with its subfolders). */
export type FolderFilter = { kind: "all" } | { kind: "folder"; path: string };

export const ALL: FolderFilter = { kind: "all" };

/** Folders in tree order (each one right after its parent), with how deep each sits. */
export function tree(folders: string[]): { path: string; name: string; depth: number }[] {
  const parts = (p: string) => p.split("/").map((s) => s.toLowerCase());
  const sorted = [...folders].sort((a, b) => {
    const x = parts(a);
    const y = parts(b);
    for (let i = 0; i < Math.min(x.length, y.length); i++) {
      if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
    }
    return x.length - y.length;
  });
  return sorted.map((path) => {
    const segs = path.split("/");
    return { path, name: segs[segs.length - 1], depth: segs.length - 1 };
  });
}

/** True when the transcript sits in `filter` ("" is the top of the library, not its subfolders). */
export function shows(e: LibraryEntry, filter: FolderFilter): boolean {
  if (filter.kind === "all") return true;
  if (filter.path === "") return e.folder === "";
  return e.folder === filter.path || e.folder.startsWith(filter.path + "/");
}

/** A folder's path after its own name changed, or the same path when it is outside the renamed one. */
export function renamed(path: string, from: string, to: string): string {
  if (path === from) return to;
  if (path.startsWith(from + "/")) return to + path.slice(from.length);
  return path;
}
