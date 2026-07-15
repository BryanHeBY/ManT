import {
  createCliRenderer,
  type ScrollBoxRenderable,
  type TextRenderable,
} from "@opentui/core";
import { createRoot, useKeyboard } from "@opentui/react";
import { useMemo, useRef, useState, type ReactNode } from "react";
import type { QueryResult } from "../query";
import type { BlockNode, InlineNode, SectionNode } from "../core";
import { Pre } from "./Pre";

interface AppProps {
  result: QueryResult;
  onQuit: () => void;
}

interface FlatNode {
  node: SectionNode;
  depth: number;
  hasChildren: boolean;
}

function flattenVisibleNodes(
  nodes: SectionNode[],
  expanded: Set<string>,
  depth = 0
): FlatNode[] {
  const result: FlatNode[] = [];
  for (const node of nodes) {
    const hasChildren = node.children.length > 0;
    result.push({ node, depth, hasChildren });
    if (hasChildren && expanded.has(node.id)) {
      result.push(...flattenVisibleNodes(node.children, expanded, depth + 1));
    }
  }
  return result;
}

function findNodeById(nodes: SectionNode[], id: string): SectionNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    const found = findNodeById(node.children, id);
    if (found) return found;
  }
  return null;
}

function navId(id: string): string {
  return `nav-${id}`;
}

function contentId(id: string): string {
  return `content-${id}`;
}

function splitByBreak(nodes: InlineNode[]): InlineNode[][] {
  const segments: InlineNode[][] = [[]];
  for (const node of nodes) {
    if (node.type === "break") {
      segments.push([]);
    } else {
      segments[segments.length - 1]!.push(node);
    }
  }
  return segments.filter((s) => s.length > 0).map(trimSegmentWhitespace);
}

function trimSegmentWhitespace(nodes: InlineNode[]): InlineNode[] {
  if (nodes.length === 0) return nodes;

  const trimmed = [...nodes];
  const first = trimmed[0];
  if (first?.type === "text") {
    first.content = first.content.replace(/^\s+/, "");
  }

  const last = trimmed[trimmed.length - 1];
  if (last?.type === "text") {
    last.content = last.content.replace(/\s+$/, "");
  }

  return trimmed;
}

/**
 * Merges consecutive paragraph and list blocks into a single `<text>` element
 * to avoid creating one TextBuffer per block (which can exceed the native
 * TextBuffer limit on large man pages like gcc ~16k blocks).
 *
 * Pre blocks are kept separate because they need char-wrap mode and a
 * distinct visual style.
 */
function renderBlockNodes(blocks: BlockNode[], baseIndent = 0): ReactNode[] {
  const result: ReactNode[] = [];
  let inlineBuffer: ReactNode[] = [];
  let bufferIndent = 0;
  let keyCounter = 0;
  let inlineKey = 0;

  function renderInlineNodes(nodes: InlineNode[]): ReactNode[] {
    return nodes.map((node) => {
      const key = inlineKey++;
      switch (node.type) {
        case "text":
          return node.content;
        case "bold":
          return (
            <span key={key} fg="#cdd6f4">
              <b>{renderInlineNodes(node.children)}</b>
            </span>
          );
        case "italic":
          return (
            <span key={key} fg="#7f849c">
              <u>{renderInlineNodes(node.children)}</u>
            </span>
          );
        case "code":
          return <Pre key={key} children={node.children} />;
        case "break":
          return null;
        default:
          return null;
      }
    });
  }

  function flushInline() {
    if (inlineBuffer.length === 0) return;
    result.push(
      <box key={`merged-${keyCounter++}`} paddingLeft={bufferIndent} shouldFill={true}>
        <text fg="#a6adc8" wrapMode="word">
          {inlineBuffer}
        </text>
      </box>
    );
    inlineBuffer = [];
  }

  for (const block of blocks) {
    switch (block.type) {
      case "paragraph": {
        if (inlineBuffer.length > 0 && baseIndent + block.indent !== bufferIndent) {
          flushInline();
        }
        if (inlineBuffer.length === 0) {
          bufferIndent = baseIndent + block.indent;
        }
        const segments = splitByBreak(block.children);
        for (const segment of segments) {
          inlineBuffer.push(...renderInlineNodes(segment));
          inlineBuffer.push("\n");
        }
        break;
      }
      case "list": {
        if (inlineBuffer.length > 0 && baseIndent + block.indent !== bufferIndent) {
          flushInline();
        }
        if (inlineBuffer.length === 0) {
          bufferIndent = baseIndent + block.indent;
        }
        for (const item of block.items) {
          inlineBuffer.push(<span key={`bullet-${inlineKey++}`} fg="#94e2d5">{"• "}</span>);
          inlineBuffer.push(...renderInlineNodes(item));
          inlineBuffer.push("\n");
        }
        break;
      }
      case "pre": {
        flushInline();
        result.push(
          <Pre
            key={`pre-${keyCounter++}`}
            children={block.children}
            block
            indent={baseIndent + block.indent}
          />
        );
        break;
      }
      default:
        break;
    }
  }
  flushInline();
  return result;
}

function SectionContent({ node, baseIndent = 3 }: { node: SectionNode; baseIndent?: number }) {
  return (
    <box flexDirection="column" gap={0}>
      <text id={contentId(node.id)} fg="#94e2d5">
        <b>{node.title}</b>
      </text>
      {renderBlockNodes(node.blocks, baseIndent)}
      <box flexDirection="column" gap={0}>
        {node.children.map((child: SectionNode) => (
          <SectionContent key={child.id} node={child} baseIndent={baseIndent + 4} />
        ))}
      </box>
    </box>
  );
}

