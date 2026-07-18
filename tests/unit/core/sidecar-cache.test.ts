/**
 * @file Tests platform-specific cache paths for embedded native sidecars.
 */

import { describe, expect, test } from "bun:test";
import { getSidecarCacheDir } from "../../../src/core/sidecar-cache";

describe("getSidecarCacheDir", () => {
  test("honours an explicit executable-cache override", () => {
    expect(getSidecarCacheDir({ MANT_SIDECAR_DIR: "/opt/mant-cache" }, "linux"))
      .toBe("/opt/mant-cache");
  });

  test("uses the XDG cache location on Linux", () => {
    expect(getSidecarCacheDir({ XDG_CACHE_HOME: "/cache", HOME: "/home/me" }, "linux"))
      .toBe("/cache/mant/sidecars");
    expect(getSidecarCacheDir({ HOME: "/home/me" }, "linux"))
      .toBe("/home/me/.cache/mant/sidecars");
  });

  test("uses platform cache conventions on macOS and Windows", () => {
    expect(getSidecarCacheDir({ HOME: "/Users/me" }, "darwin"))
      .toBe("/Users/me/Library/Caches/mant/sidecars");
    expect(getSidecarCacheDir({ LOCALAPPDATA: "C:\\Users\\me\\AppData\\Local" }, "win32"))
      .toBe("C:\\Users\\me\\AppData\\Local\\mant\\sidecars");
  });
});
