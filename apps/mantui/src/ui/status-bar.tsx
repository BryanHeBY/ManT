/**
 * @file Renders the bottom status line and temporary in-page search input.
 *
 * Search state is owned by the application so typing can remain separate from
 * the confirmed query that drives expensive manual-wide highlighting.
 */

import type { InputRenderable } from "@opentui/core";

export interface SearchBarProps {
  inputRef: { current: InputRenderable | null };
  appliedQuery: string;
  isEditing: boolean;
  matchCount: number;
  matchIndex: number;
  onSubmit: (value: string) => void;
}

export function SearchBar({
  inputRef,
  appliedQuery,
  isEditing,
  matchCount,
  matchIndex,
  onSubmit,
}: SearchBarProps) {
  // Text itself stays inside OpenTUI's native InputRenderable so large pages
  // do not reconcile on every keystroke. App owns the explicit editing flag:
  // input callbacks may arrive after submit, so they cannot safely determine
  // whether a confirmed query has subsequently been changed.
  const hasAppliedQuery = appliedQuery.length > 0 && !isEditing;
  const hasNoMatches = hasAppliedQuery && matchCount === 0;
  // OpenTUI React emits the current string, while the inherited core option
  // type still mentions an empty SubmitEvent. Accept both shapes at this
  // adapter boundary and read the renderable as a compatibility fallback.
  const submitCurrentValue = (value: unknown) => {
    onSubmit(typeof value === "string" ? value : inputRef.current?.value ?? "");
  };
  return (
    <box height={1} flexDirection="row" backgroundColor="#181825" paddingLeft={1} paddingRight={1}>
      <text fg="#f9e2af">Find:</text>
      <box width={1} />
      <input
        ref={inputRef}
        flexGrow={1}
        focused
        placeholder="Search this page"
        placeholderColor="#6c7086"
        backgroundColor="#313244"
        focusedBackgroundColor="#313244"
        textColor="#cdd6f4"
        focusedTextColor="#cdd6f4"
        onSubmit={submitCurrentValue}
      />
      <box width={1} />
      {hasNoMatches ? (
        <box flexDirection="row">
          <box backgroundColor="#f38ba8" paddingLeft={1} paddingRight={1}>
            <text fg="#11111b"><b>No matches</b></text>
          </box>
          <text fg="#f38ba8">  Edit query · Esc close</text>
        </box>
      ) : (
        <text fg="#7f849c">
          {!hasAppliedQuery
            ? "Enter search · Esc cancel"
            : `${matchIndex + 1}/${matchCount}  Enter next · Esc close`}
        </text>
      )}
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
          : `${visibleSectionCount} visible sections${hasTldr ? " · TLDR cached" : ""}`}
      </text>
    </box>
  );
}
