/**
 * @file Provides the small subprocess boundary used by the native CLI client.
 *
 * Keeping process execution here lets the client focus on protocol semantics,
 * while tests can inject a deterministic runner without spawning Rust.
 */

export interface CommandResult {
  stdout: Uint8Array;
  stderr: string;
  exitCode: number;
}

export interface CommandOptions {
  stdin?: Uint8Array;
  cwd?: string;
}

export type CommandRunner = (
  command: string[],
  options?: CommandOptions,
) => Promise<CommandResult>;

/** A command could not be started or observed reliably on the host system. */
export class CommandExecutionError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "CommandExecutionError";
  }
}

/** Executes a command while draining both output pipes concurrently. */
export async function runCommand(
  command: string[],
  options: CommandOptions = {},
): Promise<CommandResult> {
  const executable = command[0];
  if (!executable) {
    throw new CommandExecutionError("cannot run an empty command");
  }

  try {
    const process = Bun.spawn(command, {
      ...(options.cwd ? { cwd: options.cwd } : {}),
      stdin: options.stdin ?? "ignore",
      stdout: "pipe",
      stderr: "pipe",
    });

    // Reading stdout before stderr can deadlock if diagnostics fill stderr's
    // pipe, so start both reads before awaiting the exit status.
    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(process.stdout).arrayBuffer(),
      new Response(process.stderr).text(),
      process.exited,
    ]);

    return { stdout: new Uint8Array(stdout), stderr, exitCode };
  } catch (error) {
    throw commandExecutionError(executable, error);
  }
}

/** Builds a useful failure from a non-zero command result. */
export function commandError(command: string[], result: CommandResult): Error {
  return new Error(
    result.stderr.trim() || `${command.join(" ")} failed with code ${result.exitCode}`,
  );
}

function commandExecutionError(executable: string, error: unknown): Error {
  const code = getErrorCode(error);
  if (code === "ENOENT") {
    return new CommandExecutionError(
      `cannot run '${executable}': command not found; install it and ensure it is available on PATH`,
      { cause: error },
    );
  }
  if (code === "EACCES") {
    return new CommandExecutionError(
      `cannot run '${executable}': file is not executable`,
      { cause: error },
    );
  }

  const detail = error instanceof Error && error.message.trim()
    ? `: ${error.message.trim()}`
    : "";
  return new CommandExecutionError(
    `could not run '${executable}'${detail}`,
    { cause: error },
  );
}

function getErrorCode(error: unknown): string | undefined {
  if (!error || typeof error !== "object" || !("code" in error)) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === "string" ? code : undefined;
}
