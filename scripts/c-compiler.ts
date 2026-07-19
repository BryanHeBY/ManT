/**
 * @file Selects and validates the host C compiler used to build libmandoc.
 *
 * Native Mant packages target Linux and macOS. Windows users build and run
 * the Linux target under WSL, where the ordinary Linux compiler policy applies.
 */

export interface CCompilerSelection {
  /** Executable name requested by policy or through CC. */
  command: string;
  /** Absolute executable path resolved from PATH. */
  path: string;
  source: "environment" | "platform-default";
}

type SupportedBuildPlatform = "linux" | "darwin";
type Which = (command: string) => string | null;

/** Rejects hosts for which Mant cannot provide a usable native man toolchain. */
export function assertSupportedBuildPlatform(
  platform: string = process.platform,
): asserts platform is SupportedBuildPlatform {
  if (platform !== "linux" && platform !== "darwin") {
    const windowsHint = platform === "win32" ? "; use WSL on Windows" : "";
    throw new Error(
      `native Mant builds support Linux and macOS only${windowsHint}`,
    );
  }
}

/** Returns the requested compiler before consulting PATH. */
export function requestedCCompiler(
  platform: string = process.platform,
  environment: Record<string, string | undefined> = process.env,
): Omit<CCompilerSelection, "path"> {
  assertSupportedBuildPlatform(platform);

  const override = environment.CC?.trim();
  if (override) return { command: override, source: "environment" };

  if (platform === "linux") {
    return { command: "gcc", source: "platform-default" };
  }
  return { command: "clang", source: "platform-default" };
}

/** Applies host policy, then verifies that the selected executable exists. */
export function resolveCCompiler(
  platform: string = process.platform,
  environment: Record<string, string | undefined> = process.env,
  which: Which = Bun.which,
): CCompilerSelection {
  const requested = requestedCCompiler(platform, environment);
  const path = which(requested.command);
  if (!path) {
    const origin = requested.source === "environment"
      ? "selected by CC"
      : `required by the ${platform} default`;
    throw new Error(
      `C compiler '${requested.command}' (${origin}) was not found; `
      + "install it or set CC to a compatible compiler executable",
    );
  }

  return { ...requested, path };
}
