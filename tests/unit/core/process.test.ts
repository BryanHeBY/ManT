/**
 * @file Verifies that the shared process boundary turns host failures into
 * actionable, runtime-agnostic diagnostics.
 */

import { describe, expect, test } from "bun:test";
import {
  CommandExecutionError,
  commandError,
  runCommand,
} from "../../../src/core/process";

describe("process boundary", () => {
  test("runs a renderer from its requested source directory", async () => {
    const result = await runCommand(["/bin/pwd"], { cwd: import.meta.dir });

    expect(result.exitCode).toBe(0);
    expect(new TextDecoder().decode(result.stdout).trim()).toBe(import.meta.dir);
  });

  test("rejects empty commands before reaching the runtime", async () => {
    await expect(runCommand([])).rejects.toThrow("cannot run an empty command");
  });

  test("describes a missing executable without exposing a Bun spawn error", async () => {
    const error = await runCommand(["__mant_command_that_does_not_exist__"])
      .then(() => undefined, (failure: unknown) => failure);

    expect(error).toBeInstanceOf(CommandExecutionError);
    expect((error as Error).message).toBe(
      "cannot run '__mant_command_that_does_not_exist__': command not found; install it and ensure it is available on PATH",
    );
  });

  test("uses renderer diagnostics before a generic exit-code message", () => {
    expect(commandError(["man", "missing"], {
      stdout: new Uint8Array(),
      stderr: "No manual entry for missing\n",
      exitCode: 16,
    }).message).toBe("No manual entry for missing");

    expect(commandError(["man", "missing"], {
      stdout: new Uint8Array(),
      stderr: "",
      exitCode: 16,
    }).message).toBe("man missing failed with code 16");
  });
});
