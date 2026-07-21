/**
 * @file Renders normalized inline document nodes with terminal-only styling.
 *
 * This module deliberately preserves source text. Navigation behavior belongs
 * to block renderers, where clickable references receive stable render IDs.
 */

import type { ReactNode } from "react";
import type { MantInline } from "../native";

/** Renders inline semantics without changing the visible source text. */
export function renderInlineContent(nodes: MantInline[], keyPrefix: string): ReactNode[] {
  let keyCounter = 0;
  const renderNodes = (children: MantInline[]): ReactNode[] => children.map((node) => {
    const key = `${keyPrefix}-${keyCounter++}`;
    switch (node.type) {
      case "text":
        return node.value;
      case "strong":
        return <span key={key} fg="#cdd6f4"><b>{renderNodes(node.children)}</b></span>;
      case "emphasis":
        return <span key={key} fg="#7f849c"><i>{renderNodes(node.children)}</i></span>;
      case "code":
        return <span key={key} fg="#94e2d5">{node.value}</span>;
      case "external-link":
      case "email-link":
        return <span key={key} fg="#89b4fa"><u>{renderNodes(node.children)}</u></span>;
      case "manual-reference":
        return <span key={key} fg="#89dceb">{renderNodes(node.children)}</span>;
      case "section-reference":
        return <span key={key} fg="#89dceb"><u>{renderNodes(node.children)}</u></span>;
      case "anchor":
        return null;
      case "line-break":
        return "\n";
    }
  });
  return renderNodes(nodes);
}
