/**
 * @file Selects the most faithful available HTML renderer for a local man page.
 *
 * It prefers source-fed mandoc, falls back to man-db/groff when source features
 * are unsupported, and leaves all OS process work to the shared process layer.
 */

import {
  commandError,
  getDecompressor,
  runCommand,
  type CommandRunner,
} from "./process";

// Kept as a public re-export so existing callers can type injected runners
// without learning about the internal process module.
export type { CommandResult } from "./process";

export interface FetchManHtmlDependencies {
  runCommand?: CommandRunner;
  readFile?: (path: string) => Promise<Uint8Array>;
  isMandocAvailable?: () => boolean;
  onMandocFallback?: (topic: string, error: Error) => void;
}

const decoder = new TextDecoder();

function decode(bytes: Uint8Array): string {
  return decoder.decode(bytes);
}

async function readFile(path: string): Promise<Uint8Array> {
  return new Uint8Array(await Bun.file(path).arrayBuffer());
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function hasSourceInclude(source: Uint8Array): boolean {
  // mandoc resolves .so relative to its current working directory.  Feeding a
  // page through stdin loses the source file's location, so delegate these
  // pages to man-db/groff, which resolves the include in the man hierarchy.
  return /^(?:\.|')so[\t ]+/m.test(decode(source));
}

async function locateManPage(
  topic: string,
  commandRunner: CommandRunner,
): Promise<string | null> {
  const command = ["man", "-w", topic];
  const result = await commandRunner(command);
  if (result.exitCode !== 0) return null;

  const firstPath = decode(result.stdout).trim().split(/\r?\n/, 1)[0];
  return firstPath || null;
}

async function loadManSource(
  path: string,
  commandRunner: CommandRunner,
  fileReader: (path: string) => Promise<Uint8Array>,
): Promise<Uint8Array> {
  const decompressor = getDecompressor(path);
  if (!decompressor) return fileReader(path);

  const command = [decompressor, path];
  const result = await commandRunner(command);
  if (result.exitCode !== 0) throw commandError(command, result);
  return result.stdout;
}

async function renderWithMandoc(
  source: Uint8Array,
  commandRunner: CommandRunner,
): Promise<string> {
  // -Wunsupp turns mandoc's known GNU roff incompatibilities into a non-zero
  // exit code.  That lets us fall back to groff instead of silently returning
  // incomplete HTML for pages mandoc cannot faithfully render.
  const command = ["mandoc", "-Wunsupp", "-Thtml"];
  const result = await commandRunner(command, { stdin: source });
  if (result.exitCode !== 0) throw commandError(command, result);

  const html = decode(result.stdout);
  if (!html.trim()) throw new Error("mandoc produced no HTML");
  return html;
}

async function renderWithMan(
  topic: string,
  commandRunner: CommandRunner,
): Promise<string> {
  // man-db documents -Thtml as the stdout-oriented groff device option.
  // Do not use --html here: that option launches a browser instead.
  const command = ["man", "-Thtml", topic];
  const result = await commandRunner(command);
  if (result.exitCode !== 0) throw commandError(command, result);
  return decode(result.stdout);
}

function defaultMandocFallback(topic: string, error: Error): void {
  if (process.env.MANT_DEBUG) {
    console.warn(`mandoc failed for ${topic}, falling back to man: ${error.message}`);
  }
}

/**
 * Creates a man-page HTML fetcher.  The injectable process and file adapters
 * keep the renderer selection deterministic in tests without requiring the
 * host to have man-db, mandoc, or compression utilities installed.
 */
export function createManHtmlFetcher(
  dependencies: FetchManHtmlDependencies = {},
): (topic: string) => Promise<string> {
  const commandRunner = dependencies.runCommand ?? runCommand;
  const fileReader = dependencies.readFile ?? readFile;
  const isMandocAvailable = dependencies.isMandocAvailable ?? (() => Bun.which("mandoc") !== null);
  const onMandocFallback = dependencies.onMandocFallback ?? defaultMandocFallback;

  return async function fetchManHtml(topic: string): Promise<string> {
    if (isMandocAvailable()) {
      const path = await locateManPage(topic, commandRunner);

      if (path) {
        try {
          const source = await loadManSource(path, commandRunner, fileReader);
          if (hasSourceInclude(source)) {
            throw new Error("manual source contains a .so include");
          }
          return await renderWithMandoc(source, commandRunner);
        } catch (error) {
          onMandocFallback(topic, asError(error));
        }
      }
    }

    return renderWithMan(topic, commandRunner);
  };
}

export const fetchManHtml = createManHtmlFetcher();
