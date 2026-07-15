#!/usr/bin/env bun
import { query } from "./query";
import { runTui } from "./ui/app";

async function main() {
  const args = process.argv.slice(2);

  let format: "tui" | "json" = "tui";
  let topic = "";

  for (const arg of args) {
    if (arg === "--json" || arg === "-j") {
      format = "json";
    } else if (!topic) {
      topic = arg;
    }
  }

  if (!topic) {
    console.error("Usage: mant <topic> [--json]");
    process.exit(1);
  }

  const result = await query({ topic });

  if (format === "json") {
    console.log(JSON.stringify(result, null, 2));
    process.exit(0);
  }

  await runTui(result);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
