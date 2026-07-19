/**
 * @file Mirrors and validates the versioned JSON contract emitted by Rust.
 *
 * Rust's mant-ast crate is the source of truth. These declarations keep React
 * strongly typed, while the decoder rejects incompatible or malformed native
 * payloads before they enter the UI.
 */

export type SourceFormat = "man" | "mdoc" | "groff-html" | "mandoc-html";
export type DiagnosticLevel = "style" | "warning" | "error" | "unsupported";
export type ListKind = "bullet" | "ordered" | "plain";
export type TableAlignment = "left" | "center" | "right";

export interface SourceSpan {
  line: number;
  column: number;
  endLine?: number;
  endColumn?: number;
}

export interface LayoutHint {
  indentColumns?: number;
}

export type MantInline =
  | { type: "text"; value: string }
  | { type: "strong"; children: MantInline[] }
  | { type: "emphasis"; children: MantInline[] }
  | { type: "code"; value: string }
  | { type: "link"; target: string; title?: string; children: MantInline[] }
  | { type: "manual-reference"; name: string; section?: string; children: MantInline[] }
  | { type: "line-break" };

export interface MantListItem {
  blocks: MantBlock[];
}

export interface MantDefinitionItem {
  terms: MantInline[][];
  description: MantBlock[];
}

export interface MantTableRow {
  cells: MantTableCell[];
}

export interface MantTableCell {
  blocks: MantBlock[];
  columnSpan?: number;
  rowSpan?: number;
  alignment?: TableAlignment;
}

interface BlockBase {
  layout?: LayoutHint;
  source?: SourceSpan;
}

export type MantBlock =
  | (BlockBase & { type: "paragraph"; children: MantInline[] })
  | (BlockBase & { type: "preformatted"; children: MantInline[]; language?: string })
  | (BlockBase & {
      type: "list";
      kind: ListKind;
      start?: number;
      compact?: boolean;
      items: MantListItem[];
    })
  | (BlockBase & {
      type: "definition-list";
      items: MantDefinitionItem[];
      compact?: boolean;
    })
  | (BlockBase & { type: "table"; rows: MantTableRow[] })
  | (BlockBase & { type: "equation"; value: string; display?: boolean })
  | { type: "vertical-space"; lines: number; source?: SourceSpan }
  | (BlockBase & { type: "unsupported"; name?: string; text: string });

export interface MantSection {
  id: string;
  title: string;
  blocks: MantBlock[];
  children: MantSection[];
  source?: SourceSpan;
}

export interface MantDocument {
  schema: "mant.document/v1";
  producer: {
    name: string;
    version: string;
    engine?: { name: string; version: string };
  };
  source: {
    format: SourceFormat;
    path?: string;
    renderer?: string;
  };
  meta: {
    title?: string;
    section?: string;
    date?: string;
    volume?: string;
    os?: string;
    arch?: string;
    names?: string[];
    aliasTarget?: string;
  };
  diagnostics?: Array<{
    level: DiagnosticLevel;
    code?: string;
    message: string;
    source?: SourceSpan;
  }>;
  sections: MantSection[];
}

export type TldrCommandPart =
  | { type: "text"; value: string }
  | { type: "placeholder"; value: string };

export interface TldrDocument {
  title: string;
  description: string[];
  moreInformation?: string;
  examples: Array<{
    description: string;
    command: string;
    commandParts: TldrCommandPart[];
  }>;
  platform: string;
  language: string;
  sourcePath: string;
}

export interface MantQueryBundle {
  schema: "mant.query/v1";
  topic: string;
  section?: string;
  manual?: MantDocument;
  tldr?: TldrDocument;
}

export interface NativeCliProtocol {
  protocol: "mant.cli/v1";
  nativeApiVersion: "1";
  querySchema: "mant.query/v1";
  documentSchema: "mant.document/v1";
  outlineSchema?: "mant.outline/v1";
  excerptSchema?: "mant.excerpt/v1";
}

type JsonObject = Record<string, unknown>;

