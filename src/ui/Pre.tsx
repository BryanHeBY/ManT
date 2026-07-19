/**
 * @file Renders preformatted roff blocks with their intended body indentation.
 */

import { memo, type ReactNode } from "react";
import type { MantInline } from "../native";

const CODE_TOKEN_RE =
  /(\b(?:void|int|char|float|double|long|short|signed|unsigned|return|if|else|for|while|do|switch|case|break|continue|struct|union|enum|typedef|static|const|volatile|extern|inline|restrict|sizeof|NULL|true|false|null)\b)|(--?[A-Za-z][\w-]*(?:=\S+)?)|(\b\d+(?:\.\d+)?\b)|("[^"]*"|'[^']*')|(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|(\s+)|(\w+)|(.)/g;

function flattenInline(nodes: MantInline[]): string {
  return nodes
    .map((node) => {
      switch (node.type) {
        case "text":
        case "code":
          return node.value;
        case "line-break":
          return "\n";
        case "strong":
        case "emphasis":
        case "external-link":
        case "email-link":
        case "manual-reference":
        case "section-reference":
          return flattenInline(node.children);
        case "anchor":
          return "";
      }
    })
    .join("");
}

function makeCodeSpans(text: string): ReactNode[] {
  const spans: ReactNode[] = [];
  let key = 0;
  for (const match of text.matchAll(CODE_TOKEN_RE)) {
    const token = match[0];
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
    const content = bold ? <b>{token}</b> : italic ? <i>{token}</i> : token;
    spans.push(<span key={key++} fg={color}>{content}</span>);
  }
  return spans;
}

interface PreProps {
  children: MantInline[];
  block?: boolean;
  indent?: number;
}

/**
 * Renders preformatted text (code blocks).
 *
 * In block mode an unpainted spacer carries the roff indentation.  The code
 * background therefore starts at the body indent instead of looking like a
 * full-width block glued to the content pane's left edge.
 */
function PreView({ children, block = false, indent = 0 }: PreProps): ReactNode {
  const text = flattenInline(children);
  const spans = makeCodeSpans(text);
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

/** Avoid re-tokenizing every unchanged code block when a search result moves. */
export const Pre = memo(PreView);
