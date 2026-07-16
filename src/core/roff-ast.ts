import { dirname, join } from "node:path";
import type { CommandResult } from "./fetcher";

declare const MANT_COMPILED: boolean;

type CommandRunner = (command: string[]) => Promise<CommandResult>;

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
  getSidecarPath?: () => string;
  isSidecarAvailable?: (path: string) => Promise<boolean>;
}

const decoder = new TextDecoder();

function defaultSidecarPath(): string {
  if (process.env.MANT_MANDOC_JSON_BIN) {
    return process.env.MANT_MANDOC_JSON_BIN;
  }
  if (typeof MANT_COMPILED !== "undefined" && MANT_COMPILED) {
    return join(dirname(process.execPath), "mant-mandoc-json");
  }
  return new URL("../../native/bin/mant-mandoc-json", import.meta.url).pathname;
}

async function defaultIsSidecarAvailable(path: string): Promise<boolean> {
  return Bun.file(path).exists();
}

async function runCommand(command: string[]): Promise<CommandResult> {
  const process = Bun.spawn(command, { stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(process.stdout).arrayBuffer(),
    new Response(process.stderr).text(),
    process.exited,
  ]);
  return { stdout: new Uint8Array(stdout), stderr, exitCode };
}

function commandError(command: string[], result: CommandResult): Error {
  return new Error(
    result.stderr.trim() || `${command.join(" ")} failed with code ${result.exitCode}`,
  );
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
    const sidecar = getSidecarPath();
    if (!(await isSidecarAvailable(sidecar))) {
      throw new Error(
        "bundled mandoc sidecar is unavailable; run bun run build:mandoc-json",
      );
    }

    const sourcePath = await locateManPage(topic, commandRunner);
    const command = [sidecar, sourcePath];
    const result = await commandRunner(command);
    if (result.exitCode !== 0) throw commandError(command, result);

    return {
      document: parseDocument(result.stdout),
      diagnostics: result.stderr.trim()
        ? result.stderr.trim().split(/\r?\n/)
        : [],
    };
  };
}

export const fetchRoffAst = createRoffAstFetcher();