/** Parses and recursively validates one native query response. */
export function decodeMantQuery(input: string): MantQueryBundle {
  let value: unknown;
  try {
    value = JSON.parse(input);
  } catch (error) {
    throw new Error(`native query returned invalid JSON: ${String(error)}`);
  }
  validateQuery(value, "$native");
  return value;
}

/** Parses and validates the native executable before sending it requests. */
export function decodeNativeCliProtocol(input: string): NativeCliProtocol {
  let value: unknown;
  try {
    value = JSON.parse(input);
  } catch (error) {
    throw new Error(`mant-cli protocol probe returned invalid JSON: ${String(error)}`);
  }
  const object = expectObject(value, "$protocol");
  expectLiteral(object.protocol, "mant.cli/v1", "$protocol.protocol");
  expectLiteral(object.nativeApiVersion, "1", "$protocol.nativeApiVersion");
  expectLiteral(object.querySchema, "mant.query/v1", "$protocol.querySchema");
  expectLiteral(object.documentSchema, "mant.document/v1", "$protocol.documentSchema");
  if (object.outlineSchema !== undefined) {
    expectLiteral(object.outlineSchema, "mant.outline/v1", "$protocol.outlineSchema");
  }
  if (object.excerptSchema !== undefined) {
    expectLiteral(object.excerptSchema, "mant.excerpt/v1", "$protocol.excerptSchema");
  }
  return object as unknown as NativeCliProtocol;
}

function validateQuery(value: unknown, path: string): asserts value is MantQueryBundle {
  const object = expectObject(value, path);
  expectLiteral(object.schema, "mant.query/v1", `${path}.schema`);
  expectString(object.topic, `${path}.topic`);
  expectOptionalString(object.section, `${path}.section`);
  if (object.manual !== undefined) validateDocument(object.manual, `${path}.manual`);
  if (object.tldr !== undefined) validateTldr(object.tldr, `${path}.tldr`);
}

function validateDocument(value: unknown, path: string): asserts value is MantDocument {
  const object = expectObject(value, path);
  expectLiteral(object.schema, "mant.document/v1", `${path}.schema`);

  const producer = expectObject(object.producer, `${path}.producer`);
  expectString(producer.name, `${path}.producer.name`);
  expectString(producer.version, `${path}.producer.version`);
  if (producer.engine !== undefined) {
    const engine = expectObject(producer.engine, `${path}.producer.engine`);
    expectString(engine.name, `${path}.producer.engine.name`);
    expectString(engine.version, `${path}.producer.engine.version`);
  }

  const source = expectObject(object.source, `${path}.source`);
  expectOneOf(source.format, ["man", "mdoc", "groff-html", "mandoc-html"], `${path}.source.format`);
  expectOptionalString(source.path, `${path}.source.path`);
  expectOptionalString(source.renderer, `${path}.source.renderer`);

  const meta = expectObject(object.meta, `${path}.meta`);
  for (const key of ["title", "section", "date", "volume", "os", "arch", "aliasTarget"] as const) {
    expectOptionalString(meta[key], `${path}.meta.${key}`);
  }
  if (meta.names !== undefined) {
    expectArray(meta.names, `${path}.meta.names`).forEach((item, index) => {
      expectString(item, `${path}.meta.names[${index}]`);
    });
  }

  if (object.diagnostics !== undefined) {
    expectArray(object.diagnostics, `${path}.diagnostics`).forEach((item, index) => {
      const diagnosticPath = `${path}.diagnostics[${index}]`;
      const diagnostic = expectObject(item, diagnosticPath);
      expectOneOf(
        diagnostic.level,
        ["style", "warning", "error", "unsupported"],
        `${diagnosticPath}.level`,
      );
      expectOptionalString(diagnostic.code, `${diagnosticPath}.code`);
      expectString(diagnostic.message, `${diagnosticPath}.message`);
      if (diagnostic.source !== undefined) validateSourceSpan(diagnostic.source, `${diagnosticPath}.source`);
    });
  }

  expectArray(object.sections, `${path}.sections`).forEach((section, index) => {
    validateSection(section, `${path}.sections[${index}]`);
  });
}

