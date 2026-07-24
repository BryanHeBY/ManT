/**
 * @file Recursively renders the structural section hierarchy of one manual.
 *
 * Block and inline presentation live in dedicated siblings so section nesting
 * remains easy to inspect independently of individual block formats.
 */

import { memo } from "react";
import type { MantSection } from "../native";
import { renderBlockNodes } from "./block-content";
import { contentId } from "./ids";

export interface SectionContentProps {
  node: MantSection;
  baseIndent?: number;
  headingIndent?: number;
  onNavigateInternal?: ((target: string) => void) | undefined;
}

/** Recursively renders one section and its children in document order. */
function SectionContentView({
  node,
  baseIndent = 3,
  headingIndent = 0,
  onNavigateInternal,
}: SectionContentProps) {
  const quickReference = node.role === "quick-reference";
  return (
    <box
      flexDirection="column"
      gap={0}
      paddingTop={Math.max(0, Math.floor(node.spacingBeforeLines ?? 0))}
      {...(quickReference
        ? {
            backgroundColor: "#1d1a2b",
            border: ["left"] as const,
            borderColor: "#cba6f7",
            paddingRight: 1,
          }
        : {})}
    >
      <box paddingLeft={headingIndent}>
        <text id={contentId(node.id)} fg={quickReference ? "#cba6f7" : "#94e2d5"}>
          <b>{quickReference ? `◆ ${node.title}` : node.title}</b>
        </text>
      </box>
      {renderBlockNodes(node.blocks, baseIndent, node.id, "", onNavigateInternal)}
      <box flexDirection="column" gap={0}>
        {node.children.map((child) => (
          <SectionContent
            key={child.id}
            node={child}
            baseIndent={baseIndent + 4}
            headingIndent={headingIndent + 4}
            onNavigateInternal={onNavigateInternal}
          />
        ))}
      </box>
    </box>
  );
}

function sectionContentPropsEqual(previous: SectionContentProps, next: SectionContentProps): boolean {
  return previous.node === next.node
    && previous.baseIndent === next.baseIndent
    && previous.headingIndent === next.headingIndent;
}

/** App state changes do not rebuild immutable manual sections. */
export const SectionContent = memo(SectionContentView, sectionContentPropsEqual);
