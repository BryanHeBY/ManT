/**
 * @file Calls the standalone Rust `mant` through its versioned stdio API.
 *
 * This is deliberately a process and validation adapter only. Rust owns all
 * source discovery, parsing, query composition, and serialization semantics.
 */

import {
  commandError,
  runCommand,
  type CommandRunner,
} from "./process";
import {
  decodeMantQuery,
  decodeNativeCliProtocol,
  type MantQueryRequest,
  type MantQueryBundle,
  type NativeCliProtocol,
} from "./schema";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

// Bound every native invocation so a hung mant cannot leave the TUI
// blocked at startup with no output. Generous enough for large pages (gcc,
// clang) on a slow host, short enough to surface a stuck process.
const NATIVE_CLI_TIMEOUT_MS = 30_000;

export interface NativeQueryRequest {
  topic: string;
  section?: string;
  forceLibmandoc?: boolean;
  forceGroff?: boolean;
}

export interface NativeCliClient {
  protocol(): Promise<NativeCliProtocol>;
  query(request: NativeQueryRequest): Promise<MantQueryBundle>;
}

export interface NativeCliDependencies {
  env?: Record<string, string | undefined>;
  which?: (command: string) => string | null;
  runCommand?: CommandRunner;
}

/**
 * Resolve an explicitly selected CLI before consulting PATH.
 *
 * No repository-relative fallback is used: development goes through
 * `bun run dev`, which builds Rust and supplies MANT_PATH explicitly.
 */
export function resolveMantCliPath(
  environment: Record<string, string | undefined> = process.env,
  which: (command: string) => string | null = Bun.which,
): string {
  if (environment.MANT_PATH !== undefined) {
    const override = environment.MANT_PATH.trim();
    if (!override) throw new Error("MANT_PATH is set but empty");
    return override;
  }

  const installed = which("mant");
  if (installed) return installed;
  throw new Error(
    "mant was not found; install it on PATH or set MANT_PATH. "
    + "From a source checkout, use 'bun run dev -- <topic>'",
  );
}

/** Creates one client whose protocol probe is shared by all of its queries. */
export function createNativeCliClient(
  dependencies: NativeCliDependencies = {},
): NativeCliClient {
  const environment = dependencies.env ?? process.env;
  const which = dependencies.which ?? Bun.which;
  const execute = dependencies.runCommand ?? runCommand;
  let verified: Promise<{ path: string; protocol: NativeCliProtocol }> | null = null;

  async function verify(): Promise<{ path: string; protocol: NativeCliProtocol }> {
    const path = resolveMantCliPath(environment, which);
    const result = await execute([path, "--protocol-version", "--compact"], {
      timeoutMs: NATIVE_CLI_TIMEOUT_MS,
    });
    if (result.exitCode !== 0) {
      throw nativeCliFailure([path, "--protocol-version", "--compact"], result);
    }
    const protocol = decodeNativeCliProtocol(decoder.decode(result.stdout));
    return { path, protocol };
  }

  async function getVerified() {
    // A rejected promise is discarded so correcting PATH or replacing a bad
    // development binary can recover within a long-lived host process.
    verified ??= verify().catch((error) => {
      verified = null;
      throw error;
    });
    return verified;
  }

  return {
    async protocol() {
      return (await getVerified()).protocol;
    },

    async query(request) {
      const { path } = await getVerified();
      const command = [
        path,
        "--request-json",
        ...(request.forceLibmandoc ? ["--force-libmandoc"] : []),
        ...(request.forceGroff ? ["--force-groff"] : []),
        "--format",
        "json",
        "--compact",
      ];
      const wireRequest: MantQueryRequest = {
        schema: "mant.request/v2",
        topic: request.topic,
        ...(request.section === undefined ? {} : { section: request.section }),
        view: { kind: "full" },
      };
      const result = await execute(command, {
        stdin: encoder.encode(JSON.stringify(wireRequest)),
        timeoutMs: NATIVE_CLI_TIMEOUT_MS,
      });
      if (result.exitCode !== 0) throw nativeCliFailure(command, result);
      return decodeMantQuery(decoder.decode(result.stdout));
    },
  };
}

function nativeCliFailure(
  command: string[],
  result: Awaited<ReturnType<CommandRunner>>,
): Error {
  const lines = result.stderr.trim().split("\n");
  const first = lines[0]?.replace(/^mant:\s*/, "");
  if (first) return new Error(first);
  return commandError(command, result);
}

/** Default client used after the native query path becomes authoritative. */
export const nativeCli = createNativeCliClient();
