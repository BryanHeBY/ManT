/**
 * @file Renders cached and Markdown-embedded quick references through one
 * tldr presentation model.
 *
 * The adapters keep provenance and search IDs source-specific. The panel owns
 * only the shared colour, spacing, command, and placeholder presentation.
 */

import { memo } from "react";
import type {
  MantSection,
  TldrCommandPart,
  TldrDocument,
} from "../native";
import { contentId, contentSearchId, TLDR_NAV_ID } from "./ids";
import { flattenInlineText, searchPath } from "./search";
import { embeddedTldrCommandParts } from "./tldr-format";

interface QuickReferenceExample {
  description: string;
  command: string;
  commandParts: TldrCommandPart[];
  descriptionTargetId: string;
  commandTargetId: string;
}

export interface QuickReferencePanelModel {
  contentTargetId: string;
  title: string;
  description: Array<{ text: string; targetId: string }>;
  examples: QuickReferenceExample[];
  moreInformation?: { text: string; targetId: string };
  attribution?: string;
}

function TldrCommand({ parts }: { parts: TldrCommandPart[] }) {
  return (
    <text fg="#cdd6f4" wrapMode="char">
      {parts.map((part, index) => (
        <span key={index} fg={part.type === "placeholder" ? "#f9e2af" : "#cdd6f4"}>
          {part.value}
        </span>
      ))}
    </text>
  );
}

/** Shared visual surface for upstream and document-owned quick references. */
export function QuickReferencePanel({ model }: { model: QuickReferencePanelModel }) {
  return (
    <box
      id={model.contentTargetId}
      flexDirection="column"
      backgroundColor="#28243a"
      border={["top", "right", "bottom", "left"]}
      borderColor="#cba6f7"
      paddingLeft={1}
      paddingRight={1}
      paddingTop={1}
      paddingBottom={1}
    >
      <text fg="#cba6f7"><b>{model.title}</b></text>
      {model.description.map((line, index) => (
        <text
          key={`description-${index}`}
          id={line.targetId}
          fg="#bac2de"
          wrapMode="word"
        >
          {line.text}
        </text>
      ))}
      {model.examples.map((example, index) => (
        <box key={`example-${index}`} flexDirection="column" paddingTop={1}>
          <text id={example.descriptionTargetId} fg="#a6e3a1" wrapMode="word">
            {example.description}
          </text>
          {example.command && (
            <box id={example.commandTargetId} paddingLeft={2}>
              <TldrCommand parts={example.commandParts} />
            </box>
          )}
        </box>
      ))}
      {model.moreInformation && (
        <box paddingTop={1}>
          <text id={model.moreInformation.targetId} fg="#89b4fa" wrapMode="char">
            {`More information: ${model.moreInformation.text}`}
          </text>
        </box>
      )}
      {model.attribution && <text fg="#7f849c">{model.attribution}</text>}
    </box>
  );
}

function cachedTldrModel(page: TldrDocument): QuickReferencePanelModel {
  return {
    contentTargetId: contentId(TLDR_NAV_ID),
    title: `TLDR QUICK REFERENCE · ${page.title}`,
    description: page.description.map((text, index) => ({
      text,
      targetId: contentSearchId(TLDR_NAV_ID, searchPath.tldrDescription(index)),
    })),
    examples: page.examples.map((example, index) => ({
      ...example,
      descriptionTargetId: contentSearchId(
        TLDR_NAV_ID,
        searchPath.tldrExampleDescription(index),
      ),
      commandTargetId: contentSearchId(
        TLDR_NAV_ID,
        searchPath.tldrExampleCommand(index),
      ),
    })),
    ...(page.moreInformation
      ? {
          moreInformation: {
            text: page.moreInformation,
            targetId: contentSearchId(TLDR_NAV_ID, searchPath.tldrMoreInformation()),
          },
        }
      : {}),
    attribution: `tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`,
  };
}

/**
 * Adapt the marker-free list emitted by Markdown quick-reference lowering.
 *
 * Returning undefined leaves unusual user-authored sections on the generic
 * block renderer instead of guessing at their meaning.
 */
export function embeddedTldrModel(section: MantSection): QuickReferencePanelModel | undefined {
  const [block] = section.blocks;
  if (section.blocks.length !== 1 || block?.type !== "list" || block.kind !== "plain") {
    return undefined;
  }

  const examples: QuickReferenceExample[] = [];
  for (let itemIndex = 0; itemIndex < block.items.length; itemIndex++) {
    const item = block.items[itemIndex]!;
    const [descriptionBlock, commandBlock] = item.blocks;
    if (
      item.blocks.length !== 2
      || descriptionBlock?.type !== "paragraph"
      || commandBlock?.type !== "paragraph"
      || commandBlock.children.length !== 1
      || commandBlock.children[0]?.type !== "code"
    ) {
      return undefined;
    }

    const itemPath = searchPath.listItem(searchPath.block("", 0), itemIndex);
    const command = commandBlock.children[0].value;
    examples.push({
      description: flattenInlineText(descriptionBlock.children).replace(/:\s*$/, ""),
      command,
      commandParts: embeddedTldrCommandParts(command),
      descriptionTargetId: contentSearchId(
        section.id,
        searchPath.block(itemPath, 0),
      ),
      commandTargetId: contentSearchId(
        section.id,
        searchPath.block(itemPath, 1),
      ),
    });
  }

  return {
    contentTargetId: contentId(section.id),
    title: section.title.toLocaleUpperCase(),
    description: [],
    examples,
  };
}

/** Renders one cached community page before the authoritative manual. */
function TldrQuickReferenceView({ page }: { page: TldrDocument }) {
  return <QuickReferencePanel model={cachedTldrModel(page)} />;
}

/** TLDR is immutable for the lifetime of one page. */
export const TldrQuickReference = memo(TldrQuickReferenceView);
