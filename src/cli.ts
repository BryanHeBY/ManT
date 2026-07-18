#!/usr/bin/env bun
import { query } from "./query";
import { fetchRoffAst } from "./core";
import { updateTldrCache } from "./tldr";
import { runTui } from "./ui/app";

async function main() {
  const args = process.argv.slice(2);

  let format: "tui" | "json" = "tui";
  let roffAst = false;
  let updateTldr = false;
  const topicParts: string[] = [];

  for (const arg of args) {
    if (arg === "--json" || arg === "-j") {
      format = "json";
    } else if (arg === "--roff-ast") {
      roffAst = true;
    } else if (arg === "--update-tldr") {
      updateTldr = true;
    } else if (arg.startsWith("-")) {
      throw new Error(`Unknown option: ${arg}`);
    } else {
      topicParts.push(arg);
    }
  }

  if (updateTldr) {
    if (topicParts.length > 0 || roffAst || format !== "tui") {
      throw new Error("--update-tldr cannot be combined with a topic or output option");
    }
    const result = await updateTldrCache();
    console.log(`tldr cache ${result.action}: ${result.cacheDir}${result.revision ? ` (${result.revision})` : ""}`);
    return;
  }

  const topic = topicParts.join(" ");
  if (!topic) {
    console.error("Usage: mant <topic> [--json | --roff-ast]\n       mant --update-tldr");
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
