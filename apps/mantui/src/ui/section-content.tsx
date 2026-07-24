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
import {
  embeddedTldrModel,
  QuickReferencePanel,
} from "./tldr-quick-reference";

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
  const spacingBefore = Math.max(0, Math.floor(node.spacingBeforeLines ?? 0));
  const tldrModel = quickReference ? embeddedTldrModel(node) : undefined;

  if (tldrModel) {
    return (
      <box flexDirection="column" gap={0}>
        {spacingBefore > 0 ? <box height={spacingBefore} /> : null}
        <QuickReferencePanel model={tldrModel} />
      </box>
    );
  }

  /*
   * Unusual quick-reference content stays visible through the generic block
   * renderer. Keep source spacing outside its coloured surface so blank rows
   * do not look like oversized padding.
   */
  if (quickReference) {
    return (
      <box flexDirection="column" gap={0}>
        {spacingBefore > 0 ? <box height={spacingBefore} /> : null}
        <box
          flexDirection="column"
          gap={0}
          backgroundColor="#1d1a2b"
          border={["left"]}
          borderColor="#cba6f7"
          paddingLeft={1}
          paddingRight={1}
        >
          <box>
            <text id={contentId(node.id)} fg="#cba6f7">
              <b>{`◆ ${node.title}`}</b>
            </text>
          </box>
          {renderBlockNodes(node.blocks, 0, node.id, "", onNavigateInternal)}
          <box flexDirection="column" gap={0}>
            {node.children.map((child) => (
              <SectionContent
                key={child.id}
                node={child}
                baseIndent={2}
                headingIndent={2}
                onNavigateInternal={onNavigateInternal}
              />
            ))}
          </box>
        </box>
      </box>
    );
  }

  return (
    <box
      flexDirection="column"
      gap={0}
      paddingTop={spacingBefore}
    >
      <box paddingLeft={headingIndent}>
        <text id={contentId(node.id)} fg="#94e2d5">
          <b>{node.title}</b>
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
