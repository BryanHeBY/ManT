/**
 * @file Declares the renderer-neutral document model shared across Mant.
 */

export interface SectionNode {
  id: string;
  title: string;
  level: number;
  blocks: BlockNode[];
  children: SectionNode[];
}

/** One term/description pair from an HTML definition list. */
export interface DefinitionListItem {
  terms: InlineNode[][];
  description: InlineNode[];
}

export type BlockNode =
  | { type: "paragraph"; children: InlineNode[]; indent: number }
  | { type: "pre"; children: InlineNode[]; indent: number }
  | { type: "list"; items: InlineNode[][]; indent: number }
  | { type: "definition-list"; items: DefinitionListItem[]; indent: number }
  | { type: "spacer"; indent: number };

export type InlineNode =
  | { type: "text"; content: string }
  | { type: "bold"; children: InlineNode[] }
  | { type: "italic"; children: InlineNode[] }
  | { type: "code"; children: InlineNode[] }
  | { type: "break" };

export interface ManPage {
  topic: string;
  html: string;
  sections: SectionNode[];
}