function validateSection(value: unknown, path: string): asserts value is MantSection {
  const object = expectObject(value, path);
  expectString(object.id, `${path}.id`);
  expectString(object.title, `${path}.title`);
  expectArray(object.blocks, `${path}.blocks`).forEach((block, index) => {
    validateBlock(block, `${path}.blocks[${index}]`);
  });
  expectArray(object.children, `${path}.children`).forEach((child, index) => {
    validateSection(child, `${path}.children[${index}]`);
  });
  if (object.source !== undefined) validateSourceSpan(object.source, `${path}.source`);
}

function validateBlock(value: unknown, path: string): asserts value is MantBlock {
  const object = expectObject(value, path);
  const type = expectString(object.type, `${path}.type`);
  if (object.layout !== undefined) validateLayout(object.layout, `${path}.layout`);
  if (object.source !== undefined) validateSourceSpan(object.source, `${path}.source`);

  switch (type) {
    case "paragraph":
      validateInlineArray(object.children, `${path}.children`);
      return;
    case "preformatted":
      validateInlineArray(object.children, `${path}.children`);
      expectOptionalString(object.language, `${path}.language`);
      return;
    case "list":
      expectOneOf(object.kind, ["bullet", "ordered", "plain"], `${path}.kind`);
      expectOptionalNumber(object.start, `${path}.start`);
      expectOptionalBoolean(object.compact, `${path}.compact`);
      expectArray(object.items, `${path}.items`).forEach((item, index) => {
        const itemObject = expectObject(item, `${path}.items[${index}]`);
        expectArray(itemObject.blocks, `${path}.items[${index}].blocks`).forEach((block, blockIndex) => {
          validateBlock(block, `${path}.items[${index}].blocks[${blockIndex}]`);
        });
      });
      return;
    case "definition-list":
      expectOptionalBoolean(object.compact, `${path}.compact`);
      expectArray(object.items, `${path}.items`).forEach((item, index) => {
        const itemPath = `${path}.items[${index}]`;
        const itemObject = expectObject(item, itemPath);
        expectArray(itemObject.terms, `${itemPath}.terms`).forEach((term, termIndex) => {
          validateInlineArray(term, `${itemPath}.terms[${termIndex}]`);
        });
        expectArray(itemObject.description, `${itemPath}.description`).forEach((block, blockIndex) => {
          validateBlock(block, `${itemPath}.description[${blockIndex}]`);
        });
      });
      return;
    case "table":
      expectArray(object.rows, `${path}.rows`).forEach((row, rowIndex) => {
        const rowObject = expectObject(row, `${path}.rows[${rowIndex}]`);
        expectArray(rowObject.cells, `${path}.rows[${rowIndex}].cells`).forEach((cell, cellIndex) => {
          validateTableCell(cell, `${path}.rows[${rowIndex}].cells[${cellIndex}]`);
        });
      });
      return;
    case "equation":
      expectString(object.value, `${path}.value`);
      expectOptionalBoolean(object.display, `${path}.display`);
      return;
    case "vertical-space":
      expectNumber(object.lines, `${path}.lines`);
      return;
    case "unsupported":
      expectOptionalString(object.name, `${path}.name`);
      expectString(object.text, `${path}.text`);
      return;
    default:
      throw contractError(`${path}.type`, `unknown block type '${type}'`);
  }
}

function validateTableCell(value: unknown, path: string): void {
  const object = expectObject(value, path);
  expectArray(object.blocks, `${path}.blocks`).forEach((block, index) => {
    validateBlock(block, `${path}.blocks[${index}]`);
  });
  expectOptionalNumber(object.columnSpan, `${path}.columnSpan`);
  expectOptionalNumber(object.rowSpan, `${path}.rowSpan`);
  if (object.alignment !== undefined) {
    expectOneOf(object.alignment, ["left", "center", "right"], `${path}.alignment`);
  }
}

function validateInlineArray(value: unknown, path: string): asserts value is MantInline[] {
  expectArray(value, path).forEach((inline, index) => validateInline(inline, `${path}[${index}]`));
}

