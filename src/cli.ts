#!/usr/bin/env bun
import { query } from "./query";
import { fetchRoffAst } from "./core";
import { runTui } from "./ui/app";

async function main() {
  const args = process.argv.slice(2);

  let format: "tui" | "json" = "tui";
  let roffAst = false;
  let topic = "";

  for (const arg of args) {
    if (arg === "--json" || arg === "-j") {
      format = "json";
    } else if (arg === "--roff-ast") {
      roffAst = true;
    } else if (!topic) {
      topic = arg;
    }
  }

  if (!topic) {
    console.error("Usage: mant <topic> [--json | --roff-ast]");
    process.exit(1);
  }

  if (roffAst) {
    console.log(JSON.stringify(await fetchRoffAst(topic), null, 2));
    process.exit(0);
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
