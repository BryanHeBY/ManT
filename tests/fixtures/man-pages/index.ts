import { readFileSync } from "node:fs";
import { resolve } from "node:path";

export function loadManPageFixture(name: string): string {
  const filePath = resolve(import.meta.dir, `${name}.html`);
  return readFileSync(filePath, "utf-8");
}
