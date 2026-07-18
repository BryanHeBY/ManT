/**
 * @file Renders the bottom status line and temporary in-page search input.
 *
 * Search state is owned by the application so typing can remain separate from
 * the confirmed query that drives expensive manual-wide highlighting.
 */

import type { InputRenderable } from "@opentui/core";

export interface SearchBarProps {
  inputRef: { current: InputRenderable | null };
  draft: string;
  appliedQuery: string;
  matchCount: number;
  matchIndex: number;
  onDraftChange: (value: string) => void;
  onSubmit: () => void;
}

export function SearchBar({
  inputRef,
  draft,
  appliedQuery,
  matchCount,
  matchIndex,
  onDraftChange,
  onSubmit,
}: SearchBarProps) {
  return (
    <box height={1} flexDirection="row" backgroundColor="#181825" paddingLeft={1} paddingRight={1}>
      <text fg="#f9e2af">Find:</text>
      <box width={1} />
      <input
        ref={inputRef}
        flexGrow={1}
        value={draft}
        focused
        placeholder="Search this page"
        placeholderColor="#6c7086"
        backgroundColor="#313244"
        focusedBackgroundColor="#313244"
        textColor="#cdd6f4"
        focusedTextColor="#cdd6f4"
        onInput={onDraftChange}
        onSubmit={onSubmit}
      />
      <box width={1} />
      <text fg="#7f849c">
        {draft !== appliedQuery
          ? "Enter search · Esc cancel"
          : matchCount > 0
            ? `${matchIndex + 1}/${matchCount}  Enter next · Esc close`
            : "0 matches  Esc close"}
      </text>
    </box>
  );
}

export interface StatusNavigationItem {
  id: string;
  title: string;
}

export interface ManualStatusBarProps {
  navigationItems: StatusNavigationItem[];
  selectedId: string;
  visibleSectionCount: number;
  hasTldr: boolean;
  searchQuery: string;
  searchMatchCount: number;
}

/** Summarises the selected section and active search when the input is closed. */
export function ManualStatusBar({
  navigationItems,
  selectedId,
  visibleSectionCount,
  hasTldr,
  searchQuery,
  searchMatchCount,
}: ManualStatusBarProps) {
  const selectedIndex = navigationItems.findIndex((item) => item.id === selectedId);
  const selectedItem = navigationItems[selectedIndex];
  return (
    <box height={1} flexDirection="row" backgroundColor="#1e1e2e" paddingLeft={1} paddingRight={1}>
      <text fg="#a6adc8" truncate wrapMode="none">
        {navigationItems.length > 0
          ? `${selectedIndex + 1}/${navigationItems.length} · ${selectedItem?.title ?? ""}`
          : "No content"}
      </text>
      <box flexGrow={1} />
      <text fg="#6c7086" truncate wrapMode="none">
        {searchQuery && searchMatchCount > 0
          ? `Find “${searchQuery}” · ${searchMatchCount} matches`
          : `${visibleSectionCount} visible manual sections${hasTldr ? " · TLDR cached" : ""}`}
      </text>
    </box>
  );
}
