/**
 * @file Normalizes the small tldr placeholder dialect for terminal display.
 *
 * Cached pages arrive pre-tokenized by Rust. Embedded Markdown commands retain
 * their source braces, so this module gives rendering and search one identical
 * visible command string.
 */

import type { TldrCommandPart } from "../native";

/** Split one embedded command into ordinary and highlighted placeholder runs. */
export function embeddedTldrCommandParts(command: string): TldrCommandPart[] {
  const parts: TldrCommandPart[] = [];
  let cursor = 0;
  for (const match of command.matchAll(/\{\{(.*?)\}\}/g)) {
    const index = match.index;
    if (index > cursor) {
      parts.push({ type: "text", value: command.slice(cursor, index) });
    }
    parts.push({ type: "placeholder", value: resolvePlaceholder(match[1] ?? "") });
    cursor = index + match[0].length;
  }
  if (cursor < command.length) {
    parts.push({ type: "text", value: command.slice(cursor) });
  }
  return parts.length > 0 ? parts : [{ type: "text", value: command }];
}

/** TextBuffer content drawn for an embedded tldr command. */
export function visibleEmbeddedTldrCommand(command: string): string {
  return embeddedTldrCommandParts(command).map((part) => part.value).join("");
}

function resolvePlaceholder(value: string): string {
  const choices = value.match(/^\[(.+?)\|(.+)]$/);
  return choices?.[2] ?? value;
}
