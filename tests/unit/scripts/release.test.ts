/**
 * @file Verifies release naming, native target mapping, and version gates.
 */

import { describe, expect, test } from "bun:test";
import {
  versionFromReleaseTag,
  workspaceVersion,
} from "../../../scripts/package-release";
import { resolveReleasePlatform } from "../../../scripts/release-platform";

describe("release platform selection", () => {
  test("maps every supported native runner to matching archive and Bun targets", () => {
    expect(resolveReleasePlatform("linux", "x64")).toEqual({
      archiveTarget: "linux-x64",
      bunCompileTarget: "bun-linux-x64-baseline",
    });
    expect(resolveReleasePlatform("linux", "arm64")).toEqual({
      archiveTarget: "linux-arm64",
      bunCompileTarget: "bun-linux-arm64",
    });
    expect(resolveReleasePlatform("darwin", "x64")).toEqual({
      archiveTarget: "macos-x64",
      bunCompileTarget: "bun-darwin-x64",
    });
    expect(resolveReleasePlatform("darwin", "arm64")).toEqual({
      archiveTarget: "macos-arm64",
      bunCompileTarget: "bun-darwin-arm64",
    });
  });

  test("rejects platforms that cannot provide a matching native CLI", () => {
    expect(() => resolveReleasePlatform("win32", "x64")).toThrow(
      "do not support win32/x64",
    );
    expect(() => resolveReleasePlatform("linux", "riscv64")).toThrow(
      "do not support linux/riscv64",
    );
  });
});

describe("release version validation", () => {
  test("accepts stable and prerelease semantic tags", () => {
    expect(versionFromReleaseTag("v1.2.3")).toBe("1.2.3");
    expect(versionFromReleaseTag("v1.2.3-rc.1")).toBe("1.2.3-rc.1");
  });

  test("rejects ambiguous release tags", () => {
    for (const tag of ["1.2.3", "v1.2", "v01.2.3", "release-v1.2.3"]) {
      expect(() => versionFromReleaseTag(tag)).toThrow("vMAJOR.MINOR.PATCH");
    }
  });

  test("reads only the workspace package version", () => {
    expect(workspaceVersion(`
[workspace.package]
version = "0.4.2"
edition = "2024"

[workspace.dependencies]
example = "9"
`)).toBe("0.4.2");
  });
});
