#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";

const URL_NAMESPACE = Buffer.from("6ba7b8119dad11d180b400c04fd430c8", "hex");

function uuidV5(name) {
  const bytes = createHash("sha1")
    .update(URL_NAMESPACE)
    .update(name, "utf8")
    .digest()
    .subarray(0, 16);

  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  const hex = bytes.toString("hex");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join("-");
}

const [path] = process.argv.slice(2);
if (!path) {
  throw new Error("usage: finalize-cyclonedx.mjs PATH");
}

const raw = await readFile(path);
const bom = JSON.parse(raw.toString("utf8"));
if (bom.bomFormat !== "CycloneDX" || !bom.specVersion) {
  throw new Error(`${path}: expected a CycloneDX JSON document`);
}

// cargo-cyclonedx intentionally omits its random serial number when
// SOURCE_DATE_EPOCH is set. Derive a stable UUID from the otherwise complete
// document so the result stays reproducible and actions/attest can recognize it.
// Remove a prior value first so retrying this normalization is idempotent.
delete bom.serialNumber;
const normalized = `${JSON.stringify(bom, null, 2)}\n`;
const digest = createHash("sha256").update(normalized).digest("hex");
bom.serialNumber = `urn:uuid:${uuidV5(`sha256:${digest}`)}`;

await writeFile(path, `${JSON.stringify(bom, null, 2)}\n`, "utf8");
console.log(`${path}: ${bom.serialNumber}`);
