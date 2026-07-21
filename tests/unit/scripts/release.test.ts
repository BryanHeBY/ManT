/**
 * @file Verifies release naming, native target mapping, and version gates.
 */

import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";
import {
  runTar,
  versionFromReleaseTag,
  workspaceVersion,
} from "../../../scripts/package-release";
import { resolveReleasePlatform } from "../../../scripts/release-platform";

async function hasGnuTar(): Promise<boolean> {
  const child = Bun.spawn(["tar", "--version"], {
    stdin: "ignore",
    stdout: "pipe",
    stderr: "ignore",
  });
  const version = await new Response(child.stdout).text();
  await child.exited;
  return version.includes("GNU tar");
}

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

describe("release archive reproducibility", () => {
  test("produces byte-identical archives across builds with GNU tar", async () => {
    if (!(await hasGnuTar())) return;

    const workspace = await mkdtemp(join(tmpdir(), "mant-release-repro-"));
    try {
      const archiveRoot = "mant-0.0.0-linux-x64";
      const packageDirectory = join(workspace, archiveRoot);
      await Bun.write(join(packageDirectory, "mant"), "binary-one\n");
      await Bun.write(join(packageDirectory, "mant-cli"), "binary-two\n");
      await Bun.write(join(packageDirectory, "README.md"), "readme\n");

      const digests: string[] = [];
      for (const name of ["first.tar.gz", "second.tar.gz"]) {
        const archive = join(workspace, name);
        await runTar(workspace, archiveRoot, archive);
        digests.push(createHash("sha256").update(await readFile(archive)).digest("hex"));
      }

      expect(digests[0]).toBe(digests[1]);
    } finally {
      await rm(workspace, { recursive: true, force: true });
    }
  });
});
