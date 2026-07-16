import { describe, expect, test } from "bun:test";
import { fetchRoffAst } from "../../../src/core";

const sidecarPath = new URL(
  "../../../native/bin/mant-mandoc-json",
  import.meta.url,
).pathname;

const canRunSidecar = Bun.which("man") !== null
  && Bun.spawnSync([sidecarPath, "--help"], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0
  && Bun.spawnSync(["man", "-w", "ls"], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0;

const describeSidecar = canRunSidecar ? describe : describe.skip;

describeSidecar("bundled libmandoc AST sidecar", () => {
  test("parses a compressed real man page without invoking system mandoc", async () => {
    const { document, diagnostics } = await fetchRoffAst("ls");

    expect(document.schema).toBe("mant.roff-ast/v1");
    expect(document.engine).toEqual({ name: "libmandoc", version: "1.14.6" });
    expect(document.macroSet).toBe("man");
    // Distros may intentionally leave TH's title argument empty; structure,
    // not a distribution-specific metadata fallback, is the contract here.
    expect(document.meta.hasBody).toBe(true);
    expect(document.root?.children.some((node) => node.macro === "SH")).toBe(true);
    expect(diagnostics).toEqual(expect.any(Array));
  });
});
