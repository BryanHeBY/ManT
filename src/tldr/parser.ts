import type { TldrCommandPart, TldrExample, TldrPage } from "./types";

export interface TldrPageLocation {
  platform: string;
  language: string;
  sourcePath: string;
}

function flattenMarkdown(text: string): string {
  return text
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/(\*\*|__|\*|_)(.*?)\1/g, "$2")
    .replace(/<([^>]+)>/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

function extractTrailingCode(text: string): { description: string; command: string } | null {
  const match = /^(.*?)(?:\s*:\s*|\s+)`([^`]+)`\s*$/.exec(text);
  if (!match) return null;

  return {
    description: flattenMarkdown(match[1]!.replace(/:\s*$/, "")),
    command: match[2]!,
  };
}

function pushPart(parts: TldrCommandPart[], type: TldrCommandPart["type"], content: string): void {
  if (!content) return;
  const previous = parts.at(-1);
  if (previous?.type === type) {
    previous.content += content;
  } else {
    parts.push({ type, content });
  }
}

function resolveOptionPlaceholder(content: string): string | null {
  const match = /^\[([^|]+)\|([^\]]+)\]$/.exec(content);
  return match ? match[2]! : null;
}

/**
 * Parses the tldr-pages placeholder extension.  Mant intentionally uses the
 * long option variant because it is clearer in a quick-reference page.
 */
export function parseTldrCommand(command: string): TldrCommandPart[] {
  const parts: TldrCommandPart[] = [];
  let cursor = 0;

  while (cursor < command.length) {
    if (command.startsWith("\\{\\{", cursor)) {
      const close = command.indexOf("\\}\\}", cursor + 4);
      if (close >= 0) {
        pushPart(parts, "text", `{{${command.slice(cursor + 4, close)}}}`);
        cursor = close + 4;
        continue;
      }
    }

    if (command.startsWith("{{", cursor)) {
      const close = command.indexOf("}}", cursor + 2);
      if (close >= 0) {
        const placeholder = command.slice(cursor + 2, close);
        pushPart(parts, "placeholder", resolveOptionPlaceholder(placeholder) ?? placeholder);
        cursor = close + 2;
        continue;
      }
    }

    pushPart(parts, "text", command[cursor]!);
    cursor += 1;
  }

  return parts;
}

function makeExample(description: string, command: string): TldrExample {
  return {
    description,
    command,
    commandParts: parseTldrCommand(command),
  };
}

/**
 * tldr-pages has a deliberately constrained Markdown authoring format.  This
 * parser accepts its heading, quote, and example conventions while retaining
 * the command text separately for placeholder-aware rendering.
 */
export function parseTldrPage(markdown: string, location: TldrPageLocation): TldrPage {
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  let title = "";
  const description: string[] = [];
  let moreInformation: string | undefined;
  const examples: TldrExample[] = [];
  let pendingDescription: string | null = null;

  const flushPending = () => {
    if (pendingDescription) {
      examples.push(makeExample(pendingDescription, ""));
      pendingDescription = null;
    }
  };

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    if (!title && trimmed.startsWith("# ")) {
      title = flattenMarkdown(trimmed.slice(2));
      continue;
    }

    if (trimmed.startsWith(">")) {
      const quote = flattenMarkdown(trimmed.slice(1));
      const moreInformationMatch = /^More information:\s*(.+)$/i.exec(quote);
      if (moreInformationMatch) {
        moreInformation = moreInformationMatch[1];
      } else if (quote) {
        description.push(quote);
      }
      continue;
    }

    if (trimmed.startsWith("- ")) {
      flushPending();
      const example = extractTrailingCode(trimmed.slice(2));
      if (example) {
        examples.push(makeExample(example.description, example.command));
      } else {
        pendingDescription = flattenMarkdown(trimmed.slice(2).replace(/:\s*$/, ""));
      }
      continue;
    }

    const commandMatch = /^`([^`]+)`$/.exec(trimmed);
    if (pendingDescription && commandMatch) {
      examples.push(makeExample(pendingDescription, commandMatch[1]!));
      pendingDescription = null;
    }
  }

  flushPending();
  if (!title) throw new Error("tldr page is missing its command heading");

  return {
    title,
    description,
    ...(moreInformation ? { moreInformation } : {}),
    examples,
    ...location,
  };
}

export function tldrPageText(page: TldrPage): string {
  return [
    page.title,
    ...page.description,
    page.moreInformation ?? "",
    ...page.examples.flatMap((example) => [example.description, example.command]),
  ].join("\n");
}
