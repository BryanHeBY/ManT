/**
 * @file Consumes Rust-generated JSON Schemas from the real mant binary.
 *
 * The validator is test-only: production keeps its small handwritten guard,
 * while this check catches drift between Serde, Schemars, and TypeScript.
 */

import { describe, expect, test } from "bun:test";
import Ajv2020, { type AnySchema, type ValidateFunction } from "ajv/dist/2020.js";

const mantPath = new URL("../../../../engine/bin/mant", import.meta.url).pathname;
const queryFixturePath = new URL(
  "../../../../tests/contracts/minimal-query-v3.json",
  import.meta.url,
).pathname;
// A missing executable makes spawnSync throw ENOENT rather than return a
// non-zero code, so a bare probe would crash module load before describe.skip
// can guard it. Treat any spawn failure as "unavailable" so the suite skips.
function canRun(command: string[]): boolean {
  try {
    return Bun.spawnSync(command, { stdout: "ignore", stderr: "ignore" })
      .exitCode === 0;
  } catch {
    return false;
  }
}

const mantAvailable = canRun([mantPath, "--protocol-version", "--compact"]);

const describeMant = mantAvailable ? describe : describe.skip;
const contracts = ["request", "query", "outline", "excerpt", "search"] as const;
type Contract = typeof contracts[number];
type SchemaCatalog = Record<Contract, AnySchema>;

function compile(schema: AnySchema): ValidateFunction {
  // JSON Schema permits implementation-defined formats. Schemars annotates
  // Rust unsigned integers with these names while numeric bounds remain in
  // the schema; registering them keeps Ajv strict for every other format.
  return new Ajv2020({
    allErrors: true,
    strict: true,
    formats: {
      uint8: true,
      uint16: true,
      uint32: true,
      uint64: true,
    },
  }).compile(schema);
}

function expectValid(validate: ValidateFunction, value: unknown): void {
  if (!validate(value)) {
    throw new Error(`generated schema rejected a valid value: ${JSON.stringify(validate.errors)}`);
  }
}

describeMant("generated native JSON Schemas", () => {
  test("compile and validate the TypeScript request and shared query fixture", async () => {
    const output = Bun.spawnSync(
      [mantPath, "--schema", "all", "--compact"],
      { stdout: "pipe", stderr: "pipe" },
    );
    expect(output.exitCode).toBe(0);
    expect(new TextDecoder().decode(output.stderr)).toBe("");

    const catalog = JSON.parse(
      new TextDecoder().decode(output.stdout),
    ) as SchemaCatalog;
    for (const contract of contracts) compile(catalog[contract]);

    const validateRequest = compile(catalog.request);
    expectValid(validateRequest, {
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "printf", section: "3" },
      view: { kind: "full" },
    });
    expectValid(validateRequest, {
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "tar" },
      view: { kind: "outline", detail: "options" },
    });
    expectValid(validateRequest, {
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "tar" },
      view: { kind: "excerpt", nodes: ["acls"] },
    });
    expectValid(validateRequest, {
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "tar" },
      view: {
        kind: "search",
        pattern: "--acls",
        syntax: "literal",
        case: "insensitive",
        scope: "visible",
        contextLines: 1,
        limit: 20,
        offset: 0,
      },
    });
    expect(validateRequest({ topic: "printf" })).toBe(false);
    expect(validateRequest({
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "printf" },
      view: { kind: "full" },
      renderer: "html",
    })).toBe(false);
    expect(validateRequest({
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "tar" },
      view: { kind: "excerpt", nodes: [] },
    })).toBe(false);
    expect(validateRequest({
      schema: "mant.request/v3",
      input: { kind: "manual", topic: "tar" },
      view: { kind: "full", future: true },
    })).toBe(false);

    const query = JSON.parse(await Bun.file(queryFixturePath).text()) as unknown;
    expectValid(compile(catalog.query), query);

    expectValid(compile(catalog.search), {
      schema: "mant.search/v2",
      label: "tar",
      query: {
        pattern: "--acls",
        syntax: "literal",
        case: "insensitive",
        scope: "visible",
        word: false,
        contextLines: 0,
        limit: 100,
        offset: 0,
      },
      render: {
        schema: "mant.markdown/v1",
        format: "markdown",
        scope: "full",
        lineBase: 1,
        columnBase: 1,
        lineCount: 900,
      },
      total: 0,
      returned: 0,
      offset: 0,
      truncated: false,
      matches: [],
    });
  });
});
