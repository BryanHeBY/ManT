/**
 * @file Renders the collapsible section sidebar for a manual page.
 *
 * It is intentionally presentational: selection, expansion, and scrolling
 * actions are supplied by the application controller.
 */

import type { ScrollBoxRenderable } from "@opentui/core";
import type { MantQueryBundle } from "../native";
import { navId, TLDR_NAV_ID } from "./ids";
import {
  terminalColumnWidth,
  treeContinuationPrefix,
  treePrefix,
  type FlatNode,
  wrapNavigationTitle,
} from "./navigation-tree";

export interface ManualSidebarProps {
  result: MantQueryBundle;
  visibleNodes: FlatNode[];
  selectedId: string;
  expanded: ReadonlySet<string>;
  width: number;
  scrollRef: { current: ScrollBoxRenderable | null };
  onActivateNode: (id: string, hasChildren: boolean) => void;
  onActivateTldr: () => void;
}

/** Displays document hierarchy and preserves a continuous selected-row background. */
export function ManualSidebar({
  result,
  visibleNodes,
  selectedId,
  expanded,
  width,
  scrollRef,
  onActivateNode,
  onActivateTldr,
}: ManualSidebarProps) {
  return (
    <box width={width} flexDirection="column" flexShrink={0} backgroundColor="#11111b">
      <box
        flexDirection="column"
        paddingLeft={1}
        paddingRight={1}
        paddingTop={1}
        paddingBottom={1}
        border={["bottom"]}
        borderColor="#313244"
      >
        <text height={1} fg="#cdd6f4" truncate wrapMode="none" selectable={false}>
          {`MANUAL · ${result.topic}`}
        </text>
        <text height={1} fg="#7f849c" selectable={false}>
          {`${result.manual?.sections.length ?? 0} top-level · ${visibleNodes.length} manual${result.tldr ? " · TLDR" : ""}`}
        </text>
      </box>
      <box height={1} paddingLeft={1} paddingRight={1}>
        <text fg="#6c7086" selectable={false}>SECTIONS</text>
      </box>
      <scrollbox
        ref={scrollRef}
        flexGrow={1}
        scrollY
        focusable={false}
        horizontalScrollbarOptions={{ visible: false }}
        verticalScrollbarOptions={{
          trackOptions: {
            foregroundColor: "#45475a",
            backgroundColor: "#11111b",
          },
        }}
      >
        {/* Cached tldr content is a synthetic document root, deliberately
            styled apart from the authoritative local manual. */}
        {result.tldr && (
          <box
            id={navId(TLDR_NAV_ID)}
            width="100%"
            height={1}
            flexShrink={0}
            paddingLeft={1}
            backgroundColor={selectedId === TLDR_NAV_ID ? "#49405f" : "#1d1a2b"}
            onMouseDown={onActivateTldr}
          >
            <text truncate wrapMode="none" selectable={false}>
              <span fg={selectedId === TLDR_NAV_ID ? "#f5e0dc" : "#cba6f7"}>
                {selectedId === TLDR_NAV_ID ? "› ◆ " : "  ◆ "}
              </span>
              <span fg="#cba6f7"><b>TLDR QUICK REFERENCE</b></span>
            </text>
          </box>
        )}
        {/* Each selected row owns one background box per wrapped line. This
            avoids fragment-level highlights that leave tree connectors bare. */}
        {visibleNodes.map((flatNode) => {
          const { node, hasChildren } = flatNode;
          const isSelected = node.id === selectedId;
          const titleColor = isSelected
            ? "#f5e0dc"
            : flatNode.depth === 0
              ? "#cdd6f4"
              : flatNode.depth === 1
                ? "#89b4fa"
                : "#a6adc8";
          const disclosure = hasChildren
            ? expanded.has(node.id) ? "▾ " : "▸ "
            : "· ";
          const labelPrefix = `${isSelected ? "› " : "  "}${treePrefix(flatNode)}${disclosure}`;
          const selectedTitleLines = isSelected
            ? wrapNavigationTitle(node.title, width - 1 - terminalColumnWidth(labelPrefix))
            : [];

          return (
            <box
              key={navId(node.id)}
              id={navId(node.id)}
              ref={(element) => {
                if (element) element.onMouseDown = () => onActivateNode(node.id, hasChildren);
              }}
              width="100%"
              height={isSelected ? "auto" : 1}
              flexDirection={isSelected ? "column" : "row"}
              flexShrink={0}
              paddingLeft={1}
              backgroundColor={isSelected ? "#313244" : "#11111b"}
            >
              {isSelected ? selectedTitleLines.map((line, index) => {
                const prefix = index === 0
                  ? labelPrefix
                  : `  ${treeContinuationPrefix(flatNode)}`;
                return (
                  <box
                    key={`${node.id}-line-${index}`}
                    width="100%"
                    height={1}
                    flexDirection="row"
                    backgroundColor="#313244"
                  >
                    <text
                      width={terminalColumnWidth(prefix)}
                      fg={index === 0 ? "#fab387" : "#f5c2e7"}
                      wrapMode="none"
                      selectable={false}
                    >
                      {prefix}
                    </text>
                    <text fg={titleColor} wrapMode="none" selectable={false}>{line}</text>
                  </box>
                );
              }) : (
                <text truncate wrapMode="none" selectable={false}>
                  <span fg="#6c7086">{labelPrefix}</span>
                  <span fg={titleColor}>{node.title}</span>
                </text>
              )}
            </box>
          );
        })}
      </scrollbox>
    </box>
  );
}
