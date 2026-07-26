/**
 * @file Renders cached and document-owned quick references through one model.
 *
 * Rust normalizes both sources into TldrDocument. This component only varies
 * attribution according to provenance; layout and search IDs stay identical.
 */

import { memo } from "react";
import type { TldrCommandPart, TldrDocument } from "../native";
import { contentId, contentSearchId, TLDR_NAV_ID } from "./ids";
import { renderCodeSpans } from "./Pre";
import { searchPath } from "./search";

interface QuickReferenceExample {
  description: string;
  command: string;
  commandParts: TldrCommandPart[];
  descriptionTargetId: string;
  commandTargetId: string;
}

interface QuickReferencePanelModel {
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
        part.type === "placeholder"
          ? <span key={index} fg="#f9e2af">{part.value}</span>
          : renderCodeSpans(part.value, `tldr-command-${index}`)
      ))}
    </text>
  );
}

/** Shared visual surface for upstream and document-owned quick references. */
function QuickReferencePanel({ model }: { model: QuickReferencePanelModel }) {
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

function tldrModel(page: TldrDocument): QuickReferencePanelModel {
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
    ...(page.origin === "embedded"
      ? {}
      : {
          attribution: `tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`,
        }),
  };
}

/** Renders one normalized quick reference before the primary document. */
function TldrQuickReferenceView({ page }: { page: TldrDocument }) {
  return <QuickReferencePanel model={tldrModel(page)} />;
}

/** TLDR is immutable for the lifetime of one page. */
export const TldrQuickReference = memo(TldrQuickReferenceView);
