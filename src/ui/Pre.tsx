import type { ReactNode } from "react";
import type { InlineNode } from "../core";

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
    if (bold) {
      spans.push(
        <span key={key++} fg={color}>
          <b>{token}</b>
        </span>
      );
    } else if (italic) {
      spans.push(
        <span key={key++} fg={color}>
          <i>{token}</i>
        </span>
      );
    } else {
      spans.push(
        <span key={key++} fg={color}>
          {token}
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
}

/**
 * Renders preformatted text (code blocks).
 *
 * In block mode the component fills the full available width so the
 * background colour (`#181825`) spans the entire content area.  The indent
 * is applied via `paddingLeft` on the wrapping `<box>` (not the `<text>`):
 * in this OpenTUI version `paddingLeft` on a `<text>` element has no visual
 * effect, whereas on a `<box>` it correctly insets the content while the
 * box background still fills the full width.
 */
export function Pre({ children, block = false, indent = 0 }: PreProps): ReactNode {
  const text = flattenInline(children);
  const spans = makeCodeSpans(text);
  if (block) {
    return (
      <box shouldFill={true} bg="#181825" paddingLeft={indent}>
        <text wrapMode="char" fg="#cdd6f4">
          {spans}
        </text>
      </box>
    );
  }
  return spans;
}