export function App({ result, onQuit }: AppProps) {
  const [selectedId, setSelectedId] = useState<string>(
    result.sections[0]?.id ?? ""
  );
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    const initial = new Set<string>();
    for (const section of result.sections) {
      initial.add(section.id);
    }
    return initial;
  });
  const contentScrollRef = useRef<ScrollBoxRenderable | null>(null);

  const visibleNodes = useMemo(
    () => flattenVisibleNodes(result.sections, expanded),
    [result.sections, expanded]
  );

  const scrollToNode = (id: string) => {
    contentScrollRef.current?.scrollChildIntoView(contentId(id));
  };

  const attachSectionClick = (id: string, hasChildren: boolean) => {
    return (el: TextRenderable | null) => {
      if (!el) return;
      el.onMouseDown = () => {
        if (hasChildren && selectedId === id) {
          toggleExpanded(id);
        } else {
          setSelectedId(id);
          if (hasChildren) {
            setExpanded((prev) => {
              const next = new Set(prev);
              next.add(id);
              return next;
            });
          }
        }
        scrollToNode(id);
      };
    };
  };

  useKeyboard((e) => {
    if (e.name === "q" || e.name === "Q") {
      onQuit();
      return;
    }

    const currentIndex = visibleNodes.findIndex(
      (n) => n.node.id === selectedId
    );

    if (e.name === "j" || e.name === "down") {
      const next = Math.min(currentIndex + 1, visibleNodes.length - 1);
      if (next >= 0) {
        const id = visibleNodes[next]!.node.id;
        setSelectedId(id);
        scrollToNode(id);
      }
    } else if (e.name === "k" || e.name === "up") {
      const next = Math.max(currentIndex - 1, 0);
      if (next >= 0) {
        const id = visibleNodes[next]!.node.id;
        setSelectedId(id);
        scrollToNode(id);
      }
    } else if (e.name === "h" || e.name === "left") {
      const node = findNodeById(result.sections, selectedId);
      if (node && expanded.has(node.id) && node.children.length > 0) {
        setExpanded((prev) => {
          const next = new Set(prev);
          next.delete(node.id);
          return next;
        });
      }
    } else if (e.name === "l" || e.name === "right") {
      const node = findNodeById(result.sections, selectedId);
      if (node && node.children.length > 0 && !expanded.has(node.id)) {
        setExpanded((prev) => {
          const next = new Set(prev);
          next.add(node.id);
          return next;
        });
      }
    } else if (e.name === "d" || e.name === "pagedown") {
      contentScrollRef.current?.scrollBy({ x: 0, y: 10 }, "step");
    } else if (e.name === "u" || e.name === "pageup") {
      contentScrollRef.current?.scrollBy({ x: 0, y: -10 }, "step");
    }
  });

  function toggleExpanded(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <box flexDirection="column" shouldFill={true}>
      <box flexDirection="row" shouldFill={true} flexGrow={1}>
        <box
          width={30}
          flexDirection="column"
          backgroundColor="#1e1e2e"
          padding={1}
          gap={1}
        >
          <text fg="#94e2d5"><b>{`MAN: ${result.topic}`}</b></text>
          <scrollbox flexGrow={1} scrollY>
            {visibleNodes.map(({ node, depth, hasChildren }) => {
              const isSelected = node.id === selectedId;
              const indent = "  ".repeat(depth);
              const prefix = hasChildren
                ? expanded.has(node.id)
                  ? "▾ "
                  : "▸ "
                : "  ";
              const label = `${indent}${prefix}${node.title}`;
              return isSelected ? (
                <text
                  key={navId(node.id)}
                  id={navId(node.id)}
                  ref={attachSectionClick(node.id, hasChildren)}
                  fg="#cdd6f4"
                  bg="#313244"
                >
                  {label}
                </text>
              ) : (
                <text
                  key={navId(node.id)}
                  id={navId(node.id)}
                  ref={attachSectionClick(node.id, hasChildren)}
                  fg="#6c7086"
                >
                  {label}
                </text>
              );
            })}
          </scrollbox>
        </box>

        <box
          flexGrow={1}
          flexDirection="column"
          paddingLeft={1}
          paddingTop={1}
          paddingBottom={1}
          paddingRight={1}
        >
          <scrollbox ref={contentScrollRef} flexGrow={1} scrollY focusable>
            <box flexDirection="column" gap={1}>
              {result.sections.map((node) => (
                <SectionContent key={node.id} node={node} />
              ))}
            </box>
          </scrollbox>
        </box>
      </box>

      <box
        height={1}
        flexDirection="row"
        backgroundColor="#1e1e2e"
        paddingLeft={1}
        paddingRight={1}
      >
        <text fg="#6c7086">
          <span fg="#cdd6f4">q</span> quit
          {"  "}
          <span fg="#cdd6f4">j/k</span> nav
          {"  "}
          <span fg="#cdd6f4">h/l</span> fold
          {"  "}
          <span fg="#cdd6f4">d/u</span> scroll
        </text>
      </box>
    </box>
  );
}

export async function runTui(result: QueryResult): Promise<void> {
  const renderer = await createCliRenderer({
    exitOnCtrlC: true,
    useMouse: true,
  });

  const quit = () => renderer.destroy();
  createRoot(renderer).render(<App result={result} onQuit={quit} />);
}
