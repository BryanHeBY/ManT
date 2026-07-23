/**
 * @file Renders structured native blocks while preserving search and link IDs.
 *
 * Adjacent prose deliberately shares one TextBuffer. The page-search index
 * mirrors that grouping, so visual highlighting and scrolling stay exact.
 */

import type { ReactNode } from "react";
import type { MantBlock, MantInline } from "../native";
import { contentAnchorId, contentSearchId } from "./ids";
import { renderInlineContent } from "./inline-content";
import { Pre } from "./Pre";
import { searchPath, visibleInlineSegments } from "./search";

function layoutIndent(block: MantBlock): number {
  return block.type === "vertical-space"
    ? 0
    : block.layout?.indentColumns ?? 0;
}

function layoutSpacing(block: MantBlock): number {
  return block.type === "vertical-space"
    ? 0
    : Math.max(0, Math.floor(block.layout?.spacingBeforeLines ?? 0));
}

/**
 * Merges adjacent prose into larger TextBuffers while retaining structural
 * blocks. Search records use the same grouping and point at these stable IDs.
 */
export function renderBlockNodes(
  blocks: MantBlock[],
  baseIndent: number,
  sectionId: string,
  parentPath: string,
  onNavigateInternal?: (target: string) => void,
): ReactNode[] {
  const result: ReactNode[] = [];
  let inlineBuffer: ReactNode[] = [];
  let bufferIndent = 0;
  let bufferAnchorId: string | undefined;
  let keyCounter = 0;
  let inlineKey = 0;

  const flushInline = () => {
    // The final newline is a separator sentinel, not an empty terminal row.
    if (inlineBuffer[inlineBuffer.length - 1] === "\n") inlineBuffer.pop();
    if (inlineBuffer.length === 0) {
      bufferAnchorId = undefined;
      return;
    }
    result.push(
      <box
        key={`merged-${keyCounter++}`}
        {...(bufferAnchorId ? { id: bufferAnchorId } : {})}
        paddingLeft={bufferIndent}
        shouldFill={true}
      >
        <text fg="#a6adc8" wrapMode="word">{inlineBuffer}</text>
      </box>,
    );
    inlineBuffer = [];
    bufferAnchorId = undefined;
  };

  const beginInlineBlock = (indent: number, anchorId: string) => {
    if (inlineBuffer.length > 0 && indent !== bufferIndent) flushInline();
    if (inlineBuffer.length === 0) {
      bufferIndent = indent;
      bufferAnchorId = anchorId;
    }
  };

  const appendInlineLines = (nodes: MantInline[]) => {
    for (const segment of visibleInlineSegments(nodes)) {
      inlineBuffer.push(
        ...renderInlineContent(segment, `merged-${inlineKey++}`),
        "\n",
      );
    }
  };

  /**
   * Interactive references need separate Text renderables for mouse events.
   * Each visible fragment therefore receives its own search record and ID.
   */
  const renderNavigationParagraph = (
    nodes: MantInline[],
    blockPath: string,
  ): ReactNode[] => {
    const rendered: ReactNode[] = [];
    let ordinary: MantInline[] = [];
    let searchPartIndex = 0;
    const nextSearchId = () => contentSearchId(
      sectionId,
      searchPath.inline(blockPath, searchPartIndex++),
    );
    const flushOrdinary = () => {
      if (ordinary.length === 0) return;
      const current = ordinary;
      ordinary = [];
      rendered.push(
        <text
          key={`navigation-text-${inlineKey++}`}
          id={nextSearchId()}
          fg="#a6adc8"
          wrapMode="word"
        >
          {renderInlineContent(current, `navigation-${inlineKey}`)}
        </text>,
      );
    };

    for (const node of nodes) {
      if (node.type === "section-reference") {
        flushOrdinary();
        rendered.push(
          <text
            key={`section-reference-${inlineKey++}`}
            id={nextSearchId()}
            fg="#89dceb"
            wrapMode="word"
            selectable={false}
            onMouseDown={(event) => {
              event.stopPropagation();
              onNavigateInternal?.(node.target);
            }}
          >
            <u>{renderInlineContent(node.children, `reference-${inlineKey}`)}</u>
          </text>,
        );
      } else if (node.type === "anchor") {
        flushOrdinary();
        rendered.push(
          <text key={`anchor-${inlineKey++}`} id={contentAnchorId(node.id)} selectable={false}>
            {"\u200b"}
          </text>,
        );
      } else {
        ordinary.push(node);
      }
    }
    flushOrdinary();
    return rendered;
  };

  for (let blockIndex = 0; blockIndex < blocks.length; blockIndex++) {
    const block = blocks[blockIndex]!;
    const blockPath = searchPath.block(parentPath, blockIndex);
    const targetId = contentSearchId(sectionId, blockPath);
    const indent = baseIndent + layoutIndent(block);
    const spacingBefore = layoutSpacing(block);

    if (spacingBefore > 0) {
      flushInline();
      result.push(<box key={`leading-space-${keyCounter++}`} height={spacingBefore} />);
    }

    switch (block.type) {
      case "paragraph":
        if (block.children.some((node) =>
          node.type === "section-reference" || node.type === "anchor"
        )) {
          flushInline();
          result.push(
            <box
              key={`navigation-paragraph-${keyCounter++}`}
              flexDirection="row"
              flexWrap="wrap"
              paddingLeft={indent}
            >
              {renderNavigationParagraph(block.children, blockPath)}
            </box>,
          );
        } else {
          beginInlineBlock(indent, targetId);
          appendInlineLines(block.children);
        }
        break;

      case "preformatted":
        flushInline();
        result.push(
          <box key={`pre-${keyCounter++}`} id={targetId}>
            <Pre children={block.children} block indent={indent} />
          </box>,
        );
        break;

      case "list":
        flushInline();
        result.push(
          <box key={`list-${keyCounter++}`} flexDirection="column">
            {block.items.map((item, itemIndex) => {
              const itemPath = searchPath.listItem(blockPath, itemIndex);
              const marker = block.kind === "ordered"
                ? `${(block.start ?? 1) + itemIndex}. `
                : block.kind === "bullet" ? "• " : "";
              return (
                <box
                  key={`item-${itemIndex}`}
                  flexDirection="row"
                  paddingLeft={indent}
                  paddingTop={itemIndex > 0 && !block.compact ? 1 : 0}
                >
                  {marker && <text fg="#94e2d5">{marker}</text>}
                  <box flexDirection="column" flexGrow={1}>
                    {renderBlockNodes(
                      item.blocks,
                      0,
                      sectionId,
                      itemPath,
                      onNavigateInternal,
                    )}
                  </box>
                </box>
              );
            })}
          </box>,
        );
        break;

      case "definition-list":
        flushInline();
        result.push(
          <box key={`definitions-${keyCounter++}`} flexDirection="column">
            {block.items.map((item, itemIndex) => {
              const itemPath = searchPath.definition(blockPath, itemIndex);
              const inlineTerm = item.inlineTerm === true;
              return (
                <box
                  key={`definition-${itemIndex}`}
                  flexDirection="column"
                  paddingTop={
                    item.spacingBeforeLines
                      ?? (itemIndex > 0 && !block.compact ? 1 : 0)
                  }
                >
                  {inlineTerm
                    ? (
                      <box
                        key="inline-term"
                        flexDirection="row"
                        paddingLeft={indent}
                        {...(item.identity
                          ? { id: contentAnchorId(item.identity.id) }
                          : {})}
                      >
                        <text
                          id={contentSearchId(sectionId, searchPath.term(itemPath, 0))}
                          fg="#cdd6f4"
                          wrapMode="word"
                        >
                          {item.terms.map((term, termIndex) =>
                            renderInlineContent(term, `inline-term-${termIndex}`)
                          )}
                          {" "}
                        </text>
                        <box flexDirection="column" flexGrow={1}>
                          {renderBlockNodes(
                            item.description,
                            0,
                            sectionId,
                            itemPath,
                            onNavigateInternal,
                          )}
                        </box>
                      </box>
                    )
                    : (
                      <>
                        {item.terms.map((term, termIndex) => {
                          const termPath = searchPath.term(itemPath, termIndex);
                          return (
                            <box
                              key={`term-${termIndex}`}
                              {...(termIndex === 0 && item.identity
                                ? { id: contentAnchorId(item.identity.id) }
                                : {})}
                              paddingLeft={indent}
                            >
                              <text
                                id={contentSearchId(sectionId, termPath)}
                                fg="#cdd6f4"
                                wrapMode="word"
                              >
                                {renderInlineContent(term, `term-${termIndex}`)}
                              </text>
                            </box>
                          );
                        })}
                        {item.description.length > 0 && (
                          <box flexDirection="column">
                            {renderBlockNodes(
                              item.description,
                              indent + 4,
                              sectionId,
                              itemPath,
                              onNavigateInternal,
                            )}
                          </box>
                        )}
                      </>
                    )}
                </box>
              );
            })}
          </box>,
        );
        break;

      case "table":
        flushInline();
        result.push(
          <box key={`table-${keyCounter++}`} flexDirection="column" paddingLeft={indent}>
            {block.rows.map((row, rowIndex) => (
              <box key={`row-${rowIndex}`} flexDirection="row">
                {row.cells.map((cell, cellIndex) => (
                  <box key={`cell-${cellIndex}`} flexGrow={1} flexDirection="column">
                    {renderBlockNodes(
                      cell.blocks,
                      0,
                      sectionId,
                      searchPath.cell(searchPath.row(blockPath, rowIndex), cellIndex),
                      onNavigateInternal,
                    )}
                  </box>
                ))}
              </box>
            ))}
          </box>,
        );
        break;

      case "equation":
        flushInline();
        result.push(
          <box key={`equation-${keyCounter++}`} paddingLeft={indent}>
            <text id={targetId} fg="#f9e2af" wrapMode="char">{block.value}</text>
          </box>,
        );
        break;

      case "unsupported":
        flushInline();
        result.push(
          <box key={`unsupported-${keyCounter++}`} paddingLeft={indent}>
            <text id={targetId} fg="#fab387" wrapMode="word">{block.text}</text>
          </box>,
        );
        break;

      case "vertical-space":
        flushInline();
        result.push(
          <box key={`space-${keyCounter++}`} height={Math.max(1, Math.floor(block.lines))} />,
        );
        break;
    }
  }
  flushInline();
  return result;
}