function validateInline(value: unknown, path: string): asserts value is MantInline {
  const object = expectObject(value, path);
  const type = expectString(object.type, `${path}.type`);
  switch (type) {
    case "text":
    case "code":
      expectString(object.value, `${path}.value`);
      return;
    case "strong":
    case "emphasis":
      validateInlineArray(object.children, `${path}.children`);
      return;
    case "link":
      expectString(object.target, `${path}.target`);
      expectOptionalString(object.title, `${path}.title`);
      validateInlineArray(object.children, `${path}.children`);
      return;
    case "manual-reference":
      expectString(object.name, `${path}.name`);
      expectOptionalString(object.section, `${path}.section`);
      validateInlineArray(object.children, `${path}.children`);
      return;
    case "line-break":
      return;
    default:
      throw contractError(`${path}.type`, `unknown inline type '${type}'`);
  }
}

function validateTldr(value: unknown, path: string): asserts value is TldrDocument {
  const object = expectObject(value, path);
  expectString(object.title, `${path}.title`);
  expectArray(object.description, `${path}.description`).forEach((line, index) => {
    expectString(line, `${path}.description[${index}]`);
  });
  expectOptionalString(object.moreInformation, `${path}.moreInformation`);
  expectArray(object.examples, `${path}.examples`).forEach((example, index) => {
    const examplePath = `${path}.examples[${index}]`;
    const exampleObject = expectObject(example, examplePath);
    expectString(exampleObject.description, `${examplePath}.description`);
    expectString(exampleObject.command, `${examplePath}.command`);
    expectArray(exampleObject.commandParts, `${examplePath}.commandParts`).forEach((part, partIndex) => {
      const partPath = `${examplePath}.commandParts[${partIndex}]`;
      const partObject = expectObject(part, partPath);
      expectOneOf(partObject.type, ["text", "placeholder"], `${partPath}.type`);
      expectString(partObject.value, `${partPath}.value`);
    });
  });
  expectString(object.platform, `${path}.platform`);
  expectString(object.language, `${path}.language`);
  expectString(object.sourcePath, `${path}.sourcePath`);
}

function validateLayout(value: unknown, path: string): void {
  const object = expectObject(value, path);
  expectOptionalNumber(object.indentColumns, `${path}.indentColumns`);
}

function validateSourceSpan(value: unknown, path: string): void {
  const object = expectObject(value, path);
  expectNumber(object.line, `${path}.line`);
  expectNumber(object.column, `${path}.column`);
  expectOptionalNumber(object.endLine, `${path}.endLine`);
  expectOptionalNumber(object.endColumn, `${path}.endColumn`);
}

function expectObject(value: unknown, path: string): JsonObject {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw contractError(path, "expected an object");
  }
  return value as JsonObject;
}

function expectArray(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) throw contractError(path, "expected an array");
  return value;
}

function expectString(value: unknown, path: string): string {
  if (typeof value !== "string") throw contractError(path, "expected a string");
  return value;
}

function expectNumber(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw contractError(path, "expected a non-negative finite number");
  }
  return value;
}

function expectOptionalString(value: unknown, path: string): void {
  if (value !== undefined) expectString(value, path);
}

function expectOptionalNumber(value: unknown, path: string): void {
  if (value !== undefined) expectNumber(value, path);
}

function expectOptionalBoolean(value: unknown, path: string): void {
  if (value !== undefined && typeof value !== "boolean") {
    throw contractError(path, "expected a boolean");
  }
}

function expectLiteral<T extends string>(value: unknown, expected: T, path: string): asserts value is T {
  if (value !== expected) throw contractError(path, `expected '${expected}'`);
}

function expectOneOf<T extends string>(
  value: unknown,
  expected: readonly T[],
  path: string,
): asserts value is T {
  if (typeof value !== "string" || !expected.includes(value as T)) {
    throw contractError(path, `expected one of ${expected.join(", ")}`);
  }
}

function contractError(path: string, message: string): Error {
  return new Error(`incompatible native query at ${path}: ${message}`);
}
