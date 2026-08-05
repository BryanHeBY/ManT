/**
 * @file Maps a native build host to ManT's public release target names.
 *
 * Keeping the native mapping in one place prevents a release archive from
 * being labelled for a different operating system or architecture.
 */

export interface ReleasePlatform {
  archiveTarget: "linux-x64" | "linux-arm64" | "macos-x64" | "macos-arm64";
}

/** Return the release identity for one natively built distribution. */
export function resolveReleasePlatform(
  platform: string = process.platform,
  architecture: string = process.arch,
): ReleasePlatform {
  if (platform === "linux" && architecture === "x64") {
    return { archiveTarget: "linux-x64" };
  }
  if (platform === "linux" && architecture === "arm64") {
    return { archiveTarget: "linux-arm64" };
  }
  if (platform === "darwin" && architecture === "x64") {
    return { archiveTarget: "macos-x64" };
  }
  if (platform === "darwin" && architecture === "arm64") {
    return { archiveTarget: "macos-arm64" };
  }

  throw new Error(
    `ManT releases do not support ${platform}/${architecture}; `
    + "use Linux or macOS on x64 or arm64",
  );
}
