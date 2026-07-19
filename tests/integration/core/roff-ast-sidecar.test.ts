/**
 * @file Verifies the compiled libmandoc sidecar against a real compressed page.
 */

import { describe, expect, test } from "bun:test";
import { fetchRoffAst } from "../../../src/core";

const sidecarPath = new URL(
  "../../../native/bin/mant-mandoc-json",
  import.meta.url,
).pathname;
const mdocFixturePath = new URL(
  "../../fixtures/roff/minimal-mdoc.1",
  import.meta.url,
).pathname;

const canRunSidecar = Bun.spawnSync([sidecarPath, "--help"], {
  stdout: "ignore",
  stderr: "ignore",
}).exitCode === 0;

const canParseRealManPage = canRunSidecar
  && Bun.which("man") !== null
  && Bun.spawnSync(["man", "-w", "ls"], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0;

const describeSidecar = canRunSidecar ? describe : describe.skip;
const describeRealManPage = canParseRealManPage ? describe : describe.skip;

describeSidecar("bundled libmandoc AST sidecar", () => {
  test("derives hasBody from an mdoc syntax tree", async () => {
    const process = Bun.spawn([sidecarPath, mdocFixturePath], {
      stdout: "pipe",
      stderr: "pipe",
    });
    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(process.stdout).text(),
      new Response(process.stderr).text(),
      process.exited,
    ]);

    expect(exitCode).toBe(0);
    expect(stderr).toBe("");
    const document = JSON.parse(stdout);
    expect(document.macroSet).toBe("mdoc");
    expect(document.meta.hasBody).toBe(true);
    expect(document.root.children.some(
      (node: { macro?: string }) => node.macro === "Sh",
    )).toBe(true);
  });
});

describeRealManPage("bundled libmandoc AST sidecar with an installed page", () => {
  test("parses a compressed real man page without invoking system mandoc", async () => {
    const { document, diagnostics } = await fetchRoffAst("ls");

    expect(document.schema).toBe("mant.roff-ast/v1");
    expect(document.engine).toEqual({ name: "libmandoc", version: "1.14.6" });
    // GNU/Linux commonly ships ls(1) in man(7), while macOS ships it in
    // mdoc(7). Both are valid inputs to the sidecar protocol.
    expect(["man", "mdoc"]).toContain(document.macroSet);
    // Distros may intentionally leave the document title empty; structure,
    // not a distribution-specific metadata fallback, is the contract here.
    expect(document.meta.hasBody).toBe(true);
    const sectionMacro = document.macroSet === "mdoc" ? "Sh" : "SH";
    expect(document.root?.children.some((node) => node.macro === sectionMacro)).toBe(true);
    expect(diagnostics).toEqual(expect.any(Array));
  });
});
