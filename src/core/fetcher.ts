/**
 * @file Selects the most faithful available HTML renderer for a local man page.
 *
 * It prefers the bundled libmandoc HTML renderer, falls back to man-db/groff
 * for unsupported source features, and leaves process work to the shared
 * boundary. Development checkouts can use a system mandoc before the sidecar
 * has been built.
 */

import { basename, dirname } from "node:path";
import {
  commandError,
  runCommand,
  type CommandRunner,
} from "./process";
import { getBundledSidecarPath } from "./sidecar-cache";

// Kept as a public re-export so existing callers can type injected runners
// without learning about the internal process module.
export type { CommandResult } from "./process";

export interface FetchManHtmlDependencies {
  runCommand?: CommandRunner;
  getSidecarPath?: () => string | Promise<string>;
  isSidecarAvailable?: (path: string) => Promise<boolean>;
  isMandocAvailable?: () => boolean;
  isManHtmlAvailable?: () => boolean;
  onMandocFallback?: (topic: string, error: Error) => void;
}

const decoder = new TextDecoder();

function decode(bytes: Uint8Array): string {
  return decoder.decode(bytes);
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

async function defaultIsSidecarAvailable(path: string): Promise<boolean> {
  return Bun.file(path).exists();
}

function defaultIsManHtmlAvailable(): boolean {
  const man = Bun.which("man");
  // Apple's BSD man rejects -T. A separately installed man-db remains a valid
  // high-fidelity fallback and normally resolves outside /usr/bin.
  return man !== null && !(process.platform === "darwin" && man === "/usr/bin/man");
}

interface HtmlAttempt {
  /** HTML is retained even when strict unsupported-feature checks fail. */
  html: string | null;
  error: Error | null;
}

/** Returns the cwd mandoc expects for conventional `.so man1/target.1` paths. */
function manualTreeRoot(path: string): string {
  const sourceDirectory = dirname(path);
  return /^(?:man|cat)[^/]*$/.test(basename(sourceDirectory))
    ? dirname(sourceDirectory)
    : sourceDirectory;
}

async function attemptHtmlRender(
  command: string[],
  commandRunner: CommandRunner,
  cwd?: string,
): Promise<HtmlAttempt> {
  const result = await commandRunner(command, cwd ? { cwd } : {});
  const html = decode(result.stdout);
  if (result.exitCode === 0 && html.trim()) return { html, error: null };

  const error = result.exitCode === 0
    ? new Error(`${command[0]} produced no HTML`)
    : commandError(command, result);
  return { html: html.trim() ? html : null, error };
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
  const html = decode(result.stdout);
  if (!html.trim()) throw new Error(`man produced no HTML for '${topic}'`);
  return html;
}

function defaultMandocFallback(topic: string, error: Error): void {
  if (process.env.MANT_DEBUG) {
    console.warn(`strict mandoc rendering failed for ${topic}: ${error.message}`);
  }
}

function errorSummary(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.trim().split(/\r?\n/, 1)[0] || "unknown renderer error";
}

function rendererError(topic: string, mandocError: Error, manError: unknown): Error {
  return new Error(
    `no HTML renderer could render '${topic}': `
    + `mandoc: ${errorSummary(mandocError)}; `
    + `man/groff: ${errorSummary(manError)}`,
  );
}

/**
 * Creates a man-page HTML fetcher. Injectable process and sidecar adapters
 * keep renderer selection deterministic without requiring host renderers in
 * unit tests.
 */
export function createManHtmlFetcher(
  dependencies: FetchManHtmlDependencies = {},
): (topic: string) => Promise<string> {
  const commandRunner = dependencies.runCommand ?? runCommand;
  const getSidecarPath = dependencies.getSidecarPath ?? getBundledSidecarPath;
  const isSidecarAvailable = dependencies.isSidecarAvailable ?? defaultIsSidecarAvailable;
  const isMandocAvailable = dependencies.isMandocAvailable ?? (() => Bun.which("mandoc") !== null);
  const isManHtmlAvailable = dependencies.isManHtmlAvailable ?? defaultIsManHtmlAvailable;
  const onMandocFallback = dependencies.onMandocFallback ?? defaultMandocFallback;

  return async function fetchManHtml(topic: string): Promise<string> {
    const path = await locateManPage(topic, commandRunner);
    if (path) {
      let attempt: HtmlAttempt | null = null;

      try {
        const sidecarPath = await getSidecarPath();
        if (await isSidecarAvailable(sidecarPath)) {
          attempt = await attemptHtmlRender(
            [sidecarPath, "--html", path],
            commandRunner,
            manualTreeRoot(path),
          );
        }
      } catch (error) {
        if (process.env.MANT_DEBUG) {
          console.warn(`bundled mandoc sidecar is unavailable: ${errorSummary(error)}`);
        }
      }

      // Keep source paths intact: mandoc can read gzip itself and needs the
      // original location to resolve .so includes. This path is primarily for
      // development before the bundled sidecar has been built.
      if (attempt === null && isMandocAvailable()) {
        attempt = await attemptHtmlRender(
          ["mandoc", "-Wunsupp", "-Thtml", path],
          commandRunner,
          manualTreeRoot(path),
        );
      }

      if (attempt?.error === null && attempt.html !== null) return attempt.html;

      if (attempt?.error) {
        onMandocFallback(topic, attempt.error);
        if (!isManHtmlAvailable()) {
          if (attempt.html !== null) return attempt.html;
          throw rendererError(
            topic,
            attempt.error,
            new Error("the installed man implementation has no HTML device"),
          );
        }
        try {
          return await renderWithMan(topic, commandRunner);
        } catch (manError) {
          // Strict mandoc still emits its best-effort document. On hosts such
          // as macOS where BSD man has no HTML device, that usable output is a
          // better final fallback than rejecting the page entirely.
          if (attempt.html !== null) {
            if (process.env.MANT_DEBUG) {
              console.warn(
                `man/groff fallback failed for ${topic}; using best-effort bundled HTML: `
                + errorSummary(manError),
              );
            }
            return attempt.html;
          }
          throw rendererError(topic, attempt.error, manError);
        }
      }
    }

    if (!isManHtmlAvailable()) {
      throw new Error(
        `could not render manual '${topic}': the installed man implementation has no HTML device`,
      );
    }
    try {
      return await renderWithMan(topic, commandRunner);
    } catch (error) {
      throw new Error(`could not render manual '${topic}': ${errorSummary(error)}`);
    }
  };
}

export const fetchManHtml = createManHtmlFetcher();
