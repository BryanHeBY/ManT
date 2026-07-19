/**
 * @file Maps a native build host to ManT's public release target names.
 *
 * Rust/libmandoc and the Bun executable must describe the same operating
 * system and architecture. Keeping that mapping in one place prevents a
 * release archive from accidentally combining binaries for different hosts.
 */

export interface ReleasePlatform {
  archiveTarget: "linux-x64" | "linux-arm64" | "macos-x64" | "macos-arm64";
  bunCompileTarget:
    | "bun-linux-x64-baseline"
    | "bun-linux-arm64"
    | "bun-darwin-x64"
    | "bun-darwin-arm64";
}

/** Return the release identity for one natively built distribution. */
export function resolveReleasePlatform(
  platform: string = process.platform,
  architecture: string = process.arch,
): ReleasePlatform {
  if (platform === "linux" && architecture === "x64") {
    return {
      archiveTarget: "linux-x64",
      // Release binaries should also run on pre-AVX2 x64 machines.
      bunCompileTarget: "bun-linux-x64-baseline",
    };
  }
  if (platform === "linux" && architecture === "arm64") {
    return {
      archiveTarget: "linux-arm64",
      bunCompileTarget: "bun-linux-arm64",
    };
  }
  if (platform === "darwin" && architecture === "x64") {
    return {
      archiveTarget: "macos-x64",
      bunCompileTarget: "bun-darwin-x64",
    };
  }
  if (platform === "darwin" && architecture === "arm64") {
    return {
      archiveTarget: "macos-arm64",
      bunCompileTarget: "bun-darwin-arm64",
    };
  }

  throw new Error(
    `ManT releases do not support ${platform}/${architecture}; `
    + "use Linux or macOS on x64 or arm64",
  );
}
