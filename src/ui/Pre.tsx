/**
 * @file Renders preformatted roff blocks with their intended body indentation.
 */

import type { ReactNode } from "react";
import type { InlineNode } from "../core";
import {
  getSearchHighlightRanges,
  SEARCH_HIGHLIGHT_BACKGROUND,
  splitTextByHighlightRanges,
} from "./search-highlight";

const CODE_TOKEN_RE =
  /(\b(?:void|int|char|float|double|long|short|signed|unsigned|return|if|else|for|while|do|switch|case|break|continue|struct|union|enum|typedef|static|const|volatile|extern|inline|restrict|sizeof|NULL|true|false|null)\b)|(--?[A-Za-z][\w-]*(?:=\S+)?)|(\b\d+(?:\.\d+)?\b)|("[^"]*"|'[^']*')|(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|(\s+)|(\w+)|(.)/g;

function flattenInline(nodes: InlineNode[]): string {
  return nodes
    .map((node) => {
      switch (node.type) {
        case "text":
          return node.content;
        case "break":
          return "\n";
        default:
          return flattenInline(node.children);
      }
    })
    .join("");
}

function makeCodeSpans(text: string, searchQuery: string): ReactNode[] {
  const spans: ReactNode[] = [];
  const highlightRanges = getSearchHighlightRanges(text, searchQuery);
  let key = 0;
  for (const match of text.matchAll(CODE_TOKEN_RE)) {
    const token = match[0];
    const tokenOffset = match.index ?? 0;
    let color = "#cdd6f4";
    let bold = false;
    let italic = false;
    if (match[1]) {
      color = "#cba6f7";
      bold = true;
    } else if (match[2]) {
      color = "#94e2d5";
    } else if (match[3]) {
      color = "#f9e2af";
    } else if (match[4]) {
      color = "#89b4fa";
    } else if (match[5]) {
      color = "#6c7086";
      italic = true;
    } else if (match[6]) {
      color = "#6c7086";
    } else if (match[7]) {
      color = "#cdd6f4";
    } else {
      color = "#cdd6f4";
    }
    for (const fragment of splitTextByHighlightRanges(token, highlightRanges, tokenOffset)) {
      const content = bold ? <b>{fragment.text}</b> : italic ? <i>{fragment.text}</i> : fragment.text;
      spans.push(
        <span
          key={key++}
          fg={color}
          {...(fragment.highlighted ? { bg: SEARCH_HIGHLIGHT_BACKGROUND } : {})}
        >
          {content}
        </span>
      );
    }
  }
  return spans;
}

interface PreProps {
  children: InlineNode[];
  block?: boolean;
  indent?: number;
  searchQuery?: string;
}

/**
 * Renders preformatted text (code blocks).
 *
 * In block mode an unpainted spacer carries the roff indentation.  The code
 * background therefore starts at the body indent instead of looking like a
 * full-width block glued to the content pane's left edge.
 */
export function Pre({ children, block = false, indent = 0, searchQuery = "" }: PreProps): ReactNode {
  const text = flattenInline(children);
  const spans = makeCodeSpans(text, searchQuery);
  if (block) {
    return (
      <box shouldFill={true} flexDirection="row">
        {indent > 0 && <box width={indent} flexShrink={0} />}
        <box flexGrow={1} backgroundColor="#181825">
          <text wrapMode="char" fg="#cdd6f4">
            {spans}
          </text>
        </box>
      </box>
    );
  }
  return spans;
}
