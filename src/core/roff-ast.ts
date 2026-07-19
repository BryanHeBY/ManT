/**
 * @file Fetches source-level roff ASTs through the bundled libmandoc sidecar.
 *
 * The sidecar protocol isolates Bun from the libmandoc ABI while this module
 * handles source discovery, temporary decompression, and JSON validation.
 */

import { dirname, join } from "node:path";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import {
  commandError,
  getDecompressor,
  runCommand,
  type CommandRunner,
} from "./process";
import { getBundledSidecarPath } from "./sidecar-cache";

export type RoffAstResultLevel =
  | "ok"
  | "style"
  | "warning"
  | "error"
  | "unsupported"
  | "bad-argument"
  | "system-error";

export interface RoffAstNode {
  kind:
    | "root"
    | "block"
    | "head"
    | "body"
    | "tail"
    | "element"
    | "text"
    | "comment"
    | "table"
    | "equation";
  macro?: string;
  text?: string;
  loc: { line: number; column: number };
  flags: { generated: boolean; sentenceEnd: boolean; noPrint: boolean };
  children: RoffAstNode[];
}

export interface RoffAstDocument {
  schema: "mant.roff-ast/v1";
  engine: { name: "libmandoc"; version: string };
  source: { path: string };
  macroSet: "man" | "mdoc" | "none" | "unknown";
  resultLevel: RoffAstResultLevel;
  meta: {
    title: string | null;
    section: string | null;
    volume: string | null;
    os: string | null;
    name: string | null;
    aliasTarget: string | null;
    hasBody: boolean;
  };
  root: RoffAstNode | null;
}

export interface RoffAstResult {
  document: RoffAstDocument;
  diagnostics: string[];
}

export interface RoffAstFetcherDependencies {
  runCommand?: CommandRunner;
  getSidecarPath?: () => string | Promise<string>;
  isSidecarAvailable?: (path: string) => Promise<boolean>;
}

const decoder = new TextDecoder();

function defaultSidecarPath(): string | Promise<string> {
  return getBundledSidecarPath();
}

async function defaultIsSidecarAvailable(path: string): Promise<boolean> {
  return Bun.file(path).exists();
}

async function prepareSourceFile(
  path: string,
  commandRunner: CommandRunner,
): Promise<string> {
  const decompressor = getDecompressor(path);
  if (!decompressor) return path;

  // libmandoc only reads plain roff source.  Modern man pages are often
  // compressed with zstd, so stream them through the matching decompressor
  // into a temporary file and hand that path to the sidecar.
  const tmpDir = await mkdtemp(join(tmpdir(), "mant-roff-"));
  const tmpPath = join(tmpDir, "source.roff");

  try {
    const command = [decompressor, path];
    const result = await commandRunner(command);
    if (result.exitCode !== 0) throw commandError(command, result);

    await writeFile(tmpPath, result.stdout);
    return tmpPath;
  } catch (error) {
    await removeTemporaryDirectory(tmpDir);
    throw error;
  }
}

async function cleanupSourceFile(path: string, originalPath: string): Promise<void> {
  if (path === originalPath) return;
  await removeTemporaryDirectory(dirname(path));
}

async function removeTemporaryDirectory(path: string): Promise<void> {
  try {
    await rm(path, { recursive: true, force: true });
  } catch (error) {
    // Cleanup trouble should not hide a more useful renderer result or error.
    // Developers can still inspect it explicitly when diagnosing the host.
    if (process.env.MANT_DEBUG) {
      console.warn(`could not remove temporary roff directory ${path}: ${String(error)}`);
    }
  }
}

function parseDocument(output: Uint8Array): RoffAstDocument {
  let document: unknown;
  try {
    document = JSON.parse(decoder.decode(output));
  } catch (error) {
    throw new Error(`bundled mandoc sidecar returned invalid JSON: ${String(error)}`);
  }

  if (
    !document
    || typeof document !== "object"
    || (document as { schema?: unknown }).schema !== "mant.roff-ast/v1"
  ) {
    throw new Error("bundled mandoc sidecar returned an unknown AST schema");
  }
  return document as RoffAstDocument;
}

async function locateManPage(topic: string, commandRunner: CommandRunner): Promise<string> {
  const command = ["man", "-w", topic];
  const result = await commandRunner(command);
  if (result.exitCode !== 0) throw commandError(command, result);

  const path = decoder.decode(result.stdout).trim().split(/\r?\n/, 1)[0];
  if (!path) throw new Error(`man did not return a source path for ${topic}`);
  return path;
}

/**
 * Creates a source-level roff AST fetcher.  It keeps libmandoc behind a
 * versioned JSON sidecar so that the Bun process never links a system ABI.
 */
export function createRoffAstFetcher(
  dependencies: RoffAstFetcherDependencies = {},
): (topic: string) => Promise<RoffAstResult> {
  const commandRunner = dependencies.runCommand ?? runCommand;
  const getSidecarPath = dependencies.getSidecarPath ?? defaultSidecarPath;
  const isSidecarAvailable = dependencies.isSidecarAvailable ?? defaultIsSidecarAvailable;

  return async function fetchRoffAst(topic: string): Promise<RoffAstResult> {
    const sidecar = await getSidecarPath();
    if (!(await isSidecarAvailable(sidecar))) {
      throw new Error(
        "bundled mandoc sidecar is unavailable; run bun run build:mandoc-json",
      );
    }

    const sourcePath = await locateManPage(topic, commandRunner);
    const parsedPath = await prepareSourceFile(sourcePath, commandRunner);
    try {
      const command = [sidecar, parsedPath];
      const result = await commandRunner(command);
      if (result.exitCode !== 0) throw commandError(command, result);

      return {
        document: parseDocument(result.stdout),
        diagnostics: result.stderr.trim()
          ? result.stderr.trim().split(/\r?\n/)
          : [],
      };
    } finally {
      await cleanupSourceFile(parsedPath, sourcePath);
    }
  };
}

export const fetchRoffAst = createRoffAstFetcher();
