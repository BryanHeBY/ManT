/**
 * @file Verifies actionable diagnostics at the mant-cli subprocess boundary.
 */

import { describe, expect, test } from "bun:test";
import {
  CommandExecutionError,
  commandError,
  runCommand,
} from "../../../src/native/process";

describe("native process boundary", () => {
  test("runs a command from its requested directory", async () => {
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

  test("uses native diagnostics before a generic exit-code message", () => {
    expect(commandError(["mant-cli", "missing"], {
      stdout: new Uint8Array(),
      stderr: "No manual entry for missing\n",
      exitCode: 1,
    }).message).toBe("No manual entry for missing");

    expect(commandError(["mant-cli", "missing"], {
      stdout: new Uint8Array(),
      stderr: "",
      exitCode: 1,
    }).message).toBe("mant-cli missing failed with code 1");
  });
});
