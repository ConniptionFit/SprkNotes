// The one Node API the UI tests use (to read source files). The UI has no Node types of its own.
declare module "node:fs" {
  export function readFileSync(path: URL, encoding: "utf8"): string;
}
