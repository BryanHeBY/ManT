/**
 * @file Builds the shared hierarchical section model used by all HTML renderers.
 *
 * Renderers feed headings and content blocks into this small stateful adapter;
 * it owns level-based nesting and stable section identifiers.
 */

import type { BlockNode, SectionNode } from "./types";

/** Incrementally assembles a document-order section tree. */
export class SectionTree {
  private readonly sections: SectionNode[] = [];
  private readonly stack: SectionNode[] = [];
  private nextId = 0;

  /** Removes the current section and siblings whenever a peer/parent begins. */
  private closeSections(level: number): void {
    while (this.stack.length > 0 && this.stack[this.stack.length - 1]!.level >= level) {
      this.stack.pop();
    }
  }

  currentSection(): SectionNode | null {
    return this.stack[this.stack.length - 1] ?? null;
  }

  addBlock(block: BlockNode): void {
    this.currentSection()?.blocks.push(block);
  }

  pushSection(title: string, level: number): SectionNode {
    this.closeSections(level);
    const section: SectionNode = {
      id: `section-${this.nextId++}`,
      title,
      level,
      blocks: [],
      children: [],
    };

    const parent = this.currentSection();
    if (parent) parent.children.push(section);
    else this.sections.push(section);

    this.stack.push(section);
    return section;
  }

  getSections(): SectionNode[] {
    return this.sections;
  }
}
