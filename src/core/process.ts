/**
 * @file Provides the small process boundary shared by manual renderers.
 *
 * Keeping process execution, compression selection, and error messages in one
 * place makes higher-level fetchers describe renderer policy rather than pipe
 * plumbing. Command runners remain injectable for deterministic tests.
 */

export interface CommandResult {
  stdout: Uint8Array;
  stderr: string;
  exitCode: number;
}

export interface CommandOptions {
  stdin?: Uint8Array;
}

export type CommandRunner = (
  command: string[],
  options?: CommandOptions,
) => Promise<CommandResult>;

/** Executes a command while draining both output pipes concurrently. */
export async function runCommand(
  command: string[],
  options: CommandOptions = {},
): Promise<CommandResult> {
  const process = Bun.spawn(command, {
    stdin: options.stdin ?? "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });

  // Reading stdout before stderr can deadlock if diagnostics fill stderr's
  // pipe. Start both reads before awaiting the process exit status.
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(process.stdout).arrayBuffer(),
    new Response(process.stderr).text(),
    process.exited,
  ]);

  return { stdout: new Uint8Array(stdout), stderr, exitCode };
}

/** Returns the platform decompressor for the compression suffix used by man. */
export function getDecompressor(path: string): string | null {
  if (path.endsWith(".zst")) return "zstdcat";
  if (path.endsWith(".gz")) return "zcat";
  if (path.endsWith(".bz2")) return "bzcat";
  if (path.endsWith(".xz")) return "xzcat";
  return null;
}

/** Builds a useful failure from a non-zero command result. */
export function commandError(command: string[], result: CommandResult): Error {
  return new Error(
    result.stderr.trim() || `${command.join(" ")} failed with code ${result.exitCode}`,
  );
}
