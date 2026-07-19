/**
 * @file Verifies host defaults and CC overrides for native Rust builds.
 */

import { describe, expect, test } from "bun:test";
import {
  assertSupportedBuildPlatform,
  requestedCCompiler,
  resolveCCompiler,
} from "../../../scripts/c-compiler";

describe("C compiler selection", () => {
  test("uses gcc by default on Linux", () => {
    expect(requestedCCompiler("linux", {})).toEqual({
      command: "gcc",
      source: "platform-default",
    });
  });

  test("uses clang by default on macOS", () => {
    expect(requestedCCompiler("darwin", {})).toEqual({
      command: "/usr/bin/clang",
      source: "platform-default",
    });
  });

  test("resolves the macOS default to the clang executable", () => {
    expect(resolveCCompiler("darwin", {}, (command) => {
      expect(command).toBe("/usr/bin/clang");
      return "/usr/bin/clang";
    })).toEqual({
      command: "/usr/bin/clang",
      path: "/usr/bin/clang",
      source: "platform-default",
    });
  });

  test("CC overrides the compiler on supported platforms", () => {
    expect(requestedCCompiler("darwin", { CC: " clang-18 " })).toEqual({
      command: "clang-18",
      source: "environment",
    });
  });

  test("rejects native Windows even when CC is set", () => {
    expect(() => requestedCCompiler("win32", { CC: "gcc" })).toThrow(
      "use WSL on Windows",
    );
  });

  test("rejects other unsupported native hosts", () => {
    expect(() => assertSupportedBuildPlatform("freebsd")).toThrow(
      "support Linux and macOS only",
    );
  });

  test("resolves the selected command before starting the native build", () => {
    expect(resolveCCompiler("linux", { CC: "clang" }, (command) => {
      expect(command).toBe("clang");
      return "/toolchain/bin/clang";
    })).toEqual({
      command: "clang",
      path: "/toolchain/bin/clang",
      source: "environment",
    });
  });

  test("reports a missing compiler without leaking a Bun spawn error", () => {
    expect(() => resolveCCompiler("darwin", {}, () => null)).toThrow(
      "C compiler '/usr/bin/clang' (required by the darwin default) was not found",
    );
  });
});
