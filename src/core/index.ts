/**
 * @file Defines the public core API for rendering, parsing, and roff AST work.
 */

export * from "./types";
export { fetchManHtml } from "./fetcher";
export { parseManHtml } from "./parser";
export { parseMandoc } from "./mandoc-parser";
export { parseGroff } from "./groff-parser";
export { parseInline } from "./parser-utils";
export {
  createRoffAstFetcher,
  fetchRoffAst,
  type RoffAstDocument,
  type RoffAstFetcherDependencies,
  type RoffAstNode,
  type RoffAstResult,
} from "./roff-ast";
