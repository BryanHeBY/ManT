/**
 * @file Renders the collapsible section sidebar for a manual page.
 *
 * It is intentionally presentational: selection, expansion, and scrolling
 * actions are supplied by the application controller.
 */

import { useMemo } from "react";
import type { ScrollBoxRenderable } from "@opentui/core";
import type { MantQueryBundle } from "../native";
import { DOCUMENT_ROOT_ID, navId, TLDR_NAV_ID } from "./ids";
import {
  buildNavigationRows,
  type FlatNode,
  type NavigationRow,
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
  hasRoot: boolean;
  onActivateRoot: () => void;
}

function navigationTitleColor(row: NavigationRow, selected: boolean): string {
  if (selected) return "#f5e0dc";
  if (row.node.kind === "entry-group") return "#f9e2af";
  if (row.node.kind === "option") return "#a6e3a1";
  if (row.depth === 0) return "#cdd6f4";
  if (row.depth === 1) return "#89b4fa";
  return "#a6adc8";
}

interface RowGroup {
  nodeId: string;
  node: NavigationRow["node"];
  rows: NavigationRow[];
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
  hasRoot,
  onActivateRoot,
}: ManualSidebarProps) {
  const visibleDocumentSections = visibleNodes.filter(
    ({ node }) => node.kind === "section",
  ).length;

  const rows = useMemo(
    () => buildNavigationRows(visibleNodes, expanded, width, selectedId),
    [visibleNodes, expanded, width, selectedId],
  );

  const rowGroups = useMemo(() => {
    const groups: RowGroup[] = [];
    for (const row of rows) {
      const last = groups[groups.length - 1];
      if (last && last.nodeId === row.nodeId) {
        last.rows.push(row);
      } else {
        groups.push({ nodeId: row.nodeId, node: row.node, rows: [row] });
      }
    }
    return groups;
  }, [rows]);

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
          {`${result.document?.source.format === "markdown" ? "MARKDOWN" : "MANUAL"} · ${result.label}`}
        </text>
        <text height={1} fg="#7f849c" selectable={false}>
          {`${result.document?.sections.length ?? 0} top-level · ${visibleDocumentSections} sections${result.tldr ? " · TLDR" : ""}`}
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
        {/* TLDR content is a synthetic document root, deliberately styled
            apart from the authoritative manual or Markdown document. */}
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
        {hasRoot && (
          <box
            id={navId(DOCUMENT_ROOT_ID)}
            width="100%"
            height={1}
            flexShrink={0}
            paddingLeft={1}
            backgroundColor={selectedId === DOCUMENT_ROOT_ID ? "#313244" : "#11111b"}
            onMouseDown={onActivateRoot}
          >
            <text truncate wrapMode="none" selectable={false}>
              <span fg={selectedId === DOCUMENT_ROOT_ID ? "#fab387" : "#6c7086"}>
                {selectedId === DOCUMENT_ROOT_ID ? "› ◇ " : "  ◇ "}
              </span>
              <span fg={selectedId === DOCUMENT_ROOT_ID ? "#f5e0dc" : "#bac2de"}>
                OVERVIEW
              </span>
            </text>
          </box>
        )}
        {/* Each node is rendered as a fixed-height group of one or more rows.
            The group owns the item background so wrapped titles stay continuous
            and the scrollbox never sees mixed-height children. */}
        {rowGroups.map((group) => {
          const isSelected = group.nodeId === selectedId;
          const titleColor = navigationTitleColor(group.rows[0]!, isSelected);

          return (
            <box
              key={navId(group.nodeId)}
              id={navId(group.nodeId)}
              width="100%"
              height={group.rows.length}
              flexDirection="column"
              flexShrink={0}
              paddingLeft={1}
              backgroundColor={isSelected ? "#313244" : "#11111b"}
              onMouseDown={() => onActivateNode(group.nodeId, group.node.children.length > 0)}
            >
              {group.rows.map((row) => {
                const prefix = row.lineIndex === 0
                  ? `${isSelected ? "› " : "  "}${row.prefix}`
                  : `  ${row.continuationPrefix}`;
                const prefixColor = isSelected
                  ? row.lineIndex === 0 ? "#fab387" : "#f5c2e7"
                  : "#6c7086";

                return (
                  <text
                    key={row.id}
                    height={1}
                    truncate={row.lineIndex === 0 && !isSelected}
                    wrapMode="none"
                    selectable={false}
                  >
                    <span fg={prefixColor}>{prefix}</span>
                    <span fg={titleColor}>{row.text}</span>
                  </text>
                );
              })}
            </box>
          );
        })}
      </scrollbox>
    </box>
  );
}
