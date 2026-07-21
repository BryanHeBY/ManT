/**
 * @file Renders the optional tldr quick reference before a local manual.
 *
 * The native layer owns cache discovery and parsing; this component only
 * supplies presentation, placeholder styling, and stable search anchors.
 */

import { memo } from "react";
import type { TldrCommandPart, TldrDocument } from "../native";
import { contentId, contentSearchId, TLDR_NAV_ID } from "./ids";
import { searchPath } from "./search";

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

/** Renders cached community examples before the authoritative man page. */
function TldrQuickReferenceView({ page }: { page: TldrDocument }) {
  return (
    <box
      id={contentId(TLDR_NAV_ID)}
      flexDirection="column"
      backgroundColor="#28243a"
      border={["top", "right", "bottom", "left"]}
      borderColor="#cba6f7"
      paddingLeft={1}
      paddingRight={1}
      paddingTop={1}
      paddingBottom={1}
    >
      <text fg="#cba6f7"><b>{`TLDR QUICK REFERENCE · ${page.title}`}</b></text>
      {page.description.map((line, index) => (
        <text
          key={`description-${index}`}
          id={contentSearchId(TLDR_NAV_ID, searchPath.tldrDescription(index))}
          fg="#bac2de"
          wrapMode="word"
        >
          {line}
        </text>
      ))}
      {page.examples.map((example, index) => (
        <box key={`example-${index}`} flexDirection="column" paddingTop={1}>
          <text
            id={contentSearchId(TLDR_NAV_ID, searchPath.tldrExampleDescription(index))}
            fg="#a6e3a1"
            wrapMode="word"
          >
            {example.description}
          </text>
          {example.command && (
            <box
              id={contentSearchId(TLDR_NAV_ID, searchPath.tldrExampleCommand(index))}
              paddingLeft={2}
            >
              <TldrCommand parts={example.commandParts} />
            </box>
          )}
        </box>
      ))}
      {page.moreInformation && (
        <box paddingTop={1}>
          <text
            id={contentSearchId(TLDR_NAV_ID, searchPath.tldrMoreInformation())}
            fg="#89b4fa"
            wrapMode="char"
          >
            {`More information: ${page.moreInformation}`}
          </text>
        </box>
      )}
      <text fg="#7f849c">{`tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`}</text>
    </box>
  );
}

/** TLDR is immutable for the lifetime of one page. */
export const TldrQuickReference = memo(TldrQuickReferenceView);
